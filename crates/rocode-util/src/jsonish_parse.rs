// =============================================================================
// Streaming ToolCall Parser — Production-Grade Implementation
// =============================================================================
//
// A streaming JSON parser designed for LLM tool-call recovery. Handles:
//   - Token-by-token streaming with byte-offset tracking
//   - Multi-object detection (tool call arrays)
//   - Array [] and object {} bracket balancing
//   - State-machine-aware repair (single quotes, control chars, trailing commas)
//   - Truncated JSON aggressive close
//   - Schema-aware tool detection with scoring
//
// KNOWN LIMITATION: The quote-tracking state machine (`in_string` toggled by `"`)
// will desync when string content contains unescaped double quotes (e.g. HTML
// attributes like `lang="zh-CN"`). This affects `escape_control_chars_in_strings`
// and `balance_brackets_stateful`. For such cases, the structural recovery in
// `util::json::recover_tool_call_ultra` should be used as a fallback.
// =============================================================================

use serde_json::Value;
use std::fmt;

// ─── Tool Schema ────────────────────────────────────────────────────────────

/// Describes a tool's JSON structure for matching parsed objects to tools.
#[derive(Clone, Debug)]
pub struct ToolSchema {
    pub name: String,
    /// Keys that must be present (higher match weight).
    pub required_keys: Vec<String>,
    /// Optional keys (lower match weight).
    pub optional_keys: Vec<String>,
}

// ─── Parse Result ───────────────────────────────────────────────────────────

/// Successful parse result with diagnostics.
#[derive(Debug, Clone)]
pub struct ToolParseResult {
    pub tool_name: String,
    pub value: Value,
    /// Byte range of this object in the original buffer.
    pub span: (usize, usize),
    /// Repair operations applied (for diagnostics).
    pub repairs: Vec<String>,
}

/// Parse failure diagnostics.
#[derive(Debug, Clone)]
pub enum ParseError {
    /// No JSON object found in buffer.
    NoObject,
    /// JSON structure found but repair failed.
    InvalidJson {
        repaired: String,
        serde_error: String,
    },
    /// Valid JSON but no tool schema matched.
    NoToolMatch { value: Value },
}
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NoObject => write!(f, "No JSON object found in buffer"),
            ParseError::InvalidJson {
                repaired,
                serde_error,
            } => write!(
                f,
                "JSON repair failed: {} (repaired: {}...)",
                serde_error,
                &repaired[..repaired.len().min(100)]
            ),
            ParseError::NoToolMatch { .. } => write!(f, "No tool schema matched"),
        }
    }
}

// ─── Tracked Object ─────────────────────────────────────────────────────────

/// Tracks a top-level JSON object discovered in the buffer.
#[derive(Clone, Debug)]
struct TrackedObject {
    /// Byte offset of the opening `{`.
    start: usize,
    /// Byte offset past the closing `}`, if seen.
    end: Option<usize>,
}

// ─── Scanner State ──────────────────────────────────────────────────────────

/// Character-level state machine for JSON structure tracking.
#[derive(Clone, Debug)]
struct ScannerState {
    brace_depth: i32,
    bracket_depth: i32,
    in_string: bool,
    escape: bool,
    byte_offset: usize,
}

impl ScannerState {
    fn new() -> Self {
        Self {
            brace_depth: 0,
            bracket_depth: 0,
            in_string: false,
            escape: false,
            byte_offset: 0,
        }
    }
}

// ─── Main Parser ────────────────────────────────────────────────────────────

pub struct StreamingToolParser {
    buffer: String,
    state: ScannerState,
    objects: Vec<TrackedObject>,
    schemas: Vec<ToolSchema>,
}
impl StreamingToolParser {
    pub fn new(schemas: Vec<ToolSchema>) -> Self {
        Self {
            buffer: String::new(),
            state: ScannerState::new(),
            objects: Vec::new(),
            schemas,
        }
    }

    /// Current buffer content (for diagnostics).
    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    /// Number of top-level objects tracked so far.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    // ── Push delta ──────────────────────────────────────────────────────

    /// Push streaming delta into the parser. Core entry point.
    pub fn push(&mut self, delta: &str) {
        let base_offset = self.buffer.len();
        self.buffer.push_str(delta);

        let mut local_byte = 0usize;
        for ch in delta.chars() {
            let char_byte_offset = base_offset + local_byte;
            self.scan_char(ch, char_byte_offset);
            local_byte += ch.len_utf8();
        }

        self.state.byte_offset = self.buffer.len();
    }

    /// State machine: process a single character.
    fn scan_char(&mut self, ch: char, byte_offset: usize) {
        if self.state.in_string {
            if self.state.escape {
                self.state.escape = false;
                return;
            }
            if ch == '\\' {
                self.state.escape = true;
                return;
            }
            if ch == '"' {
                self.state.in_string = false;
            }
            return;
        }

        match ch {
            '"' => {
                self.state.in_string = true;
            }
            '{' => {
                if self.state.brace_depth == 0 && self.state.bracket_depth == 0 {
                    self.objects.push(TrackedObject {
                        start: byte_offset,
                        end: None,
                    });
                }
                self.state.brace_depth += 1;
            }
            '}' => {
                self.state.brace_depth = (self.state.brace_depth - 1).max(0);
                if self.state.brace_depth == 0 && self.state.bracket_depth == 0 {
                    if let Some(obj) = self.objects.last_mut() {
                        if obj.end.is_none() {
                            obj.end = Some(byte_offset + ch.len_utf8());
                        }
                    }
                }
            }
            '[' => {
                self.state.bracket_depth += 1;
            }
            ']' => {
                self.state.bracket_depth = (self.state.bracket_depth - 1).max(0);
            }
            _ => {}
        }
    }

    // ── Try parse (partial, tolerant) ───────────────────────────────────

    /// Try to parse the last discovered object. Safe to call mid-stream.
    pub fn try_parse(&self) -> Result<ToolParseResult, ParseError> {
        self.try_parse_object(self.objects.len().saturating_sub(1))
    }

