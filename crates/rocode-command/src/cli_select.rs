//! Interactive terminal selector for CLI question prompts.
//!
//! Provides a Claude Code–style interactive selection UI with:
//! - Arrow key / j/k navigation
//! - Visual cursor indicator (`❯`)
//! - "Other" option for free text input
//! - Multi-select support with checkboxes
//! - Enter to confirm selection

use crate::cli_panel::CliPanelFrame;
use crate::cli_style::CliStyle;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use std::io::{self, Write};

/// A single selectable option.
#[derive(Debug, Clone)]
pub struct SelectOption {
    pub label: String,
    pub description: Option<String>,
}

/// Result of an interactive selection.
#[derive(Debug, Clone)]
pub enum SelectResult {
    /// User selected one or more options by label.
    Selected(Vec<String>),
    /// User chose "Other" and typed custom text.
    Other(String),
    /// User cancelled (Ctrl+C / Escape).
    Cancelled,
}

/// Run an interactive single-select prompt.
///
/// Displays the question with options in a navigable list.
/// Includes an automatic "Other" option at the end.
///
/// Returns the selected option label, custom text, or cancellation.
pub fn interactive_select(
    question: &str,
    header: Option<&str>,
    options: &[SelectOption],
    style: &CliStyle,
) -> io::Result<SelectResult> {
    if !style.color || options.is_empty() {
        // Fallback: plain numbered list if not a TTY
        return fallback_select(question, header, options);
    }

    let total_items = options.len() + 1; // +1 for "Other"
    run_selector(question, header, options, total_items, false, style)
}

/// Run an interactive multi-select prompt.
///
/// Displays options with checkboxes. Space toggles selection,
/// Enter confirms.
pub fn interactive_multi_select(
    question: &str,
    header: Option<&str>,
    options: &[SelectOption],
    style: &CliStyle,
) -> io::Result<SelectResult> {
    if !style.color || options.is_empty() {
        return fallback_select(question, header, options);
    }

    let total_items = options.len() + 1;
    run_selector(question, header, options, total_items, true, style)
}

