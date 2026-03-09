//! Enhanced REPL prompt with line editing and history.
//!
//! Uses crossterm raw mode for character-by-character input:
//! - Left/Right arrow keys for cursor movement
//! - Up/Down arrow keys for cursor movement across wrapped rows
//! - Ctrl+P / Ctrl+N for history navigation
//! - Home/End for start/end of the current visual row
//! - Backspace and Delete
//! - Ctrl+C to cancel current line
//! - Ctrl+D on empty line to exit
//! - Enter to submit
//! - Shift+Enter to insert newline
//! - Ctrl+U to clear line
//! - Ctrl+W to delete word backward

use crate::cli_style::CliStyle;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

/// Result of reading a prompt line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptResult {
    /// User submitted a line of text.
    Line(String),
    /// User pressed Ctrl+D on an empty line (exit signal).
    Eof,
    /// User pressed Ctrl+C (cancel current input, not exit).
    Interrupt,
}

/// Prompt history buffer.
#[derive(Debug, Clone)]
pub struct PromptHistory {
    entries: Vec<String>,
    max_size: usize,
}

impl PromptHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
        }
    }

    /// Add a new entry to the history (most recent at the end).
    pub fn push(&mut self, line: &str) {
        let line = line.trim().to_string();
        if line.is_empty() {
            return;
        }
        self.entries.retain(|entry| entry != &line);
        self.entries.push(line);
        if self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(|entry| entry.as_str())
    }
}

/// Visual prompt frame for interactive CLI input.
#[derive(Debug, Clone)]
pub struct PromptFrame {
    plain_prompt: String,
    header_line: String,
    footer_line: String,
    input_prefix_width: u16,
    inner_width: usize,
    max_visible_rows: usize,
    color: bool,
}

#[derive(Debug, Clone)]
struct PromptRenderState {
    cursor_row_in_view: usize,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
struct WrappedRow {
    start: usize,
    end: usize,
    text: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct WrappedViewport {
    visible_rows: Vec<String>,
    total_rows: usize,
    visible_start_row: usize,
    cursor_row: usize,
    cursor_col: usize,
}

impl PromptFrame {
    pub fn boxed(mode_label: &str, model_label: &str, style: &CliStyle) -> Self {
        let header_text = truncate_visible(
            &format!(
                " {}{}{} ",
                mode_label.trim(),
                bullet_separator(style),
                model_label.trim()
            ),
            160,
        );
        let footer_text = truncate_visible(
            " Ready  •  Alt+Enter/Ctrl+J newline  •  /help  •  Ctrl+D exit ",
            160,
        );
        let inner_width = usize::from(style.width.saturating_sub(5)).max(20);
        let chrome_width = inner_width + 2;
        let max_visible_rows = prompt_max_visible_rows();

        let header_content = pad_right(
            &truncate_visible(&header_text, chrome_width),
            chrome_width,
            '─',
        );
        let footer_content = pad_right(
            &truncate_visible(&footer_text, chrome_width),
            chrome_width,
            '─',
        );

        let header_line = if style.color {
            format!(
                "{}{}{}",
                style.cyan("╭"),
                style.bold_cyan(&header_content),
                style.cyan("╮")
            )
        } else {
            format!("╭{}╮", header_content)
        };

        let footer_line = if style.color {
            format!(
                "{}{}{}",
                style.cyan("╰"),
                style.dim(&footer_content),
                style.cyan("╯")
            )
        } else {
            format!("╰{}╯", footer_content)
        };

        Self {
            plain_prompt: "> ".to_string(),
            header_line,
            footer_line,
            input_prefix_width: 2,
            inner_width,
            max_visible_rows,
            color: style.color,
        }
    }

