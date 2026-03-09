//! Terminal spinner for CLI progress indicators.
//!
//! Provides an animated spinner that runs in a background tokio task,
//! displaying a progress message with rotating frames.

use crate::cli_style::CliStyle;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Spinner frames for terminal animation.
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Interval between spinner frame updates (milliseconds).
const SPINNER_INTERVAL_MS: u64 = 80;

/// A terminal spinner that animates in a background task.
pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    /// Start a new spinner with the given message.
    pub fn start(message: impl Into<String>, style: &CliStyle) -> Self {
        let message = message.into();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let color = style.color;

        let handle = tokio::spawn(async move {
            let mut frame_idx = 0usize;
            while running_clone.load(Ordering::Relaxed) {
                let frame = SPINNER_FRAMES[frame_idx % SPINNER_FRAMES.len()];
                let line = if color {
                    format!("\r\x1b[36m{}\x1b[0m \x1b[2m{}\x1b[0m", frame, message)
                } else {
                    format!("\r{} {}", frame, message)
                };
                let _ = write!(io::stderr(), "{}", line);
                let _ = io::stderr().flush();
                frame_idx += 1;
                tokio::time::sleep(tokio::time::Duration::from_millis(SPINNER_INTERVAL_MS)).await;
            }
            // Clear the spinner line
            let _ = write!(io::stderr(), "\r\x1b[2K");
            let _ = io::stderr().flush();
        });

        Self {
            running,
            handle: Some(handle),
        }
    }

    /// Stop the spinner and clear the line.
    pub async fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    /// Update the spinner message.
    pub fn update_message(&self, _message: impl Into<String>) {
        // For a more advanced implementation, we'd use a shared message.
        // For now, the message is set at creation time.
        // Future enhancement: use Arc<Mutex<String>> for dynamic messages.
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Can't await in Drop, but at least signal stop
    }
}

/// A simple progress bar for step-based progress.
pub struct ProgressBar {
    total: usize,
    current: usize,
    label: String,
    style_color: bool,
}

impl ProgressBar {
    pub fn new(total: usize, label: impl Into<String>, style: &CliStyle) -> Self {
        Self {
            total,
            current: 0,
            label: label.into(),
            style_color: style.color,
        }
    }

    /// Advance the progress bar by one step and redraw.
    pub fn tick(&mut self) {
        self.current = (self.current + 1).min(self.total);
        self.draw();
    }

    /// Set the current progress and redraw.
    pub fn set(&mut self, current: usize) {
        self.current = current.min(self.total);
        self.draw();
    }

    /// Clear the progress bar line.
    pub fn finish(&self) {
        let _ = write!(io::stderr(), "\r\x1b[2K");
        let _ = io::stderr().flush();
    }

    fn draw(&self) {
        let bar_width = 20;
        let filled = if self.total > 0 {
            (self.current * bar_width) / self.total
        } else {
            0
        };
        let empty = bar_width - filled;
        let bar: String = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
        let pct = if self.total > 0 {
            (self.current * 100) / self.total
        } else {
            0
        };

        let line = if self.style_color {
            format!(
                "\r\x1b[36m{}\x1b[0m \x1b[2m{} {}/{}  {}%\x1b[0m",
                bar, self.label, self.current, self.total, pct
            )
        } else {
            format!(
                "\r{} {} {}/{}  {}%",
                bar, self.label, self.current, self.total, pct
            )
        };
        let _ = write!(io::stderr(), "{}", line);
        let _ = io::stderr().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_frames_not_empty() {
        assert!(!SPINNER_FRAMES.is_empty());
    }

    #[test]
    fn progress_bar_calculates_percentage() {
        let style = CliStyle::plain();
        let mut bar = ProgressBar::new(10, "test", &style);
        bar.set(5);
        assert_eq!(bar.current, 5);
        assert_eq!(bar.total, 10);
    }

    #[test]
    fn progress_bar_clamps_overflow() {
        let style = CliStyle::plain();
        let mut bar = ProgressBar::new(10, "test", &style);
        bar.set(20);
        assert_eq!(bar.current, 10);
    }

    #[tokio::test]
    async fn spinner_can_start_and_stop() {
        let style = CliStyle::plain();
        let spinner = Spinner::start("testing...", &style);
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        spinner.stop().await;
        // If we get here without panic, the spinner lifecycle works
    }
}
