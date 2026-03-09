//! Terminal markdown renderer for CLI output.
//!
//! Converts markdown text to ANSI-styled terminal output.
//! Designed for both full-text rendering and streaming (delta) mode.

use crate::cli_style::CliStyle;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Render a complete markdown string to ANSI-styled terminal text.
pub fn render_markdown(text: &str, style: &CliStyle) -> String {
    if !style.color {
        return text.to_string();
    }
    let mut renderer = MarkdownRenderer::new(style);
    renderer.push(text);
    renderer.finish()
}

/// Streaming markdown renderer that accumulates delta text and renders
/// complete markdown blocks. Incomplete trailing content is buffered
/// until more text arrives or `finish()` is called.
pub struct MarkdownStreamer<'a> {
    buffer: String,
    rendered_up_to: usize,
    style: &'a CliStyle,
}

impl<'a> MarkdownStreamer<'a> {
    pub fn new(style: &'a CliStyle) -> Self {
        Self {
            buffer: String::new(),
            rendered_up_to: 0,
            style,
        }
    }

    /// Push a new delta chunk. Returns any newly renderable output.
    pub fn push(&mut self, delta: &str) -> String {
        if !self.style.color {
            return delta.to_string();
        }
        self.buffer.push_str(delta);
        self.try_render_complete()
    }

    /// Flush remaining buffer and render everything.
    pub fn finish(&mut self) -> String {
        if !self.style.color {
            return String::new();
        }
        let remaining = self.buffer[self.rendered_up_to..].to_string();
        self.rendered_up_to = self.buffer.len();
        if remaining.is_empty() {
            return String::new();
        }
        let mut renderer = MarkdownRenderer::new(self.style);
        renderer.push(&remaining);
        renderer.finish()
    }

    /// Try to render complete lines from the buffer.
    /// We render line-by-line to handle streaming safely — complete lines
    /// are rendered with markdown, the trailing incomplete line is held.
    fn try_render_complete(&mut self) -> String {
        let pending = &self.buffer[self.rendered_up_to..];

        // Find the last newline in the pending content
        let last_newline = pending.rfind('\n');
        let Some(last_nl_offset) = last_newline else {
            // No complete line yet — hold everything
            return String::new();
        };

        let complete = &pending[..=last_nl_offset];
        self.rendered_up_to += complete.len();

        let mut renderer = MarkdownRenderer::new(self.style);
        renderer.push(complete);
        renderer.finish()
    }
}

/// Internal markdown-to-ANSI renderer using pulldown-cmark events.
struct MarkdownRenderer<'a> {
    style: &'a CliStyle,
    output: String,
    /// Stack of active inline styles (bold, italic, etc.)
    emphasis_depth: usize,
    strong_depth: usize,
    in_code_block: bool,
    code_block_lang: Option<String>,
    code_block_buf: String,
    in_heading: bool,
    heading_level: u8,
    heading_buf: String,
    list_depth: usize,
    ordered_counters: Vec<u64>,
    in_list_item: bool,
    /// Track if we already emitted a blank line before a block
    last_was_blank: bool,
    in_block_quote: bool,
    block_quote_buf: String,
}

impl<'a> MarkdownRenderer<'a> {
    fn new(style: &'a CliStyle) -> Self {
        Self {
            style,
            output: String::new(),
            emphasis_depth: 0,
            strong_depth: 0,
            in_code_block: false,
            code_block_lang: None,
            code_block_buf: String::new(),
            in_heading: false,
            heading_level: 0,
            heading_buf: String::new(),
            list_depth: 0,
            ordered_counters: Vec::new(),
            in_list_item: false,
            last_was_blank: true,
            in_block_quote: false,
            block_quote_buf: String::new(),
        }
    }

    fn push(&mut self, text: &str) {
        let mut opts = Options::empty();
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TABLES);