fn run_selector(
    question: &str,
    header: Option<&str>,
    options: &[SelectOption],
    total_items: usize,
    multi: bool,
    style: &CliStyle,
) -> io::Result<SelectResult> {
    let mut cursor_pos: usize = 0;
    let mut selected: Vec<bool> = vec![false; total_items];
    let frame = CliPanelFrame::boxed(
        header.unwrap_or("Question"),
        Some(if multi {
            "↑↓ move  •  Space toggle  •  Enter confirm  •  Esc cancel"
        } else {
            "↑↓ move  •  Enter confirm  •  Esc cancel"
        }),
        style,
    );
    let mut stdout = io::stdout();

    // Enter raw mode for key-by-key reading
    terminal::enable_raw_mode()?;
    // Hide cursor during selection
    execute!(stdout, cursor::Hide)?;

    // Initial draw
    let mut lines_drawn = draw_panel(
        &mut stdout,
        &frame,
        question,
        options,
        cursor_pos,
        &selected,
        multi,
        style,
    )?;

    loop {
        if let Event::Key(key) = event::read()? {
            if !is_primary_key_event(&key) {
                continue;
            }
            match key {
                // Navigation: Up / k
                KeyEvent {
                    code: KeyCode::Up, ..
                }
                | KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => {
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                    } else {
                        cursor_pos = total_items - 1; // wrap around
                    }
                }
                // Navigation: Down / j
                KeyEvent {
                    code: KeyCode::Down,
                    ..
                }
                | KeyEvent {
                    code: KeyCode::Char('j'),
                    modifiers: KeyModifiers::NONE,
                    ..
                } => {
                    if cursor_pos < total_items - 1 {
                        cursor_pos += 1;
                    } else {
                        cursor_pos = 0; // wrap around
                    }
                }
                // Toggle selection (multi-select) or quick-select by number
                KeyEvent {
                    code: KeyCode::Char(' '),
                    ..
                } if multi => {
                    selected[cursor_pos] = !selected[cursor_pos];
                }
                // Number shortcuts: 1-9
                KeyEvent {
                    code: KeyCode::Char(c),
                    ..
                } if c.is_ascii_digit() => {
                    let num = c.to_digit(10).unwrap_or(0) as usize;
                    if num >= 1 && num <= options.len() {
                        let idx = num - 1;
                        if multi {
                            selected[idx] = !selected[idx];
                            cursor_pos = idx;
                        } else {
                            // Single select: pick immediately
                            cleanup_terminal(&mut stdout, lines_drawn)?;
                            print_final_selection(
                                &mut stdout,
                                question,
                                &[options[idx].label.clone()],
                                style,
                            )?;
                            return Ok(SelectResult::Selected(vec![options[idx].label.clone()]));
                        }
                    }
                }
                // Enter: confirm
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } => {
                    cleanup_terminal(&mut stdout, lines_drawn)?;

                    // Check if "Other" is selected
                    if cursor_pos == options.len() {
                        // "Other" selected — prompt for free text
                        let text = prompt_other_text(&mut stdout, question, style)?;
                        return if text.trim().is_empty() {
                            Ok(SelectResult::Cancelled)
                        } else {
                            Ok(SelectResult::Other(text))
                        };
                    }

                    if multi {
                        // Collect all checked items
                        let choices: Vec<String> = selected
                            .iter()
                            .enumerate()
                            .filter(|(i, &checked)| checked && *i < options.len())
                            .map(|(i, _)| options[i].label.clone())
                            .collect();
                        if choices.is_empty() {
                            // Nothing checked — use cursor position
                            print_final_selection(
                                &mut stdout,
                                question,
                                &[options[cursor_pos].label.clone()],
                                style,
                            )?;
                            return Ok(SelectResult::Selected(vec![options[cursor_pos]
                                .label
                                .clone()]));
                        }
                        print_final_selection(&mut stdout, question, &choices, style)?;
                        return Ok(SelectResult::Selected(choices));
                    } else {
                        // Single select: use cursor position
                        print_final_selection(
                            &mut stdout,
                            question,
                            &[options[cursor_pos].label.clone()],
                            style,
                        )?;
                        return Ok(SelectResult::Selected(vec![options[cursor_pos]
                            .label
                            .clone()]));
                    }
                }
                // Escape or Ctrl+C: cancel
                KeyEvent {
                    code: KeyCode::Esc, ..
                }
                | KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    cleanup_terminal(&mut stdout, lines_drawn)?;
                    return Ok(SelectResult::Cancelled);
                }
                _ => {}
            }

            // Redraw options
            erase_lines(&mut stdout, lines_drawn)?;
            lines_drawn = draw_panel(
                &mut stdout,
                &frame,
                question,
                options,
                cursor_pos,
                &selected,
                multi,
                style,
            )?;
        }
    }
}

fn draw_panel(
    out: &mut impl Write,
    frame: &CliPanelFrame,
    question: &str,
    options: &[SelectOption],
    cursor_pos: usize,
    selected: &[bool],
    multi: bool,
    style: &CliStyle,
) -> io::Result<usize> {
    let mut lines = vec![
        format!("{} {}", style.bullet(), question),
        String::new(),
    ];

    for (i, opt) in options.iter().enumerate() {
        let is_cursor = i == cursor_pos;
        let is_selected = selected.get(i).copied().unwrap_or(false);

        let pointer = if is_cursor {
            "❯".to_string()
        } else {
            " ".to_string()
        };

        let checkbox = if multi {
            if is_selected {
                "◉ ".to_string()
            } else {
                "○ ".to_string()
            }
        } else {
            String::new()
        };

        let label = opt.label.clone();

        let desc = match &opt.description {
            Some(d) if is_cursor => format!(" — {}", d),
            _ => String::new(),
        };

        lines.push(format!(
            " {} {}{}. {}{}",
            pointer, checkbox, i + 1, label, desc
        ));
    }

    // "Other" option
    let other_idx = options.len();
    let is_cursor = cursor_pos == other_idx;
    let pointer = if is_cursor {
        "❯".to_string()
    } else {
        " ".to_string()
    };
    lines.push(format!(" {} · Other", pointer));

    let rendered = frame.render_lines(&lines);
    write!(out, "{rendered}")?;
    out.flush()?;
    Ok(frame.rendered_line_count(&lines))
}