    /// Try to parse all discovered objects.
    pub fn try_parse_all(&self) -> Vec<Result<ToolParseResult, ParseError>> {
        (0..self.objects.len())
            .map(|i| self.try_parse_object(i))
            .collect()
    }
    fn try_parse_object(&self, index: usize) -> Result<ToolParseResult, ParseError> {
        let obj = self.objects.get(index).ok_or(ParseError::NoObject)?;

        let end = obj.end.unwrap_or(self.buffer.len());
        let slice = &self.buffer[obj.start..end];

        let mut repairs = Vec::new();
        let repaired = repair_json(slice, false, &mut repairs);

        let value: Value =
            serde_json::from_str(&repaired).map_err(|e| ParseError::InvalidJson {
                repaired: repaired.clone(),
                serde_error: e.to_string(),
            })?;

        let tool_name = detect_tool(&value, &self.schemas).ok_or(ParseError::NoToolMatch {
            value: value.clone(),
        })?;

        Ok(ToolParseResult {
            tool_name,
            value,
            span: (obj.start, end),
            repairs,
        })
    }

    // ── Finalize (aggressive) ───────────────────────────────────────────

    /// Call when stream ends. Uses more aggressive repair strategies.
    pub fn finalize(&self) -> Result<ToolParseResult, ParseError> {
        self.finalize_object(self.objects.len().saturating_sub(1))
    }

    /// Finalize all discovered objects.
    pub fn finalize_all(&self) -> Vec<Result<ToolParseResult, ParseError>> {
        (0..self.objects.len())
            .map(|i| self.finalize_object(i))
            .collect()
    }

    fn finalize_object(&self, index: usize) -> Result<ToolParseResult, ParseError> {
        let obj = self.objects.get(index).ok_or(ParseError::NoObject)?;

        let end = obj.end.unwrap_or(self.buffer.len());
        let slice = &self.buffer[obj.start..end];

        let mut repairs = Vec::new();
        let repaired = repair_json(slice, true, &mut repairs);

        let value: Value =
            serde_json::from_str(&repaired).map_err(|e| ParseError::InvalidJson {
                repaired: repaired.clone(),
                serde_error: e.to_string(),
            })?;

        let tool_name = detect_tool(&value, &self.schemas).ok_or(ParseError::NoToolMatch {
            value: value.clone(),
        })?;

        Ok(ToolParseResult {
            tool_name,
            value,
            span: (obj.start, end),
            repairs,
        })
    }

    /// Reset parser state for reuse.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.state = ScannerState::new();
        self.objects.clear();
    }
}
// =============================================================================
// Phase 0: SANITIZE — strip framing noise around the JSON
// =============================================================================

/// Remove non-JSON framing that LLMs wrap around tool-call output.
/// Handles: BOM (A5), ANSI escapes (A6), XML/HTML wrappers (A7),
/// markdown fences (A1/A8), trailing semicolons (D7).
fn sanitize_input(input: &str, repairs: &mut Vec<String>) -> String {
    let mut s = input.to_string();

    // ── BOM (A5) ──
    if s.starts_with('\u{feff}') {
        s = s.trim_start_matches('\u{feff}').to_string();
        repairs.push("stripped BOM".into());
    }

    // ── ANSI escape sequences (A6) ──
    // Matches: ESC[ ... m  (SGR sequences — the vast majority of ANSI codes)
    // Also: ESC[ ... [A-Z] for cursor movement, etc.
    let ansi_len = s.len();
    s = strip_ansi_escapes(&s);
    if s.len() != ansi_len {
        repairs.push("stripped ANSI escape sequences".into());
    }

    // ── XML/HTML tool wrappers (A7) ──
    // Common patterns: <tool_call>...</tool_call>, <function_call>...</function_call>,
    // <json>...</json>, <tool_input>...</tool_input>
    let wrapper_tags = [
        "tool_call",
        "function_call",
        "tool_input",
        "json",
        "arguments",
        "tool_result",
    ];
    for tag in &wrapper_tags {
        let open = format!("<{}>", tag);
        let close = format!("</{}>", tag);
        // Also handle <tag ...> with attributes
        let open_attr = format!("<{} ", tag);
        if let Some(start) = s.find(&open).or_else(|| s.find(&open_attr)) {
            let content_start = s[start..].find('>').map(|i| start + i + 1);
            let content_end = s.rfind(&close);
            if let (Some(cs), Some(ce)) = (content_start, content_end) {
                if cs < ce {
                    s = s[cs..ce].to_string();
                    repairs.push(format!("stripped <{}> wrapper", tag));
                    break;
                }
            }
        }
    }

    // ── Markdown code fences (A1/A8) ──
    let trimmed = s.trim();
    if trimmed.starts_with("```") {
        // Find end of first fence line
        if let Some(nl) = trimmed.find('\n') {
            let after_fence = &trimmed[nl + 1..];
            // Find closing fence
            if let Some(close_pos) = after_fence.rfind("```") {
                s = after_fence[..close_pos].trim().to_string();
                repairs.push("stripped markdown code fences".into());
            } else {
                // No closing fence — just strip the opening line
                s = after_fence.trim().to_string();
                repairs.push("stripped markdown opening fence".into());
            }
        }
    }

    // ── Trailing semicolons (D7) ──
    let trimmed = s.trim_end();
    if trimmed.ends_with(';') {
        s = trimmed.trim_end_matches(';').to_string();
        repairs.push("stripped trailing semicolons".into());
    }

    s.trim().to_string()
}

/// Strip ANSI escape sequences (CSI sequences: ESC[ ... final_byte)
fn strip_ansi_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Check for CSI sequence: ESC [
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                              // Consume parameter bytes (0x30-0x3F) and intermediate bytes (0x20-0x2F)
                              // until final byte (0x40-0x7E)
                loop {
                    match chars.next() {
                        Some(c) if ('\x40'..='\x7e').contains(&c) => break,
                        Some(_) => continue,
                        None => break,
                    }
                }
                continue;
            }
            // OSC sequence: ESC ]
            if chars.peek() == Some(&']') {
                chars.next();
                // Consume until ST (ESC \ or BEL \x07)
                loop {
                    match chars.next() {
                        Some('\x07') => break,
                        Some('\x1b') if chars.peek() == Some(&'\\') => {
                            chars.next();
                            break;
                        }
                        Some(_) => continue,
                        None => break,
                    }
                }
                continue;
            }
            // Simple two-byte escape: ESC + single char
            if chars.peek().is_some() {
                chars.next();
            }
            continue;
        }
        out.push(ch);
    }
    out
}

// =============================================================================
// Phase 1: NORMALIZE — convert non-standard syntax to valid JSON
// =============================================================================