        let parser = Parser::new_ext(text, opts);
        for event in parser {
            self.handle_event(event);
        }
    }

    fn finish(self) -> String {
        self.output
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.handle_start(tag),
            Event::End(tag) => self.handle_end(tag),
            Event::Text(text) => self.handle_text(&text),
            Event::Code(code) => self.handle_inline_code(&code),
            Event::SoftBreak => self.handle_soft_break(),
            Event::HardBreak => self.handle_hard_break(),
            Event::Rule => self.handle_rule(),
            _ => {}
        }
    }

    fn handle_start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.in_heading = true;
                self.heading_level = level as u8;
                self.heading_buf.clear();
            }
            Tag::Paragraph => {
                if !self.in_list_item && !self.in_heading && !self.in_block_quote {
                    if !self.last_was_blank {
                        self.output.push('\n');
                    }
                }
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                self.code_block_buf.clear();
                self.code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang = lang.to_string();
                        if lang.is_empty() {
                            None
                        } else {
                            Some(lang)
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
            }
            Tag::List(start) => {
                self.list_depth += 1;
                if let Some(n) = start {
                    self.ordered_counters.push(n);
                } else {
                    self.ordered_counters.push(0); // 0 = unordered
                }
                if self.list_depth == 1 && !self.last_was_blank {
                    self.output.push('\n');
                }
            }
            Tag::Item => {
                self.in_list_item = true;
            }
            Tag::BlockQuote(_) => {
                self.in_block_quote = true;
                self.block_quote_buf.clear();
            }
            Tag::Emphasis => {
                self.emphasis_depth += 1;
            }
            Tag::Strong => {
                self.strong_depth += 1;
            }
            Tag::Strikethrough => {}
            Tag::Link { .. } => {}
            _ => {}
        }
    }

    fn handle_end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.in_heading = false;
                let text = std::mem::take(&mut self.heading_buf);
                let rendered = match self.heading_level {
                    1 => format!("\n{}\n", self.style.bold_cyan(&format!("# {}", text))),
                    2 => format!("\n{}\n", self.style.bold_cyan(&format!("## {}", text))),
                    _ => format!(
                        "\n{}\n",
                        self.style.bold(&format!(
                            "{} {}",
                            "#".repeat(self.heading_level as usize),
                            text
                        ))
                    ),
                };
                self.output.push_str(&rendered);
                self.last_was_blank = false;
            }
            TagEnd::Paragraph => {
                if !self.in_list_item && !self.in_block_quote {
                    self.output.push('\n');
                    self.last_was_blank = true;
                }
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                let code = std::mem::take(&mut self.code_block_buf);
                let lang_label = self.code_block_lang.take();
                self.render_code_block(&code, lang_label.as_deref());
                self.last_was_blank = false;
            }
            TagEnd::List(_) => {
                self.list_depth = self.list_depth.saturating_sub(1);
                self.ordered_counters.pop();
                if self.list_depth == 0 {
                    self.output.push('\n');
                    self.last_was_blank = true;
                }
            }
            TagEnd::Item => {
                self.in_list_item = false;
            }
            TagEnd::BlockQuote(_) => {
                self.in_block_quote = false;
                let text = std::mem::take(&mut self.block_quote_buf);
                for line in text.lines() {
                    self.output.push_str(&format!(
                        "  {} {}\n",
                        self.style.dim("│"),
                        self.style.dim(line)
                    ));
                }
                self.last_was_blank = false;
            }
            TagEnd::Emphasis => {
                self.emphasis_depth = self.emphasis_depth.saturating_sub(1);
            }
            TagEnd::Strong => {
                self.strong_depth = self.strong_depth.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn handle_text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_block_buf.push_str(text);
            return;
        }

        if self.in_heading {
            self.heading_buf.push_str(text);
            return;
        }

        if self.in_block_quote {
            self.block_quote_buf.push_str(text);
            return;
        }

        if self.in_list_item {
            self.render_list_item_text(text);
            return;
        }

        let styled = self.apply_inline_style(text);
        self.output.push_str(&styled);
        self.last_was_blank = false;
    }

    fn handle_inline_code(&mut self, code: &str) {
        if self.in_heading {
            self.heading_buf.push_str(code);
            return;
        }
        if self.in_block_quote {
            self.block_quote_buf.push('`');
            self.block_quote_buf.push_str(code);
            self.block_quote_buf.push('`');
            return;
        }

        let rendered = self.style.cyan(&format!("`{}`", code));
        if self.in_list_item {
            self.output.push_str(&rendered);
        } else {
            self.output.push_str(&rendered);
        }
        self.last_was_blank = false;
    }

    fn handle_soft_break(&mut self) {
        if self.in_code_block {
            self.code_block_buf.push('\n');
        } else if self.in_heading {
            self.heading_buf.push(' ');
        } else if self.in_block_quote {
            self.block_quote_buf.push('\n');
        } else {
            self.output.push('\n');
        }
    }

    fn handle_hard_break(&mut self) {
        if self.in_code_block {
            self.code_block_buf.push('\n');
        } else {
            self.output.push('\n');
        }
    }

    fn handle_rule(&mut self) {
        self.output.push_str(&self.style.hr());
        self.output.push('\n');
        self.last_was_blank = true;
    }

    fn apply_inline_style(&self, text: &str) -> String {
        if self.strong_depth > 0 {
            self.style.bold(text)
        } else if self.emphasis_depth > 0 {
            self.style.dim(text) // italic → dim in terminal
        } else {
            text.to_string()
        }
    }

    fn render_list_item_text(&mut self, text: &str) {
        let indent = "  ".repeat(self.list_depth.saturating_sub(1));
        let is_ordered = self.ordered_counters.last().copied().unwrap_or(0);

        let bullet = if is_ordered > 0 {
            let counter = is_ordered;
            // Increment the counter for next item
            if let Some(c) = self.ordered_counters.last_mut() {
                *c += 1;
            }
            format!("{}. ", counter)
        } else {
            format!("{} ", self.style.dim("•"))
        };

        let styled = self.apply_inline_style(text);
        self.output
            .push_str(&format!("  {}{}{}\n", indent, bullet, styled));
        self.last_was_blank = false;
    }

    fn render_code_block(&mut self, code: &str, lang: Option<&str>) {
        let header = if let Some(lang) = lang {
            format!("  {} {}\n", self.style.dim("```"), self.style.dim(lang))
        } else {
            format!("  {}\n", self.style.dim("```"))
        };
        self.output.push_str(&header);

        for line in code.lines() {
            self.output
                .push_str(&format!("  {}\n", self.style.cyan(line)));
        }

        self.output
            .push_str(&format!("  {}\n", self.style.dim("```")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("# Hello World", &style);
        assert!(out.contains("# Hello World"));
        // Should have ANSI codes
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn renders_bold_text() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("This is **bold** text", &style);
        assert!(out.contains("bold"));
        // Bold uses ANSI \x1b[1m
        assert!(out.contains("\x1b[1m"));
    }

    #[test]
    fn renders_inline_code() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("Use `cargo test` to run", &style);
        assert!(out.contains("`cargo test`"));
        // Cyan ANSI code
        assert!(out.contains("\x1b[36m"));
    }

    #[test]
    fn renders_code_block() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("```rust\nfn main() {}\n```", &style);
        assert!(out.contains("fn main() {}"));
        assert!(out.contains("rust"));
    }

    #[test]
    fn renders_unordered_list() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("- item one\n- item two\n- item three", &style);
        assert!(out.contains("item one"));
        assert!(out.contains("item two"));
        assert!(out.contains("item three"));
    }

    #[test]
    fn renders_ordered_list() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("1. first\n2. second\n3. third", &style);
        assert!(out.contains("first"));
        assert!(out.contains("second"));
        assert!(out.contains("third"));
    }

    #[test]
    fn plain_mode_returns_raw_text() {
        let style = CliStyle::plain();
        let out = render_markdown("# Hello **World**", &style);
        assert_eq!(out, "# Hello **World**");
        assert!(!out.contains("\x1b["));
    }

    #[test]
    fn streamer_buffers_incomplete_lines() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let mut streamer = MarkdownStreamer::new(&style);

        // Push incomplete line — should buffer
        let out1 = streamer.push("Hello ");
        assert!(out1.is_empty());

        // Push more — still no newline
        let out2 = streamer.push("World");
        assert!(out2.is_empty());

        // Push newline — now it should render
        let out3 = streamer.push("\n");
        assert!(out3.contains("Hello World"));
    }

    #[test]
    fn streamer_finish_flushes_remaining() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let mut streamer = MarkdownStreamer::new(&style);

        streamer.push("trailing text");
        let out = streamer.finish();
        assert!(out.contains("trailing text"));
    }

    #[test]
    fn renders_block_quote() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("> quoted text", &style);
        assert!(out.contains("quoted text"));
        assert!(out.contains("│"));
    }

    #[test]
    fn renders_horizontal_rule() {
        let style = CliStyle {
            color: true,
            width: 80,
        };
        let out = render_markdown("---", &style);
        assert!(out.contains("─"));
    }
}
