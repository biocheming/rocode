use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};

// 将字符串拆分为 (RO部分, 剩余部分) 的元组，避免运行时处理 UTF-8 字节切片
const LOGO: [(&str, &str); 4] = [
    ("  █▀▀▄ █▀▀█ ", "█▀▀▀         █     "),
    ("  █  █ █  █ ", "█    █▀▀█ █▀▀█ █▀▀█"),
    ("  █▀█▀ █  █ ", "█    █  █ █  █ █▀▀▀"),
    ("  ▀ ▀▀ ▀▀▀▀ ", "▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀"),
];

pub fn exit_logo_lines(pad: &str) -> Vec<String> {
    (0..4)
        // 将拆分后的两部分重新拼接并格式化
        .map(|i| format!("{pad}{}{}", LOGO[i].0, LOGO[i].1))
        .collect()
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
            .map(|(ro, rest)| {
                // 每行由两个 Span 组成，分别应用不同的颜色
                Line::from(vec![
                    Span::styled(
                        *ro,
                        Style::default()
                            .fg(self.muted_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        *rest,
                        Style::default()
                            .fg(self.primary_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            })
            .collect();

        let paragraph =
            Paragraph::new(Text::from(lines)).alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(paragraph, area);
    }
}