/// Convert non-standard JSON-like syntax to valid JSON.
/// Handles: D2 (unquoted keys), D3/D4 (comments), D5 (hex numbers),
/// D6 (Infinity/NaN), D8 (Python literals), D10 (plus prefix).
/// Single quotes (D1) are handled separately in convert_single_quotes.
fn normalize_syntax(input: &str, repairs: &mut Vec<String>) -> String {
    let mut s = input.to_string();

    // ── Strip comments (D3/D4) — must run before other transforms ──
    let before_comments = s.len();
    s = strip_comments(&s);
    if s.len() != before_comments {
        repairs.push("stripped comments".into());
    }

    // ── Python literals (D8) — True/False/None → true/false/null ──
    // Only replace outside strings
    let before_py = s.clone();
    s = replace_python_literals(&s);
    if s != before_py {
        repairs.push("converted Python literals to JSON".into());
    }

    // ── Infinity/NaN (D6) → null ──
    let before_inf = s.clone();
    s = replace_special_numbers(&s);
    if s != before_inf {
        repairs.push("replaced Infinity/NaN with null".into());
    }

    // ── Unquoted keys (D2) ──
    let before_keys = s.clone();
    s = quote_unquoted_keys(&s);
    if s != before_keys {
        repairs.push("quoted unquoted keys".into());
    }

    // ── Hex numbers (D5) ──
    let before_hex = s.clone();
    s = convert_hex_numbers(&s);
    if s != before_hex {
        repairs.push("converted hex numbers to decimal".into());
    }

    // ── Plus prefix on numbers (D10) ──
    let before_plus = s.clone();
    s = strip_plus_prefix(&s);
    if s != before_plus {
        repairs.push("stripped plus prefix from numbers".into());
    }

    s
}

