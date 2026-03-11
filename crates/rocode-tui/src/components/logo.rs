use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

const LOGO: [&str; 3] = [
    "█▀▀▄ █▀▀█ █▀▀ ▄▄▄▄ ▄▄▄█ ▄▄▄▄",
    "█▀█▀ █  █ █   █  █ █  █ █▀▀▀",
    "▀ ▀▀ ▀▀▀▀ ▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀",
];

pub fn exit_logo_lines(pad: &str) -> Vec<String> {
    LOGO.iter().map(|line| format!("{pad}{line}")).collect()
}

pub struct Logo {
    primary_color: Color,
    muted_color: Color,
}

impl Logo {
    // 移除了 text_muted_color 前面的下划线，将其存入结构体
    pub fn new(text_color: Color, text_muted_color: Color, _bg_color: Color) -> Self {
        Self {
            primary_color: text_color,
            muted_color: text_muted_color,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let lines: Vec<Line> = LOGO
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let color = if idx == 0 {
                    self.primary_color
                } else {
                    self.muted_color
                };
                Line::from(Span::styled(
                    *line,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))
            })
            .collect();

        let paragraph =
            Paragraph::new(Text::from(lines)).alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}