    pub fn content_width(&self) -> usize {
        self.inner_width
    }
}

/// Read a single line from the terminal with editing and history support.
///
/// If the terminal is not a TTY, falls back to plain `stdin.read_line()`.
pub fn read_prompt_line(
    frame: &PromptFrame,
    history: &PromptHistory,
    style: &CliStyle,
) -> io::Result<PromptResult> {
    if !style.color {
        return read_plain_line(&frame.plain_prompt);
    }

    read_raw_line(frame, history)
}

fn read_plain_line(prompt_str: &str) -> io::Result<PromptResult> {
    print!("{}", prompt_str);
    io::stdout().flush()?;

    let mut input = String::new();
    let bytes = io::stdin().read_line(&mut input)?;
    if bytes == 0 {
        return Ok(PromptResult::Eof);
    }
    Ok(PromptResult::Line(
        input
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .to_string(),
    ))
}

fn read_raw_line(frame: &PromptFrame, history: &PromptHistory) -> io::Result<PromptResult> {
    let mut line = String::new();
    let mut cursor_pos = 0usize;
    let mut preferred_column: Option<usize> = None;
    let mut history_index: Option<usize> = None;
    let mut saved_input = String::new();
    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    let mut render_state = render_prompt_frame(&mut stdout, frame, &line, cursor_pos, None)?;

    let result = loop {
        let ev = event::read()?;
        match ev {
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::SHIFT)
                || modifiers.contains(KeyModifiers::ALT) =>
            {
                insert_char_at_cursor(&mut line, cursor_pos, '\n');
                cursor_pos += 1;
                preferred_column = None;
                history_index = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('j'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                insert_char_at_cursor(&mut line, cursor_pos, '\n');
                cursor_pos += 1;
                preferred_column = None;
                history_index = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                dismiss_prompt(&mut stdout, &render_state)?;
                break PromptResult::Line(line);
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                dismiss_prompt(&mut stdout, &render_state)?;
                break PromptResult::Interrupt;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('d'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                if line.is_empty() {
                    dismiss_prompt(&mut stdout, &render_state)?;
                    break PromptResult::Eof;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('u'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                line.clear();
                cursor_pos = 0;
                preferred_column = None;
                history_index = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('w'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                if cursor_pos > 0 {
                    let chars: Vec<char> = line.chars().collect();
                    let mut new_pos = cursor_pos;
                    while new_pos > 0 && chars[new_pos - 1].is_whitespace() {
                        new_pos -= 1;
                    }
                    while new_pos > 0 && !chars[new_pos - 1].is_whitespace() {
                        new_pos -= 1;
                    }
                    replace_char_range(&mut line, new_pos, cursor_pos, "");
                    cursor_pos = new_pos;
                    preferred_column = None;
                    history_index = None;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('p'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                browse_history_prev(
                    history,
                    &mut history_index,
                    &mut saved_input,
                    &mut line,
                    &mut cursor_pos,
                );
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('n'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                browse_history_next(
                    history,
                    &mut history_index,
                    &saved_input,
                    &mut line,
                    &mut cursor_pos,
                );
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                if cursor_pos > 0 {
                    replace_char_range(&mut line, cursor_pos - 1, cursor_pos, "");
                    cursor_pos -= 1;
                    preferred_column = None;
                    history_index = None;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Delete,
                ..
            }) => {
                if cursor_pos < line.chars().count() {
                    replace_char_range(&mut line, cursor_pos, cursor_pos + 1, "");
                    preferred_column = None;
                    history_index = None;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Left,
                ..
            }) => {
                if cursor_pos > 0 {
                    cursor_pos -= 1;
                }
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Right,
                ..
            }) => {
                if cursor_pos < line.chars().count() {
                    cursor_pos += 1;
                }
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Home,
                ..
            }) => {
                cursor_pos = move_cursor_home(&line, cursor_pos, frame.inner_width);
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::End, ..
            }) => {
                cursor_pos = move_cursor_end(&line, cursor_pos, frame.inner_width);
                preferred_column = None;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                cursor_pos = move_cursor_vertically(
                    &line,
                    cursor_pos,
                    frame.inner_width,
                    -1,
                    &mut preferred_column,
                );
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                cursor_pos = move_cursor_vertically(
                    &line,
                    cursor_pos,
                    frame.inner_width,
                    1,
                    &mut preferred_column,
                );
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                insert_char_at_cursor(&mut line, cursor_pos, ch);
                cursor_pos += 1;
                preferred_column = None;
                history_index = None;
            }
            _ => {}
        }

        render_state =
            render_prompt_frame(&mut stdout, frame, &line, cursor_pos, Some(&render_state))?;
    };

    terminal::disable_raw_mode()?;
    Ok(result)
}

fn render_prompt_frame(
    stdout: &mut io::Stdout,
    frame: &PromptFrame,
    line: &str,
    cursor_pos: usize,
    previous_state: Option<&PromptRenderState>,
) -> io::Result<PromptRenderState> {
    if let Some(state) = previous_state {
        execute!(
            stdout,
            cursor::MoveToColumn(0),
            cursor::MoveUp((state.cursor_row_in_view + 1) as u16),
            terminal::Clear(ClearType::FromCursorDown)
        )?;
    }

    let viewport = wrapped_viewport(line, cursor_pos, frame.inner_width, frame.max_visible_rows);
    write!(
        stdout,
        "\r{}{}",
        frame.header_line,
        terminal::Clear(ClearType::UntilNewLine),
    )?;
    for row in &viewport.visible_rows {
        write!(
            stdout,
            "\r\n\r{}{}",
            compose_input_row(frame, row),
            terminal::Clear(ClearType::UntilNewLine),
        )?;
    }
    write!(
        stdout,
        "\r\n\r{}{}",
        frame.footer_line,
        terminal::Clear(ClearType::UntilNewLine),
    )?;
    execute!(
        stdout,
        cursor::MoveUp((viewport.visible_rows.len() - viewport.cursor_row) as u16),
        cursor::MoveToColumn(frame.input_prefix_width + viewport.cursor_col as u16)
    )?;
    stdout.flush()?;

    Ok(PromptRenderState {
        cursor_row_in_view: viewport.cursor_row,
    })
}

fn dismiss_prompt(stdout: &mut io::Stdout, state: &PromptRenderState) -> io::Result<()> {
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        cursor::MoveUp((state.cursor_row_in_view + 1) as u16),
        terminal::Clear(ClearType::FromCursorDown),
        cursor::MoveToColumn(0)
    )?;
    stdout.flush()?;
    Ok(())
}

fn compose_input_row(frame: &PromptFrame, visible_line: &str) -> String {
    let content = pad_right(visible_line, frame.inner_width, ' ');
    if frame.color {
        format!("{} {} {}", "\x1b[36m│\x1b[0m", content, "\x1b[36m│\x1b[0m")
    } else {
        format!("│ {} │", content)
    }
}

fn wrapped_rows(text: &str, width: usize) -> Vec<WrappedRow> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let row_width = width.max(1);
    let mut rows = Vec::new();
    let mut row_start = 0usize;
    let mut row_len = 0usize;
    let mut index = 0usize;

    while index < len {
        if row_len == row_width {
            rows.push(make_wrapped_row(&chars, row_start, index));
            row_start = index;
            row_len = 0;
            continue;
        }

        if chars[index] == '\n' {
            rows.push(make_wrapped_row(&chars, row_start, index));
            row_start = index + 1;
            row_len = 0;
            index += 1;
            continue;
        }

        row_len += 1;
        index += 1;
    }

    rows.push(make_wrapped_row(&chars, row_start, len));
    if rows.is_empty() {
        rows.push(make_wrapped_row(&chars, 0, 0));
    }
    rows
}

fn make_wrapped_row(chars: &[char], start: usize, end: usize) -> WrappedRow {
    WrappedRow {
        start,
        end,
        text: chars[start.min(chars.len())..end.min(chars.len())]
            .iter()
            .collect(),
    }
}

fn current_wrapped_position(
    text: &str,
    cursor_pos: usize,
    width: usize,
) -> (Vec<WrappedRow>, usize, usize) {
    let rows = wrapped_rows(text, width);
    let clamped_cursor = cursor_pos.min(text.chars().count());
    let row_index = rows
        .iter()
        .rposition(|row| row.start <= clamped_cursor)
        .unwrap_or(0);
    let row = &rows[row_index];
    let row_len = row.end.saturating_sub(row.start);
    let col = clamped_cursor.saturating_sub(row.start).min(row_len);
    (rows, row_index, col)
}

fn wrapped_viewport(
    text: &str,
    cursor_pos: usize,
    width: usize,
    max_visible_rows: usize,
) -> WrappedViewport {
    let (rows, row_index, col) = current_wrapped_position(text, cursor_pos, width);
    let visible_rows_count = rows.len().min(max_visible_rows.max(1));
    let visible_start_row = row_index
        .saturating_add(1)
        .saturating_sub(visible_rows_count);
    let visible_end_row = visible_start_row + visible_rows_count;
    let visible_rows = rows[visible_start_row..visible_end_row]
        .iter()
        .map(|row| row.text.clone())
        .collect::<Vec<_>>();

    WrappedViewport {
        visible_rows,
        total_rows: rows.len(),
        visible_start_row,
        cursor_row: row_index.saturating_sub(visible_start_row),
        cursor_col: col,
    }
}

fn move_cursor_vertically(
    text: &str,
    cursor_pos: usize,
    width: usize,
    delta_rows: isize,
    preferred_column: &mut Option<usize>,
) -> usize {
    let (rows, row_index, current_col) = current_wrapped_position(text, cursor_pos, width);
    let preferred = preferred_column.get_or_insert(current_col);
    let target_row = if delta_rows < 0 {
        row_index.saturating_sub(delta_rows.unsigned_abs())
    } else {
        row_index
            .saturating_add(delta_rows as usize)
            .min(rows.len().saturating_sub(1))
    };
    let row = &rows[target_row];
    row.start + (*preferred).min(row.end.saturating_sub(row.start))
}

fn move_cursor_home(text: &str, cursor_pos: usize, width: usize) -> usize {
    let (rows, row_index, _) = current_wrapped_position(text, cursor_pos, width);
    rows[row_index].start
}

fn move_cursor_end(text: &str, cursor_pos: usize, width: usize) -> usize {
    let (rows, row_index, _) = current_wrapped_position(text, cursor_pos, width);
    rows[row_index].end
}

fn browse_history_prev(
    history: &PromptHistory,
    history_index: &mut Option<usize>,
    saved_input: &mut String,
    line: &mut String,
    cursor_pos: &mut usize,
) {
    if history.is_empty() {
        return;
    }
    match *history_index {
        None => {
            *saved_input = line.clone();
            let idx = history.len() - 1;
            *history_index = Some(idx);
            *line = history.get(idx).unwrap_or_default().to_string();
        }
        Some(idx) if idx > 0 => {
            let new_idx = idx - 1;
            *history_index = Some(new_idx);
            *line = history.get(new_idx).unwrap_or_default().to_string();
        }
        _ => {}
    }
    *cursor_pos = line.chars().count();
}

fn browse_history_next(
    history: &PromptHistory,
    history_index: &mut Option<usize>,
    saved_input: &str,
    line: &mut String,
    cursor_pos: &mut usize,
) {
    if let Some(idx) = *history_index {
        if idx + 1 < history.len() {
            let new_idx = idx + 1;
            *history_index = Some(new_idx);
            *line = history.get(new_idx).unwrap_or_default().to_string();
        } else {
            *history_index = None;
            *line = saved_input.to_string();
        }
        *cursor_pos = line.chars().count();
    }
}

fn insert_char_at_cursor(text: &mut String, cursor_pos: usize, ch: char) {
    let byte_pos = char_index_to_byte_offset(text, cursor_pos);
    text.insert(byte_pos, ch);
}

fn replace_char_range(text: &mut String, start: usize, end: usize, replacement: &str) {
    let byte_start = char_index_to_byte_offset(text, start);
    let byte_end = char_index_to_byte_offset(text, end);
    text.replace_range(byte_start..byte_end, replacement);
}

fn char_index_to_byte_offset(text: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(char_index)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

fn prompt_max_visible_rows() -> usize {
    match terminal::size() {
        Ok((_, rows)) => usize::from(rows.saturating_sub(10)).clamp(3, 12),
        Err(_) => 6,
    }
}

fn truncate_visible(text: &str, max_width: usize) -> String {
    if text.chars().count() <= max_width {
        return text.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut truncated: String = text.chars().take(max_width - 1).collect();
    truncated.push('…');
    truncated
}

fn pad_right(text: &str, width: usize, fill: char) -> String {
    let current = text.chars().count();
    if current >= width {
        return text.to_string();
    }
    format!("{}{}", text, fill.to_string().repeat(width - current))
}

fn bullet_separator(style: &CliStyle) -> String {
    if style.color {
        "  •  ".to_string()
    } else {
        " | ".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_push_deduplicates() {
        let mut history = PromptHistory::new(100);
        history.push("hello");
        history.push("world");
        history.push("hello");

        assert_eq!(history.len(), 2);
        assert_eq!(history.get(0), Some("world"));
        assert_eq!(history.get(1), Some("hello"));
    }

    #[test]
    fn history_push_trims_whitespace() {
        let mut history = PromptHistory::new(100);
        history.push("  hello  ");
        assert_eq!(history.get(0), Some("hello"));
    }

    #[test]
    fn history_push_ignores_empty() {
        let mut history = PromptHistory::new(100);
        history.push("");
        history.push("   ");
        assert!(history.is_empty());
    }

    #[test]
    fn history_respects_max_size() {
        let mut history = PromptHistory::new(3);
        history.push("a");
        history.push("b");
        history.push("c");
        history.push("d");

        assert_eq!(history.len(), 3);
        assert_eq!(history.get(0), Some("b"));
        assert_eq!(history.get(1), Some("c"));
        assert_eq!(history.get(2), Some("d"));
    }

    #[test]
    fn prompt_result_variants() {
        let line = PromptResult::Line("test".to_string());
        let eof = PromptResult::Eof;
        let interrupt = PromptResult::Interrupt;

        assert_eq!(line, PromptResult::Line("test".to_string()));
        assert_eq!(eof, PromptResult::Eof);
        assert_eq!(interrupt, PromptResult::Interrupt);
    }

    #[test]
    fn boxed_prompt_frame_uses_full_terminal_width_budget() {
        let style = CliStyle::plain();
        let frame = PromptFrame::boxed("Preset prometheus", "Model auto", &style);
        assert_eq!(frame.content_width(), 75);
        assert!(frame.header_line.contains("Preset prometheus"));
        assert!(frame.footer_line.contains("Alt+Enter/Ctrl+J newline"));
    }

    #[test]
    fn wrapped_viewport_soft_wraps_and_keeps_cursor_visible() {
        let viewport = wrapped_viewport("abcdefghijklmnopqrstuvwxyz", 25, 10, 4);
        assert_eq!(
            viewport.visible_rows,
            vec!["abcdefghij", "klmnopqrst", "uvwxyz"]
        );
        assert_eq!(viewport.cursor_row, 2);
        assert_eq!(viewport.cursor_col, 5);
        assert_eq!(viewport.total_rows, 3);
    }

    #[test]
    fn wrapped_viewport_respects_explicit_newlines() {
        let viewport = wrapped_viewport("abc\ndef\n\nghi", 8, 10, 6);
        assert_eq!(viewport.visible_rows, vec!["abc", "def", "", "ghi"]);
        assert_eq!(viewport.cursor_row, 2);
        assert_eq!(viewport.cursor_col, 0);
    }

    #[test]
    fn move_cursor_vertically_preserves_preferred_column() {
        let mut preferred = None;
        let text = "abc\ndefgh\nxy";
        let pos = move_cursor_vertically(text, 7, 10, -1, &mut preferred);
        assert_eq!(pos, 3);
        let pos = move_cursor_vertically(text, pos, 10, 1, &mut preferred);
        assert_eq!(pos, 7);
    }

    #[test]
    fn move_cursor_home_and_end_use_visual_rows() {
        let text = "abcdefghijXYZ";
        assert_eq!(move_cursor_home(text, 12, 5), 10);
        assert_eq!(move_cursor_end(text, 12, 5), 13);
    }
}