/// Strip JavaScript-style comments: /* ... */ and // ... \n
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // Block comment: /* ... */
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() {
                if chars[i] == '*' && chars[i + 1] == '/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            // If we ran off the end without finding */, skip remaining
            if i >= chars.len() {
                break;
            }
            out.push(' '); // Replace comment with space to preserve token separation
            continue;
        }

        // Line comment: // ... \n
        if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            // Keep the newline
            if i < chars.len() {
                out.push('\n');
                i += 1;
            }
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Replace Python True/False/None with JSON true/false/null (outside strings).
fn replace_python_literals(input: &str) -> String {
    // We need to be careful to only replace these as standalone tokens,
    // not inside strings or as parts of identifiers.
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    // Pre-compute byte offsets to avoid O(n²) char_indices().nth(i) lookups
    let byte_offsets: Vec<usize> = input.char_indices().map(|(b, _)| b).collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
            } else if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // Check for Python literals at word boundary
        let rest = &input[byte_offsets[i]..];
        if let Some((py, json)) = match_python_literal(rest) {
            // Verify it's at a word boundary (not part of a larger identifier)
            let prev_is_boundary = i == 0 || !chars[i - 1].is_alphanumeric();
            let next_idx = i + py.chars().count();
            let next_is_boundary = next_idx >= chars.len() || !chars[next_idx].is_alphanumeric();

            if prev_is_boundary && next_is_boundary {
                out.push_str(json);
                i = next_idx;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

fn match_python_literal(s: &str) -> Option<(&str, &str)> {
    if s.starts_with("True") {
        Some(("True", "true"))
    } else if s.starts_with("False") {
        Some(("False", "false"))
    } else if s.starts_with("None") {
        Some(("None", "null"))
    } else {
        None
    }
}

/// Replace Infinity, -Infinity, NaN with null (outside strings).
fn replace_special_numbers(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    // Pre-compute byte offsets to avoid O(n²) char_indices().nth(i) lookups
    let byte_offsets: Vec<usize> = input.char_indices().map(|(b, _)| b).collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
            } else if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let rest = &input[byte_offsets[i]..];

        // -Infinity
        if rest.starts_with("-Infinity") {
            let next_idx = i + 9;
            let next_is_boundary = next_idx >= chars.len() || !chars[next_idx].is_alphanumeric();
            if next_is_boundary {
                out.push_str("null");
                i = next_idx;
                continue;
            }
        }
        // Infinity
        if rest.starts_with("Infinity") {
            let next_idx = i + 8;
            let next_is_boundary = next_idx >= chars.len() || !chars[next_idx].is_alphanumeric();
            if next_is_boundary {
                out.push_str("null");
                i = next_idx;
                continue;
            }
        }
        // NaN
        if rest.starts_with("NaN") {
            let next_idx = i + 3;
            let next_is_boundary = next_idx >= chars.len() || !chars[next_idx].is_alphanumeric();
            if next_is_boundary {
                out.push_str("null");
                i = next_idx;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Quote unquoted keys: `{key: "value"}` → `{"key": "value"}`
/// Detects patterns like `{ identifier :` outside strings.
fn quote_unquoted_keys(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 32);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
            } else if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // After `{` or `,`, look for unquoted key pattern: identifier followed by `:`
        if chars[i] == '{' || chars[i] == ',' {
            out.push(chars[i]);
            i += 1;
            // Skip whitespace
            while i < chars.len() && chars[i].is_whitespace() {
                out.push(chars[i]);
                i += 1;
            }
            // Check if next token is an unquoted identifier (not `"`, `{`, `[`, etc.)
            if i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '_' || chars[i] == '$') {
                // Collect the identifier
                let key_start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
                {
                    i += 1;
                }
                let key: String = chars[key_start..i].iter().collect();
                // Skip whitespace after key
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                // If followed by `:`, this is an unquoted key
                if j < chars.len() && chars[j] == ':' {
                    out.push('"');
                    out.push_str(&key);
                    out.push('"');
                    // Push the whitespace we skipped
                    for k in i..j {
                        out.push(chars[k]);
                    }
                    i = j;
                    continue;
                } else {
                    // Not a key — push as-is
                    out.push_str(&key);
                    i = key_start + key.len();
                    continue;
                }
            }
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Convert hex numbers (0xFF) to decimal (255) outside strings.
fn convert_hex_numbers(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
            } else if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // Detect 0x or 0X prefix
        if chars[i] == '0' && i + 1 < chars.len() && (chars[i + 1] == 'x' || chars[i + 1] == 'X') {
            let hex_start = i + 2;
            let mut hex_end = hex_start;
            while hex_end < chars.len() && chars[hex_end].is_ascii_hexdigit() {
                hex_end += 1;
            }
            if hex_end > hex_start {
                let hex_str: String = chars[hex_start..hex_end].iter().collect();
                if let Ok(val) = u64::from_str_radix(&hex_str, 16) {
                    out.push_str(&val.to_string());
                    i = hex_end;
                    continue;
                }
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Strip `+` prefix from numbers: `+1` → `1`, `+3.14` → `3.14`
fn strip_plus_prefix(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        if in_string {
            if escape {
                escape = false;
                out.push(chars[i]);
                i += 1;
                continue;
            }
            if chars[i] == '\\' {
                escape = true;
            } else if chars[i] == '"' {
                in_string = false;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = true;
            out.push(chars[i]);
            i += 1;
            continue;
        }

        // `+` followed by digit, after `:` or `,` or `[`
        if chars[i] == '+' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            let prev = if i > 0 { Some(chars[i - 1]) } else { None };
            let prev_non_ws = chars[..i]
                .iter()
                .rev()
                .find(|c| !c.is_whitespace())
                .copied();
            if matches!(prev_non_ws, Some(':') | Some(',') | Some('[') | None)
                || matches!(prev, Some(c) if c.is_whitespace())
            {
                // Skip the `+`
                i += 1;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
}

// =============================================================================
// Repair Layer — state-machine-aware JSON repair
// =============================================================================
//
// NOTE: The quote-tracking in these functions shares the fundamental limitation
// that unescaped `"` inside string values (e.g. HTML attributes) will desync
// the `in_string` state. This is acceptable because:
// 1. Most LLM output has properly escaped quotes in JSON strings
// 2. The ultra structural recovery handles the truly broken cases
// 3. These repairs handle the common cases (control chars, trailing commas, etc.)

/// Core repair pipeline. `aggressive` enables more forceful strategies (finalize).
fn repair_json(input: &str, aggressive: bool, repairs: &mut Vec<String>) -> String {
    let mut s = input.to_string();

    // ═══ Phase 0: SANITIZE — strip framing noise ═══
    s = sanitize_input(&s, repairs);

    // ═══ Phase 1: NORMALIZE — non-standard syntax → JSON ═══
    s = convert_single_quotes(&s, repairs);
    s = normalize_syntax(&s, repairs);

    // ═══ Phase 2: REPAIR — fix broken JSON syntax ═══
    s = normalize_line_endings(&s, repairs);
    s = escape_control_chars_in_strings(&s, repairs);
    s = repair_unclosed_strings(&s, repairs);
    s = insert_missing_commas(&s, repairs);
    s = insert_missing_colons(&s, repairs);
    s = remove_trailing_commas(&s, repairs);

    // ═══ Phase 3: CLOSE — aggressive finalization ═══
    if aggressive {
        s = aggressive_close(&s, repairs);
    }

    // Always last: balance brackets (stack-ordered)
    s = balance_brackets_stateful(&s, repairs);

    s
}

// ─── Single quote conversion ────────────────────────────────────────────────

fn convert_single_quotes(input: &str, repairs: &mut Vec<String>) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut in_double = false;
    let mut in_single = false;
    let mut escape = false;
    let mut converted = false;

    for &ch in &chars {
        if escape {
            escape = false;
            out.push(ch);
            continue;
        }
        if ch == '\\' {
            escape = true;
            out.push(ch);
            continue;
        }

        if !in_double && !in_single {
            if ch == '\'' {
                out.push('"');
                in_single = true;
                converted = true;
                continue;
            }
            if ch == '"' {
                in_double = true;
                out.push(ch);
                continue;
            }
        } else if in_single {
            if ch == '\'' {
                out.push('"');
                in_single = false;
                continue;
            }
            if ch == '"' {
                // Double quote inside single-quoted string needs escaping
                out.push('\\');
                out.push('"');
                continue;
            }
        } else if in_double && ch == '"' {
            in_double = false;
        }

        out.push(ch);
    }

    if converted {
        repairs.push("converted single quotes to double quotes".into());
    }
    out
}
// ─── Control character escaping ──────────────────────────────────────────────

fn escape_control_chars_in_strings(input: &str, repairs: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut fixed = false;

    while i < chars.len() {
        let ch = chars[i];

        if in_string {
            // ── Handle escape sequences ──
            if ch == '\\' {
                if i + 1 < chars.len() {
                    let next = chars[i + 1];
                    match next {
                        // Valid JSON escapes — pass through
                        '"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' => {
                            out.push('\\');
                            out.push(next);
                            i += 2;
                            continue;
                        }
                        // Unicode escape — validate length and content (C7, C8)
                        'u' => {
                            if i + 5 < chars.len()
                                && chars[i + 2].is_ascii_hexdigit()
                                && chars[i + 3].is_ascii_hexdigit()
                                && chars[i + 4].is_ascii_hexdigit()
                                && chars[i + 5].is_ascii_hexdigit()
                            {
                                let hex: String = chars[i + 2..i + 6].iter().collect();
                                if let Ok(code) = u16::from_str_radix(&hex, 16) {
                                    // C8: Lone high surrogate (D800-DBFF)
                                    if (0xD800..=0xDBFF).contains(&code) {
                                        // Check if followed by low surrogate \uDCxx-\uDFxx
                                        let has_low = i + 11 < chars.len()
                                            && chars[i + 6] == '\\'
                                            && chars[i + 7] == 'u';
                                        if has_low {
                                            // Valid surrogate pair — pass through all 12 chars
                                            for j in 0..12 {
                                                out.push(chars[i + j]);
                                            }
                                            i += 12;
                                            continue;
                                        } else {
                                            // Lone surrogate → replacement char
                                            out.push_str("\\uFFFD");
                                            i += 6;
                                            fixed = true;
                                            continue;
                                        }
                                    }
                                    // C8: Lone low surrogate (DC00-DFFF)
                                    if (0xDC00..=0xDFFF).contains(&code) {
                                        out.push_str("\\uFFFD");
                                        i += 6;
                                        fixed = true;
                                        continue;
                                    }
                                }
                                // Valid \uXXXX — pass through
                                for j in 0..6 {
                                    out.push(chars[i + j]);
                                }
                                i += 6;
                                continue;
                            } else {
                                // C7: Truncated unicode escape — pad with zeros
                                out.push_str("\\u");
                                i += 2;
                                let mut hex_count = 0;
                                while i < chars.len()
                                    && hex_count < 4
                                    && chars[i].is_ascii_hexdigit()
                                {
                                    out.push(chars[i]);
                                    hex_count += 1;
                                    i += 1;
                                }
                                for _ in hex_count..4 {
                                    out.push('0');
                                }
                                fixed = true;
                                continue;
                            }
                        }
                        // C6: Invalid escape sequence — double the backslash
                        _ => {
                            out.push_str("\\\\");
                            // Don't consume `next` — it will be processed normally
                            i += 1;
                            fixed = true;
                            continue;
                        }
                    }
                } else {
                    // Trailing backslash at end of input — escape it
                    out.push_str("\\\\");
                    i += 1;
                    fixed = true;
                    continue;
                }
            }

            if ch == '"' {
                in_string = false;
                out.push(ch);
                i += 1;
                continue;
            }

            // ── Control characters (C1, C2, C3, C10) ──
            match ch {
                '\n' => {
                    out.push_str("\\n");
                    fixed = true;
                }
                '\r' => {
                    out.push_str("\\r");
                    fixed = true;
                }
                '\t' => {
                    out.push_str("\\t");
                    fixed = true;
                }
                '\x08' => {
                    out.push_str("\\b");
                    fixed = true;
                }
                '\x0C' => {
                    out.push_str("\\f");
                    fixed = true;
                }
                c if c.is_control() => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                    fixed = true;
                }
                _ => out.push(ch),
            }
            i += 1;
            continue;
        }

        // ── Outside string ──
        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
        i += 1;
    }

    if fixed {
        repairs.push("escaped control characters / fixed escape sequences in strings".into());
    }
    out
}

// ─── Unclosed string repair ─────────────────────────────────────────────────

fn repair_unclosed_strings(input: &str, repairs: &mut Vec<String>) -> String {
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
                continue;
            }
        } else if ch == '"' {
            in_string = true;
        }
    }

    if in_string {
        let mut out = input.to_string();
        // Count consecutive trailing backslashes. An odd count means the last
        // one is unescaped and would re-escape the closing quote we're about
        // to add. An even count means they're all escaped pairs — safe to keep.
        let trailing_backslashes = out.chars().rev().take_while(|&c| c == '\\').count();
        if trailing_backslashes % 2 == 1 {
            out.pop();
        }
        out.push('"');
        repairs.push("closed unclosed string".into());
        return out;
    }

    input.to_string()
}
// ─── Trailing comma removal ─────────────────────────────────────────────────

fn remove_trailing_commas(input: &str, repairs: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut in_string = false;
    let mut escape = false;
    let mut fixed = false;
    let mut i = 0;

    while i < chars.len() {
        let ch = chars[i];

        if in_string {
            if escape {
                escape = false;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '\\' {
                escape = true;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == ',' {
            let rest = &chars[i + 1..];
            let next_non_ws = rest.iter().find(|c| !c.is_whitespace());
            // B9: trailing comma before } or ]
            if next_non_ws == Some(&'}') || next_non_ws == Some(&']') {
                fixed = true;
                i += 1;
                continue;
            }
            // B10: consecutive commas — skip extra commas
            if next_non_ws == Some(&',') {
                fixed = true;
                i += 1;
                continue;
            }
        }

        out.push(ch);
        i += 1;
    }

    if fixed {
        repairs.push("removed trailing commas".into());
    }
    out
}

// ─── Missing comma insertion (B11) ──────────────────────────────────────────

/// Insert missing commas between fields: `"a":"1" "b":"2"` → `"a":"1", "b":"2"`
/// Detects pattern: `"value" "key"` (string end followed by string start without comma).
fn insert_missing_commas(input: &str, repairs: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(input.len() + 32);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut fixed = false;

    while i < chars.len() {
        let ch = chars[i];

        if in_string {
            if escape {
                escape = false;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '\\' {
                escape = true;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push(ch);
                i += 1;

                // After closing a string, check if next non-whitespace is `"`
                // without a comma, colon, }, ] in between
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '"' {
                    // Look back: was the string before this a value (preceded by `:`)?
                    // If so, the next `"` starts a new key — insert comma.
                    // Simple heuristic: if we see `"value" "key":`, insert comma.
                    // Check if after the upcoming `"..."` there's a `:`
                    let mut k = j + 1;
                    let mut k_escape = false;
                    while k < chars.len() {
                        if k_escape {
                            k_escape = false;
                            k += 1;
                            continue;
                        }
                        if chars[k] == '\\' {
                            k_escape = true;
                            k += 1;
                            continue;
                        }
                        if chars[k] == '"' {
                            k += 1;
                            break;
                        }
                        k += 1;
                    }
                    // Skip whitespace after the closing quote
                    while k < chars.len() && chars[k].is_whitespace() {
                        k += 1;
                    }
                    // If followed by `:`, this is a key — insert comma
                    if k < chars.len() && chars[k] == ':' {
                        out.push(',');
                        fixed = true;
                    }
                }
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
        i += 1;
    }

    if fixed {
        repairs.push("inserted missing commas between fields".into());
    }
    out
}

// ─── Missing colon insertion (B12) ──────────────────────────────────────────

/// Insert missing colons: `{"a" "value"}` → `{"a": "value"}`
/// Detects pattern: `"key" "value"` where the first string is a key (after `{` or `,`).
fn insert_missing_colons(input: &str, repairs: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut fixed = false;
    // Track whether we expect a key (after `{` or `,`)
    let mut expect_key = false;

    while i < chars.len() {
        let ch = chars[i];

        if in_string {
            if escape {
                escape = false;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '\\' {
                escape = true;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push(ch);
                i += 1;

                if expect_key {
                    // We just closed a key string. Check if next non-ws is `"` (missing colon)
                    let mut j = i;
                    while j < chars.len() && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if j < chars.len()
                        && (chars[j] == '"'
                            || chars[j] == '{'
                            || chars[j] == '['
                            || chars[j].is_ascii_digit()
                            || chars[j] == '-')
                    {
                        // Check it's not already followed by `:`
                        if j < chars.len() && chars[j] != ':' {
                            out.push(':');
                            fixed = true;
                        }
                    }
                    expect_key = false;
                }
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '{' | ',' => {
                expect_key = true;
                out.push(ch);
            }
            ':' | '}' | ']' => {
                expect_key = false;
                out.push(ch);
            }
            _ => {
                out.push(ch);
            }
        }
        i += 1;
    }

    if fixed {
        repairs.push("inserted missing colons".into());
    }
    out
}

// ─── Line ending normalization (E4) ─────────────────────────────────────────

/// Normalize `\r\n` → `\n` and lone `\r` → `\n` in string values.
fn normalize_line_endings(input: &str, repairs: &mut Vec<String>) -> String {
    if !input.contains('\r') {
        return input.to_string();
    }

    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut fixed = false;

    while i < chars.len() {
        let ch = chars[i];

        if in_string {
            if escape {
                escape = false;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '\\' {
                escape = true;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push(ch);
                i += 1;
                continue;
            }
            if ch == '\r' {
                // \r\n → \n, lone \r → \n
                if i + 1 < chars.len() && chars[i + 1] == '\n' {
                    i += 1; // skip \r, the \n will be processed next iteration
                } else {
                    out.push('\n');
                    i += 1;
                    fixed = true;
                }
                continue;
            }
            out.push(ch);
            i += 1;
            continue;
        }

        if ch == '"' {
            in_string = true;
        }
        out.push(ch);
        i += 1;
    }

    if fixed {
        repairs.push("normalized line endings in strings".into());
    }
    out
}

// ─── State-machine-aware bracket balancing ──────────────────────────────────

fn balance_brackets_stateful(input: &str, repairs: &mut Vec<String>) -> String {
    // Track the actual nesting order so we close in correct reverse order
    let mut stack: Vec<char> = Vec::new(); // '{' or '['
    let mut in_string = false;
    let mut escape = false;

    for ch in input.chars() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => stack.push('{'),
            '}' => {
                // Pop matching '{', or ignore stray '}'
                if let Some(pos) = stack.iter().rposition(|&c| c == '{') {
                    stack.remove(pos);
                }
            }
            '[' => stack.push('['),
            ']' => {
                if let Some(pos) = stack.iter().rposition(|&c| c == '[') {
                    stack.remove(pos);
                }
            }
            _ => {}
        }
    }

    if stack.is_empty() {
        return input.to_string();
    }

    let mut out = input.to_string();
    // Close in reverse nesting order
    for &opener in stack.iter().rev() {
        match opener {
            '{' => out.push('}'),
            '[' => out.push(']'),
            _ => {}
        }
    }

    repairs.push(format!(
        "balanced brackets (unclosed: {})",
        stack.iter().collect::<String>()
    ));
    out
}
// ─── Aggressive close ───────────────────────────────────────────────────────

fn aggressive_close(input: &str, repairs: &mut Vec<String>) -> String {
    let trimmed = input.trim_end();
    if trimmed.ends_with('}') || trimmed.ends_with(']') {
        return input.to_string();
    }

    let last_significant = trimmed.chars().rev().find(|c| !c.is_whitespace());

    match last_significant {
        // B2: truncated after colon — add null placeholder
        Some(':') => {
            repairs.push("added null for truncated value".into());
            format!("{}null", trimmed)
        }
        // B3: truncated after comma — remove dangling comma
        Some(',') => {
            repairs.push("removed dangling comma".into());
            trimmed.trim_end_matches(',').to_string()
        }
        // B14: truncated mid-escape — `"hello\` → remove trailing backslash
        Some('\\') => {
            let mut s = trimmed.to_string();
            s.pop(); // remove `\`
            repairs.push("removed truncated escape sequence".into());
            s
        }
        // B17: truncated mid-number — `{"a": 3.1` → number is valid, just needs closing
        // Also handles truncated after `"` (string just closed, needs comma or close)
        Some(c) if c.is_ascii_digit() || c == '.' => {
            // Number is fine as-is, bracket balancing will close it
            trimmed.to_string()
        }
        _ => trimmed.to_string(),
    }
}

// =============================================================================
// Tool Detection — scored schema matching
// =============================================================================

fn detect_tool(value: &Value, schemas: &[ToolSchema]) -> Option<String> {
    let obj = value.as_object()?;

    // Direct name field takes priority
    if let Some(name_val) = obj.get("name").or_else(|| obj.get("tool")) {
        if let Some(name_str) = name_val.as_str() {
            if let Some(schema) = schemas.iter().find(|s| s.name == name_str) {
                return Some(schema.name.clone());
            }
        }
    }

    let mut best: Option<&ToolSchema> = None;
    let mut best_score: i32 = 0;
    let mut ambiguous = false;

    for schema in schemas {
        let mut score: i32 = 0;

        for key in &schema.required_keys {
            if obj.contains_key(key) {
                score += 3;
            } else {
                score -= 1;
            }
        }

        for key in &schema.optional_keys {
            if obj.contains_key(key) {
                score += 1;
            }
        }

        if score > best_score {
            best_score = score;
            best = Some(schema);
            ambiguous = false;
        } else if score == best_score && score > 0 {
            ambiguous = true;
        }
    }

    // Ambiguous low-score matches are rejected
    if ambiguous && best_score < 3 {
        return None;
    }

    best.map(|s| s.name.clone())
}

// =============================================================================
// Public API — standalone repair for external callers
// =============================================================================

/// Apply the full repair pipeline to a raw JSON string without requiring
/// a streaming parser or tool schemas. Returns the repaired string and
/// a list of repair operations applied.
///
/// This is the standalone entry point for callers like `recover_tool_call_ultra`
/// that want to use the repair pipeline without the streaming parser.
pub fn repair_json_standalone(input: &str, aggressive: bool) -> (String, Vec<String>) {
    let mut repairs = Vec::new();
    let repaired = repair_json(input, aggressive, &mut repairs);
    (repaired, repairs)
}

/// Apply only Phase 0 (sanitize) — strip framing noise without modifying
/// the JSON structure itself. Use this when you need clean input for
/// structural recovery that searches for field boundaries.
pub fn sanitize_standalone(input: &str) -> (String, Vec<String>) {
    let mut repairs = Vec::new();
    let sanitized = sanitize_input(input, &mut repairs);
    (sanitized, repairs)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_schemas() -> Vec<ToolSchema> {
        vec![
            ToolSchema {
                name: "write_file".into(),
                required_keys: vec!["file_path".into(), "content".into()],
                optional_keys: vec!["description".into()],
            },
            ToolSchema {
                name: "run_command".into(),
                required_keys: vec!["command".into()],
                optional_keys: vec!["working_dir".into()],
            },
            ToolSchema {
                name: "search".into(),
                required_keys: vec!["query".into(), "path".into()],
                optional_keys: vec![],
            },
        ]
    }
    #[test]
    fn test_complete_json() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"{"file_path": "/a/b.rs", "content": "hello world"}"#);

        let result = parser.try_parse().unwrap();
        assert_eq!(result.tool_name, "write_file");
        assert_eq!(result.value["file_path"], "/a/b.rs");
    }

    #[test]
    fn test_streaming_chunks() {
        let mut parser = StreamingToolParser::new(test_schemas());

        parser.push(r#"{"file_pa"#);
        parser.push(r#"th": "/a/b.rs", "#);
        parser.push(r#""content": "hello "#);
        parser.push(r#"world"}"#);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "write_file");
        assert_eq!(result.value["content"], "hello world");
    }

    #[test]
    fn test_truncated_string() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"{"file_path": "/a/b.rs", "content": "hello wor"#);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "write_file");
        assert!(result.value["content"]
            .as_str()
            .unwrap()
            .starts_with("hello wor"));
    }

    #[test]
    fn test_truncated_after_colon() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"{"file_path": "/a/b.rs", "content":"#);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "write_file");
    }

    #[test]
    fn test_html_content_with_escaped_quotes() {
        // This tests properly escaped HTML quotes — the normal case that works fine
        let mut parser = StreamingToolParser::new(test_schemas());
        let input = r#"{"file_path": "/index.html", "content": "<html lang=\"zh-CN\">\n<head>\n<title>Test</title>\n</head>\n</html>"}"#;
        parser.push(input);

        let result = parser.try_parse().unwrap();
        assert_eq!(result.tool_name, "write_file");
    }

    #[test]
    fn test_unescaped_newlines() {
        let mut parser = StreamingToolParser::new(test_schemas());
        let input = "{\"file_path\": \"/a.txt\", \"content\": \"line1\nline2\nline3\"}";
        parser.push(input);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "write_file");
    }
    #[test]
    fn test_trailing_comma() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"{"command": "ls -la", "working_dir": "/tmp",}"#);

        let result = parser.try_parse().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    #[test]
    fn test_multiple_objects() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(
            r#"{"command": "mkdir /tmp/test"} {"file_path": "/tmp/test/a.txt", "content": "hi"}"#,
        );

        assert_eq!(parser.object_count(), 2);

        let results = parser.finalize_all();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap().tool_name, "run_command");
        assert_eq!(results[1].as_ref().unwrap().tool_name, "write_file");
    }

    #[test]
    fn test_stray_closing_brace() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"some text } more text {"command": "echo hi"}"#);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    #[test]
    fn test_utf8_content() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(
            r#"这是一些前缀文本 {"file_path": "/中文路径/测试.txt", "content": "你好世界🌍"}"#,
        );

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "write_file");
        assert_eq!(result.value["content"], "你好世界🌍");
    }

    #[test]
    fn test_array_value_truncated() {
        let schemas = vec![ToolSchema {
            name: "multi_edit".into(),
            required_keys: vec!["edits".into()],
            optional_keys: vec![],
        }];
        let mut parser = StreamingToolParser::new(schemas);
        parser.push(r#"{"edits": [{"file": "a.txt", "content": "hello"}, {"file": "b.txt""#);

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "multi_edit");
    }

    #[test]
    fn test_single_quotes() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push("{'command': 'echo hello'}");

        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    #[test]
    fn test_reset() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(r#"{"command": "ls"}"#);
        assert_eq!(parser.object_count(), 1);

        parser.reset();
        assert_eq!(parser.object_count(), 0);
        assert_eq!(parser.buffer(), "");
    }

    #[test]
    fn test_repair_diagnostics() {
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push("{\"command\": \"echo hello\nworld\",}");

        let result = parser.finalize().unwrap();
        assert!(!result.repairs.is_empty());
    }

    // =========================================================================
    // Taxonomy Tests — one per malformation ID
    // =========================================================================
    //
    // Each test uses repair_json_standalone to verify the repair in isolation,
    // then (where applicable) runs through StreamingToolParser for integration.

    // ── Category A: Framing ──────────────────────────────────────────────────

    #[test]
    fn test_a5_bom_prefix() {
        let input = "\u{feff}{\"command\": \"ls\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("BOM")));
    }

    #[test]
    fn test_a6_ansi_escape_sequences() {
        let input = "\x1b[32m{\"command\": \"ls\"}\x1b[0m";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("ANSI")));
    }

    #[test]
    fn test_a6_ansi_osc_sequence() {
        // OSC sequence: ESC ] ... BEL
        let input = "\x1b]0;title\x07{\"command\": \"pwd\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "pwd");
        assert!(repairs.iter().any(|r| r.contains("ANSI")));
    }

    #[test]
    fn test_a7_xml_tool_call_wrapper() {
        let input = "<tool_call>{\"command\": \"echo hi\"}</tool_call>";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "echo hi");
        assert!(repairs.iter().any(|r| r.contains("tool_call")));
    }

    #[test]
    fn test_a7_xml_function_call_wrapper() {
        let input = "<function_call>{\"command\": \"date\"}</function_call>";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "date");
        assert!(repairs.iter().any(|r| r.contains("function_call")));
    }

    #[test]
    fn test_a7_xml_json_wrapper_with_attrs() {
        let input = "<json type=\"tool\">{\"command\": \"whoami\"}</json>";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "whoami");
        assert!(repairs.iter().any(|r| r.contains("<json>")));
    }

    #[test]
    fn test_a1_markdown_code_fence() {
        let input = "```json\n{\"command\": \"ls -la\"}\n```";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls -la");
        assert!(repairs.iter().any(|r| r.contains("markdown")));
    }

    #[test]
    fn test_a1_markdown_fence_no_closing() {
        let input = "```json\n{\"command\": \"ls\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("markdown")));
    }

    #[test]
    fn test_d7_trailing_semicolons() {
        let input = "{\"command\": \"echo ok\"};";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "echo ok");
        assert!(repairs.iter().any(|r| r.contains("semicolon")));
    }

    // ── Category D: Syntax Sugar ─────────────────────────────────────────────

    #[test]
    fn test_d2_unquoted_keys() {
        let input = "{command: \"ls\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("unquoted")));
    }

    #[test]
    fn test_d2_unquoted_keys_multiple() {
        let input = "{file_path: \"/a.txt\", content: \"hello\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["file_path"], "/a.txt");
        assert_eq!(v["content"], "hello");
        assert!(repairs.iter().any(|r| r.contains("unquoted")));
    }

    #[test]
    fn test_d3_block_comments() {
        let input = "{\"command\": \"ls\" /* list files */}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("comment")));
    }

    #[test]
    fn test_d4_line_comments() {
        let input = "{\"command\": \"ls\" // list files\n}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("comment")));
    }

    #[test]
    fn test_d5_hex_numbers() {
        let input = "{\"a\": 0xFF}";
        let (repaired, _) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["a"], 255);
    }

    #[test]
    fn test_d6_infinity() {
        let input = "{\"a\": Infinity}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert!(v["a"].is_null());
        assert!(repairs
            .iter()
            .any(|r| r.contains("Infinity") || r.contains("NaN")));
    }

    #[test]
    fn test_d6_negative_infinity() {
        let input = "{\"a\": -Infinity}";
        let (repaired, _) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert!(v["a"].is_null());
    }

    #[test]
    fn test_d6_nan() {
        let input = "{\"a\": NaN}";
        let (repaired, _) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert!(v["a"].is_null());
    }

    #[test]
    fn test_d8_python_true_false_none() {
        let input = "{\"a\": True, \"b\": False, \"c\": None}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["a"], true);
        assert_eq!(v["b"], false);
        assert!(v["c"].is_null());
        assert!(repairs.iter().any(|r| r.contains("Python")));
    }

    #[test]
    fn test_d8_python_literals_not_in_strings() {
        // "True" inside a string value should NOT be converted
        let input = "{\"command\": \"echo True\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "echo True");
    }

    #[test]
    fn test_d10_plus_prefix() {
        let input = "{\"a\": +42}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["a"], 42);
        assert!(repairs.iter().any(|r| r.contains("plus")));
    }

    // ── Category B: Structural ───────────────────────────────────────────────

    #[test]
    fn test_b10_consecutive_commas() {
        let input = "{\"command\": \"ls\",,,}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs
            .iter()
            .any(|r| r.contains("trailing comma") || r.contains("commas")));
    }

    #[test]
    fn test_b11_missing_comma_between_fields() {
        let input = "{\"file_path\": \"/a.txt\" \"content\": \"hello\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["file_path"], "/a.txt");
        assert_eq!(v["content"], "hello");
        assert!(repairs.iter().any(|r| r.contains("missing comma")));
    }

    #[test]
    fn test_b12_missing_colon() {
        let input = "{\"command\" \"ls\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("missing colon")));
    }

    #[test]
    fn test_b14_truncated_mid_escape() {
        let input = "{\"command\": \"hello\\";
        let (repaired, _) = repair_json_standalone(input, true);
        // Should be parseable after aggressive close + bracket balance
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
    }

    #[test]
    fn test_b17_truncated_mid_number() {
        let input = "{\"a\": 3.14";
        let (repaired, _) = repair_json_standalone(input, true);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["a"], 3.14);
    }

    // ── Category C: String Content ───────────────────────────────────────────

    #[test]
    fn test_c5_unescaped_backslash() {
        // \q is not a valid JSON escape — should become \\q
        let input = r#"{"command": "echo \q"}"#;
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert!(v["command"].as_str().unwrap().contains("\\q"));
        assert!(repairs.iter().any(|r| r.contains("escape")));
    }

    #[test]
    fn test_c6_invalid_escape_windows_path() {
        // Windows path: C:\Users\name → should escape the backslashes
        let input = "{\"file_path\": \"C:\\Users\\name\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
    }

    #[test]
    fn test_c7_truncated_unicode_escape() {
        // \u00 is only 2 hex digits — should be padded to \u0000
        let input = "{\"a\": \"text\\u00\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
        assert!(repairs.iter().any(|r| r.contains("escape")));
    }

    #[test]
    fn test_c7_truncated_unicode_escape_one_digit() {
        let input = "{\"a\": \"text\\uA\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
    }

    #[test]
    fn test_c8_lone_high_surrogate() {
        // \uD83D without a following low surrogate → \uFFFD
        let input = "{\"a\": \"emoji \\uD83D end\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
        assert!(repaired.contains("uFFFD"));
        assert!(repairs.iter().any(|r| r.contains("escape")));
    }

    #[test]
    fn test_c8_lone_low_surrogate() {
        // \uDC00 (low surrogate without preceding high) → \uFFFD
        let input = "{\"a\": \"bad \\uDC00 end\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
        assert!(repaired.contains("uFFFD"));
    }

    #[test]
    fn test_c8_valid_surrogate_pair_preserved() {
        // Valid surrogate pair should pass through unchanged
        let input = "{\"a\": \"emoji \\uD83D\\uDE00 end\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
        assert!(repaired.contains("\\uD83D\\uDE00"));
    }

    // ── Category E: Encoding ─────────────────────────────────────────────────

    #[test]
    fn test_e4_crlf_in_strings() {
        let input = "{\"command\": \"line1\r\nline2\"}";
        let (repaired, _) = repair_json_standalone(input, false);
        // After repair, \r should be gone or normalized
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
    }

    #[test]
    fn test_e4_lone_cr_in_strings() {
        let input = "{\"command\": \"line1\rline2\"}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let result = serde_json::from_str::<Value>(&repaired);
        assert!(result.is_ok(), "Failed to parse: {}", repaired);
        assert!(repairs.iter().any(|r| r.contains("line ending")));
    }

    // ── Integration: Combined malformations ──────────────────────────────────

    #[test]
    fn test_combined_bom_plus_trailing_comma() {
        // BOM is before the `{`, so the scanner extracts the object without BOM.
        // The standalone API handles BOM; the streaming parser only sees the trailing comma.
        let input = "\u{feff}{\"command\": \"ls\",}";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.len() >= 2); // BOM + trailing comma
    }

    #[test]
    fn test_combined_markdown_fence_plus_single_quotes() {
        let input = "```json\n{'command': 'echo hello'}\n```";
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(input);
        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    #[test]
    fn test_combined_xml_wrapper_plus_python_literals() {
        let input = "<tool_call>{\"a\": True, \"command\": \"test\"}</tool_call>";
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(input);
        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    #[test]
    fn test_combined_ansi_plus_unquoted_keys() {
        // ANSI escape `[` confuses the scanner's bracket depth, so this goes
        // through the standalone API (the correct path for pre-framed input).
        let input = "\x1b[1m{command: \"ls -la\"}\x1b[0m";
        let (repaired, repairs) = repair_json_standalone(input, false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls -la");
        assert!(repairs.iter().any(|r| r.contains("ANSI")));
        assert!(repairs.iter().any(|r| r.contains("unquoted")));
    }

    #[test]
    fn test_combined_comments_plus_trailing_comma() {
        let input = "{\"command\": \"ls\" /* list */, }";
        let mut parser = StreamingToolParser::new(test_schemas());
        parser.push(input);
        let result = parser.finalize().unwrap();
        assert_eq!(result.tool_name, "run_command");
    }

    // ── Standalone API tests ─────────────────────────────────────────────────

    #[test]
    fn test_standalone_api_non_aggressive() {
        let (repaired, repairs) = repair_json_standalone("```\n{\"command\": \"ls\",}\n```", false);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(!repairs.is_empty());
    }

    #[test]
    fn test_standalone_api_aggressive() {
        let (repaired, repairs) =
            repair_json_standalone("{\"command\": \"ls\", \"working_dir\":", true);
        let v: Value = serde_json::from_str(&repaired).unwrap();
        assert_eq!(v["command"], "ls");
        assert!(repairs.iter().any(|r| r.contains("null")));
    }
}