fn erase_lines(out: &mut impl Write, count: usize) -> io::Result<()> {
    for _ in 0..count {
        execute!(
            out,
            cursor::MoveUp(1),
            terminal::Clear(ClearType::CurrentLine)
        )?;
    }
    Ok(())
}

fn cleanup_terminal(out: &mut impl Write, lines_drawn: usize) -> io::Result<()> {
    erase_lines(out, lines_drawn)?;
    execute!(out, cursor::Show)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

fn print_final_selection(
    out: &mut impl Write,
    question: &str,
    choices: &[String],
    style: &CliStyle,
) -> io::Result<()> {
    let answer = choices.join(", ");
    write!(
        out,
        "  {} {} {}\r\n",
        style.bold_green(style.check()),
        style.dim(question),
        style.bold_cyan(&answer)
    )?;
    out.flush()
}

fn prompt_other_text(out: &mut impl Write, question: &str, style: &CliStyle) -> io::Result<String> {
    // Restore terminal to normal mode for text input
    execute!(out, cursor::Show)?;
    terminal::disable_raw_mode()?;

    write!(out, "  {} {} ", style.bold_cyan("›"), style.dim(question),)?;
    out.flush()?;

    // Read from stdin (not stderr)
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let answer = input.trim().to_string();

    // Erase the input line and show final
    execute!(
        io::stdout(),
        cursor::MoveUp(1),
        terminal::Clear(ClearType::CurrentLine)
    )?;
    write!(
        io::stdout(),
        "  {} {} {}\r\n",
        style.bold_green(style.check()),
        style.dim(question),
        style.bold_cyan(&answer)
    )?;
    io::stdout().flush()?;

    Ok(answer)
}

fn is_primary_key_event(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
}

/// Fallback for non-TTY / no options: plain numbered prompt.
fn fallback_select(
    question: &str,
    _header: Option<&str>,
    options: &[SelectOption],
) -> io::Result<SelectResult> {
    println!();
    println!("  {}", question);
    println!();
    for (i, opt) in options.iter().enumerate() {
        if let Some(ref desc) = opt.description {
            println!("  {}. {} — {}", i + 1, opt.label, desc);
        } else {
            println!("  {}. {}", i + 1, opt.label);
        }
    }
    println!("  ·  Other");
    println!();
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() && !options.is_empty() {
        return Ok(SelectResult::Selected(vec![options[0].label.clone()]));
    }

    if let Ok(num) = input.parse::<usize>() {
        if num >= 1 && num <= options.len() {
            return Ok(SelectResult::Selected(vec![options[num - 1].label.clone()]));
        }
    }

    // Treat as free text (Other)
    Ok(SelectResult::Other(input.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_select_with_empty_options() {
        // Just verify the SelectOption struct works
        let opts = vec![
            SelectOption {
                label: "Yes".to_string(),
                description: Some("Confirm".to_string()),
            },
            SelectOption {
                label: "No".to_string(),
                description: None,
            },
        ];
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].label, "Yes");
        assert_eq!(opts[1].description, None);
    }

    #[test]
    fn select_result_variants() {
        let selected = SelectResult::Selected(vec!["Yes".to_string()]);
        let other = SelectResult::Other("custom".to_string());
        let cancelled = SelectResult::Cancelled;

        match selected {
            SelectResult::Selected(v) => assert_eq!(v, vec!["Yes"]),
            _ => panic!("expected Selected"),
        }
        match other {
            SelectResult::Other(s) => assert_eq!(s, "custom"),
            _ => panic!("expected Other"),
        }
        match cancelled {
            SelectResult::Cancelled => {}
            _ => panic!("expected Cancelled"),
        }
    }
}
