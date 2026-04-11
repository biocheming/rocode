use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::api::{SkillCatalogEntry, SkillDetailResponse};
use crate::theme::Theme;

#[derive(Clone, Debug, Default)]
struct TextEditorState {
    text: String,
    cursor: usize,
    scroll: u16,
}

impl TextEditorState {
    fn with_text(text: String) -> Self {
        let cursor = text.len();
        let mut state = Self {
            text,
            cursor,
            scroll: 0,
        };
        state.sync_scroll_to_cursor();
        state
    }

    fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.sync_scroll_to_cursor();
    }

    fn insert_newline(&mut self) {
        self.text.insert(self.cursor, '\n');
        self.cursor += 1;
        self.sync_scroll_to_cursor();
    }

    fn backspace(&mut self) {
        if let Some(prev) = prev_char_boundary(&self.text, self.cursor) {
            self.text.replace_range(prev..self.cursor, "");
            self.cursor = prev;
            self.sync_scroll_to_cursor();
        }
    }

    fn delete(&mut self) {
        if let Some(next) = next_char_boundary(&self.text, self.cursor) {
            self.text.replace_range(self.cursor..next, "");
            self.sync_scroll_to_cursor();
        }
    }

    fn move_left(&mut self) {
        if let Some(prev) = prev_char_boundary(&self.text, self.cursor) {
            self.cursor = prev;
            self.sync_scroll_to_cursor();
        }
    }

    fn move_right(&mut self) {
        if let Some(next) = next_char_boundary(&self.text, self.cursor) {
            self.cursor = next;
            self.sync_scroll_to_cursor();
        }
    }

    fn move_home(&mut self) {
        self.cursor = line_start(&self.text, self.cursor);
        self.sync_scroll_to_cursor();
    }

    fn move_end(&mut self) {
        self.cursor = line_end(&self.text, self.cursor);
        self.sync_scroll_to_cursor();
    }

    fn cursor_row_col(&self) -> (u16, u16) {
        let prefix = &self.text[..self.cursor.min(self.text.len())];
        let row = prefix.bytes().filter(|byte| *byte == b'\n').count() as u16;
        let col = prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count() as u16;
        (row, col)
    }

    fn sync_scroll_to_cursor(&mut self) {
        let (row, _) = self.cursor_row_col();
        self.scroll = row.saturating_sub(4);
    }

    fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkillCreateField {
    Name,
    Description,
    Category,
    Body,
}

impl SkillCreateField {
    fn next(self) -> Self {
        match self {
            Self::Name => Self::Description,
            Self::Description => Self::Category,
            Self::Category => Self::Body,
            Self::Body => Self::Name,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Name => Self::Body,
            Self::Description => Self::Name,
            Self::Category => Self::Description,
            Self::Body => Self::Category,
        }
    }
}

#[derive(Clone, Debug)]
struct SkillCreateDraft {
    active_field: SkillCreateField,
    name: String,
    description: String,
    category: String,
    body: TextEditorState,
}

impl Default for SkillCreateDraft {
    fn default() -> Self {
        Self {
            active_field: SkillCreateField::Name,
            name: String::new(),
            description: String::new(),
            category: String::new(),
            body: TextEditorState::default(),
        }
    }
}

#[derive(Clone, Debug)]
struct SkillEditDraft {
    name: String,
    source: TextEditorState,
}

#[derive(Clone, Debug)]
enum SkillListMode {
    Browse,
    Create(SkillCreateDraft),
    Edit(SkillEditDraft),
    ConfirmDelete { name: String },
}

pub struct SkillListDialog {
    skills: Vec<SkillCatalogEntry>,
    filtered: Vec<usize>,
    query: String,
    detail: Option<SkillDetailResponse>,
    detail_error: Option<String>,
    detail_scroll: u16,
    state: ListState,
    mode: SkillListMode,
    open: bool,
}

impl SkillListDialog {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            skills: Vec::new(),
            filtered: Vec::new(),
            query: String::new(),
            detail: None,
            detail_error: None,
            detail_scroll: 0,
            state,
            mode: SkillListMode::Browse,
            open: false,
        }
    }

    pub fn set_skills(&mut self, mut skills: Vec<SkillCatalogEntry>) {
        skills.sort_by_key(|skill| skill.name.to_ascii_lowercase());
        skills.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
        self.skills = skills;
        self.clear_detail();
        self.filter();
    }

    pub fn open(&mut self) {
        self.open = true;
        self.mode = SkillListMode::Browse;
        self.query.clear();
        self.detail_scroll = 0;
        self.filter();
    }

    pub fn close(&mut self) {
        self.open = false;
        self.mode = SkillListMode::Browse;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_browse_mode(&self) -> bool {
        matches!(self.mode, SkillListMode::Browse)
    }

    pub fn is_create_mode(&self) -> bool {
        matches!(self.mode, SkillListMode::Create(_))
    }

    pub fn is_edit_mode(&self) -> bool {
        matches!(self.mode, SkillListMode::Edit(_))
    }

    pub fn is_delete_confirm_mode(&self) -> bool {
        matches!(self.mode, SkillListMode::ConfirmDelete { .. })
    }

    pub fn handle_input(&mut self, c: char) {
        self.query.push(c);
        self.filter();
    }

    pub fn handle_backspace(&mut self) {
        self.query.pop();
        self.filter();
    }

    pub fn move_up(&mut self) {
        if let Some(selected) = self.state.selected() {
            if selected > 0 {
                self.state.select(Some(selected - 1));
                self.detail_scroll = 0;
            }
        }
    }

    pub fn move_down(&mut self) {
        if let Some(selected) = self.state.selected() {
            if selected < self.filtered.len().saturating_sub(1) {
                self.state.select(Some(selected + 1));
                self.detail_scroll = 0;
            }
        }
    }

    pub fn preview_scroll_up(&mut self) {
        if self.detail_scroll > 0 {
            self.detail_scroll -= 1;
        }
    }

    pub fn preview_scroll_down(&mut self) {
        self.detail_scroll = self.detail_scroll.saturating_add(1);
    }

    pub fn selected_skill(&self) -> Option<&str> {
        let idx = self.state.selected().and_then(|s| self.filtered.get(s))?;
        self.skills.get(*idx).map(|skill| skill.name.as_str())
    }

    pub fn selected_entry(&self) -> Option<&SkillCatalogEntry> {
        let idx = self.state.selected().and_then(|s| self.filtered.get(s))?;
        self.skills.get(*idx)
    }

    pub fn set_skill_detail(&mut self, detail: SkillDetailResponse) {
        self.detail = Some(detail);
        self.detail_error = None;
        self.detail_scroll = 0;
    }

    pub fn set_skill_detail_error(&mut self, message: impl Into<String>) {
        self.detail = None;
        self.detail_error = Some(message.into());
        self.detail_scroll = 0;
    }

    pub fn clear_detail(&mut self) {
        self.detail = None;
        self.detail_error = None;
        self.detail_scroll = 0;
    }

    pub fn begin_create(&mut self) {
        self.mode = SkillListMode::Create(SkillCreateDraft::default());
    }

    pub fn begin_edit(&mut self) -> Result<(), String> {
        let detail = self
            .detail
            .clone()
            .ok_or_else(|| "Load a skill before editing it.".to_string())?;
        if !detail.writable {
            return Err(
                "This skill is read-only in the current workspace. Only workspace-local skills can be edited.".to_string(),
            );
        }
        self.mode = SkillListMode::Edit(SkillEditDraft {
            name: detail.skill.meta.name,
            source: TextEditorState::with_text(detail.source),
        });
        Ok(())
    }

    pub fn begin_delete(&mut self) -> Result<(), String> {
        let entry = self
            .selected_entry()
            .cloned()
            .ok_or_else(|| "Select a skill before deleting it.".to_string())?;
        if !entry.writable {
            return Err(
                "This skill is read-only in the current workspace. Only workspace-local skills can be deleted.".to_string(),
            );
        }
        self.mode = SkillListMode::ConfirmDelete { name: entry.name };
        Ok(())
    }

    pub fn cancel_manage_mode(&mut self) {
        self.mode = SkillListMode::Browse;
    }

    pub fn create_payload(&self) -> Option<(String, String, Option<String>, String)> {
        let SkillListMode::Create(draft) = &self.mode else {
            return None;
        };
        Some((
            draft.name.trim().to_string(),
            draft.description.trim().to_string(),
            (!draft.category.trim().is_empty()).then(|| draft.category.trim().to_string()),
            draft.body.text.clone(),
        ))
    }

    pub fn edit_payload(&self) -> Option<(String, String)> {
        let SkillListMode::Edit(draft) = &self.mode else {
            return None;
        };
        Some((draft.name.clone(), draft.source.text.clone()))
    }

    pub fn delete_payload(&self) -> Option<String> {
        let SkillListMode::ConfirmDelete { name } = &self.mode else {
            return None;
        };
        Some(name.clone())
    }

    pub fn handle_manage_char(&mut self, c: char) {
        match &mut self.mode {
            SkillListMode::Create(draft) => match draft.active_field {
                SkillCreateField::Name => draft.name.push(c),
                SkillCreateField::Description => draft.description.push(c),
                SkillCreateField::Category => draft.category.push(c),
                SkillCreateField::Body => draft.body.insert_char(c),
            },
            SkillListMode::Edit(draft) => draft.source.insert_char(c),
            _ => {}
        }
    }

    pub fn handle_manage_backspace(&mut self) {
        match &mut self.mode {
            SkillListMode::Create(draft) => match draft.active_field {
                SkillCreateField::Name => {
                    draft.name.pop();
                }
                SkillCreateField::Description => {
                    draft.description.pop();
                }
                SkillCreateField::Category => {
                    draft.category.pop();
                }
                SkillCreateField::Body => draft.body.backspace(),
            },
            SkillListMode::Edit(draft) => draft.source.backspace(),
            _ => {}
        }
    }

    pub fn handle_manage_delete(&mut self) {
        if let SkillListMode::Edit(draft) = &mut self.mode {
            draft.source.delete();
        }
    }

    pub fn handle_manage_left(&mut self) {
        if let SkillListMode::Edit(draft) = &mut self.mode {
            draft.source.move_left();
        }
    }

    pub fn handle_manage_right(&mut self) {
        if let SkillListMode::Edit(draft) = &mut self.mode {
            draft.source.move_right();
        }
    }

    pub fn handle_manage_home(&mut self) {
        if let SkillListMode::Edit(draft) = &mut self.mode {
            draft.source.move_home();
        }
    }

    pub fn handle_manage_end(&mut self) {
        if let SkillListMode::Edit(draft) = &mut self.mode {
            draft.source.move_end();
        }
    }

    pub fn handle_manage_enter(&mut self) {
        match &mut self.mode {
            SkillListMode::Create(draft) => match draft.active_field {
                SkillCreateField::Name => draft.active_field = SkillCreateField::Description,
                SkillCreateField::Description => draft.active_field = SkillCreateField::Category,
                SkillCreateField::Category => draft.active_field = SkillCreateField::Body,
                SkillCreateField::Body => draft.body.insert_newline(),
            },
            SkillListMode::Edit(draft) => draft.source.insert_newline(),
            _ => {}
        }
    }

    pub fn handle_manage_tab(&mut self, reverse: bool) {
        if let SkillListMode::Create(draft) = &mut self.mode {
            draft.active_field = if reverse {
                draft.active_field.previous()
            } else {
                draft.active_field.next()
            };
        }
    }

    pub fn handle_manage_page_up(&mut self) {
        match &mut self.mode {
            SkillListMode::Create(draft) => {
                if matches!(draft.active_field, SkillCreateField::Body) {
                    draft.body.scroll_up();
                }
            }
            SkillListMode::Edit(draft) => draft.source.scroll_up(),
            _ => {}
        }
    }

    pub fn handle_manage_page_down(&mut self) {
        match &mut self.mode {
            SkillListMode::Create(draft) => {
                if matches!(draft.active_field, SkillCreateField::Body) {
                    draft.body.scroll_down();
                }
            }
            SkillListMode::Edit(draft) => draft.source.scroll_down(),
            _ => {}
        }
    }

    fn filter(&mut self) {
        let query = self.query.to_ascii_lowercase();
        self.filtered = self
            .skills
            .iter()
            .enumerate()
            .filter(|(_, skill)| {
                skill.name.to_ascii_lowercase().contains(&query)
                    || skill.description.to_ascii_lowercase().contains(&query)
                    || skill
                        .category
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(&query)
            })
            .map(|(idx, _)| idx)
            .collect();
        self.state.select(if self.filtered.is_empty() {
            None
        } else {
            Some(0)
        });
        self.detail_scroll = 0;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.open {
            return;
        }

        match &self.mode {
            SkillListMode::Browse => self.render_browse(frame, area, theme),
            SkillListMode::Create(draft) => self.render_create(frame, area, theme, draft),
            SkillListMode::Edit(draft) => self.render_edit(frame, area, theme, draft),
            SkillListMode::ConfirmDelete { name } => {
                self.render_delete_confirm(frame, area, theme, name)
            }
        }
    }

    fn render_browse(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let total_count = self.skills.len();
        let matched_count = self.filtered.len();
        let title = format!(" Skills ({}/{}) ", matched_count, total_count);
        let dialog_area = centered_rect(88, 22, area);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_panel));
        let inner = super::dialog_inner(block.inner(dialog_area));
        frame.render_widget(block, dialog_area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("> ", Style::default().fg(theme.primary)),
                Span::styled(&self.query, Style::default().fg(theme.text)),
                Span::styled("▏", Style::default().fg(theme.primary)),
            ])),
            layout[0],
        );

        let content_layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
            .split(layout[1]);

        let items = if self.filtered.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "No skills available",
                Style::default().fg(theme.text_muted),
            )))]
        } else {
            self.filtered
                .iter()
                .filter_map(|idx| self.skills.get(*idx))
                .map(|skill| {
                    let mut lines = vec![Line::from(vec![
                        Span::styled("/", Style::default().fg(theme.primary)),
                        Span::styled(
                            &skill.name,
                            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                        ),
                    ])];
                    let mut detail = String::new();
                    if let Some(category) =
                        skill.category.as_deref().filter(|value| !value.is_empty())
                    {
                        detail.push('[');
                        detail.push_str(category);
                        detail.push_str("] ");
                    }
                    detail.push_str(&skill.description);
                    if !skill.supporting_files.is_empty() {
                        detail.push_str(&format!(" · files {}", skill.supporting_files.len()));
                    }
                    if skill.writable {
                        detail.push_str(" · workspace");
                    } else {
                        detail.push_str(" · read-only");
                    }
                    lines.push(Line::from(Span::styled(
                        detail,
                        Style::default().fg(theme.text_muted),
                    )));
                    lines.push(Line::from(Span::styled(
                        skill.location.as_str(),
                        Style::default().fg(theme.text_muted),
                    )));
                    ListItem::new(Text::from(lines))
                })
                .collect::<Vec<_>>()
        };

        frame.render_stateful_widget(
            List::new(items).highlight_style(
                Style::default()
                    .bg(theme.background_element)
                    .add_modifier(Modifier::BOLD),
            ),
            content_layout[0],
            &mut self.state.clone(),
        );

        let preview_block = Block::default()
            .title(Span::styled(
                " Preview ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_panel));
        let preview_inner = super::dialog_inner(preview_block.inner(content_layout[1]));
        frame.render_widget(preview_block, content_layout[1]);

        let preview_lines = if let Some(detail) = &self.detail {
            let meta = &detail.skill.meta;
            let mut lines = vec![
                Line::from(Span::styled(
                    meta.name.as_str(),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    meta.description.as_str(),
                    Style::default().fg(theme.text_muted),
                )),
                Line::from(Span::styled(
                    format!(
                        "{} · {} supporting files · {}",
                        meta.category.as_deref().unwrap_or("uncategorized"),
                        meta.supporting_files.len(),
                        if detail.writable {
                            "workspace writable"
                        } else {
                            "read-only"
                        }
                    ),
                    Style::default().fg(theme.text_muted),
                )),
                Line::from(Span::styled(
                    meta.location.as_str(),
                    Style::default().fg(theme.text_muted),
                )),
                Line::from(""),
            ];
            lines.extend(detail.source.lines().map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.text),
                ))
            }));
            lines
        } else if let Some(message) = &self.detail_error {
            vec![Line::from(Span::styled(
                message.as_str(),
                Style::default().fg(theme.error),
            ))]
        } else {
            vec![Line::from(Span::styled(
                "Select a skill to load its raw SKILL.md preview.",
                Style::default().fg(theme.text_muted),
            ))]
        };

        frame.render_widget(
            Paragraph::new(preview_lines)
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0))
                .style(Style::default().bg(theme.background_panel)),
            preview_inner,
        );

        let footer = format!(
            "Enter insert /skill  c create  e edit  d delete  PgUp/PgDn preview  Esc close  Matched: {}/{}",
            matched_count, total_count
        );
        frame.render_widget(
            Paragraph::new(footer).style(Style::default().fg(theme.text_muted)),
            layout[2],
        );
    }

    fn render_create(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        draft: &SkillCreateDraft,
    ) {
        let dialog_area = centered_rect(92, 24, area);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(Span::styled(
                " Create Skill ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_panel));
        let inner = super::dialog_inner(block.inner(dialog_area));
        frame.render_widget(block, dialog_area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(8),
                Constraint::Length(2),
            ])
            .split(inner);

        render_field_line(
            frame,
            layout[0],
            theme,
            "Name",
            &draft.name,
            matches!(draft.active_field, SkillCreateField::Name),
        );
        render_field_line(
            frame,
            layout[1],
            theme,
            "Description",
            &draft.description,
            matches!(draft.active_field, SkillCreateField::Description),
        );
        render_field_line(
            frame,
            layout[2],
            theme,
            "Category",
            &draft.category,
            matches!(draft.active_field, SkillCreateField::Category),
        );

        let body_block = Block::default()
            .title(Span::styled(
                " Body ",
                field_block_style(theme, matches!(draft.active_field, SkillCreateField::Body))
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(field_block_style(
                theme,
                matches!(draft.active_field, SkillCreateField::Body),
            ))
            .style(Style::default().bg(theme.background_panel));
        let body_inner = super::dialog_inner(body_block.inner(layout[3]));
        frame.render_widget(body_block, layout[3]);
        frame.render_widget(
            Paragraph::new(if draft.body.text.is_empty() {
                vec![Line::from(Span::styled(
                    "Write markdown body here...",
                    Style::default().fg(theme.text_muted),
                ))]
            } else {
                draft
                    .body
                    .text
                    .lines()
                    .map(|line| {
                        Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(theme.text),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .wrap(Wrap { trim: false })
            .scroll((draft.body.scroll, 0)),
            body_inner,
        );

        let footer = vec![
            Line::from(Span::styled(
                "Tab/Shift+Tab move fields  Enter newline/body-next  Ctrl+S create  Esc cancel",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(Span::styled(
                "Creates a workspace-local skill via /skill/manage.",
                Style::default().fg(theme.text_muted),
            )),
        ];
        frame.render_widget(Paragraph::new(footer), layout[4]);

        match draft.active_field {
            SkillCreateField::Name => {
                set_line_cursor(frame, layout[0], "Name", draft.name.chars().count() as u16);
            }
            SkillCreateField::Description => {
                set_line_cursor(
                    frame,
                    layout[1],
                    "Description",
                    draft.description.chars().count() as u16,
                );
            }
            SkillCreateField::Category => {
                set_line_cursor(
                    frame,
                    layout[2],
                    "Category",
                    draft.category.chars().count() as u16,
                );
            }
            SkillCreateField::Body => {
                let (row, col) = draft.body.cursor_row_col();
                let cursor_y = body_inner
                    .y
                    .saturating_add(row.saturating_sub(draft.body.scroll));
                let cursor_x = body_inner.x.saturating_add(col);
                if cursor_y < body_inner.y.saturating_add(body_inner.height) {
                    frame.set_cursor(cursor_x, cursor_y);
                }
            }
        }
    }

    fn render_edit(&self, frame: &mut Frame, area: Rect, theme: &Theme, draft: &SkillEditDraft) {
        let dialog_area = centered_rect(92, 24, area);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(Span::styled(
                format!(" Edit Skill: {} ", draft.name),
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(theme.background_panel));
        let inner = super::dialog_inner(block.inner(dialog_area));
        frame.render_widget(block, dialog_area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(10),
                Constraint::Length(1),
            ])
            .split(inner);

        let header = vec![
            Line::from(Span::styled(
                "Editing raw SKILL.md source from the workspace authority.",
                Style::default().fg(theme.text_muted),
            )),
            Line::from(Span::styled(
                "Ctrl+S save  Esc cancel  PageUp/PageDown scroll",
                Style::default().fg(theme.text_muted),
            )),
        ];
        frame.render_widget(Paragraph::new(header), layout[0]);

        let editor_block = Block::default()
            .title(Span::styled(
                " Source ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.primary))
            .style(Style::default().bg(theme.background_panel));
        let editor_inner = super::dialog_inner(editor_block.inner(layout[1]));
        frame.render_widget(editor_block, layout[1]);
        frame.render_widget(
            Paragraph::new(
                draft
                    .source
                    .text
                    .lines()
                    .map(|line| {
                        Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(theme.text),
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
            .wrap(Wrap { trim: false })
            .scroll((draft.source.scroll, 0)),
            editor_inner,
        );

        frame.render_widget(
            Paragraph::new("Saving uses POST /skill/manage action=edit.")
                .style(Style::default().fg(theme.text_muted)),
            layout[2],
        );

        let (row, col) = draft.source.cursor_row_col();
        let cursor_y = editor_inner
            .y
            .saturating_add(row.saturating_sub(draft.source.scroll));
        let cursor_x = editor_inner.x.saturating_add(col);
        if cursor_y < editor_inner.y.saturating_add(editor_inner.height) {
            frame.set_cursor(cursor_x, cursor_y);
        }
    }

    fn render_delete_confirm(&self, frame: &mut Frame, area: Rect, theme: &Theme, name: &str) {
        let dialog_area = centered_rect(64, 8, area);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(Span::styled(
                " Delete Skill ",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.warning))
            .style(Style::default().bg(theme.background_panel));
        let inner = super::dialog_inner(block.inner(dialog_area));
        frame.render_widget(block, dialog_area);

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("Delete workspace skill `{}`?", name),
                    Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "This will call POST /skill/manage action=delete.",
                    Style::default().fg(theme.text_muted),
                )),
            ]),
            layout[0],
        );
        frame.render_widget(
            Paragraph::new("Enter confirm  Esc cancel")
                .style(Style::default().fg(theme.text_muted)),
            layout[1],
        );
    }
}

impl Default for SkillListDialog {
    fn default() -> Self {
        Self::new()
    }
}

fn render_field_line(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    label: &str,
    value: &str,
    active: bool,
) {
    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", label),
            field_block_style(theme, active).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(field_block_style(theme, active))
        .style(Style::default().bg(theme.background_panel));
    let inner = super::dialog_inner(block.inner(area));
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(if value.is_empty() {
            Line::from(Span::styled(" ", Style::default().fg(theme.text_muted)))
        } else {
            Line::from(Span::styled(
                value.to_string(),
                Style::default().fg(theme.text),
            ))
        }),
        inner,
    );
}

fn field_block_style(theme: &Theme, active: bool) -> Style {
    if active {
        Style::default().fg(theme.primary)
    } else {
        Style::default().fg(theme.border)
    }
}

fn set_line_cursor(frame: &mut Frame, area: Rect, label: &str, value_width: u16) {
    let x = area.x.saturating_add(label.len() as u16).saturating_add(4);
    let y = area.y.saturating_add(1);
    frame.set_cursor(x.saturating_add(value_width), y);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    super::centered_rect(width, height, area)
}

fn prev_char_boundary(input: &str, cursor_position: usize) -> Option<usize> {
    if cursor_position == 0 || cursor_position > input.len() {
        return None;
    }
    input[..cursor_position]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
}

fn next_char_boundary(input: &str, cursor_position: usize) -> Option<usize> {
    if cursor_position >= input.len() {
        return None;
    }
    let suffix = &input[cursor_position..];
    suffix
        .chars()
        .next()
        .map(|ch| cursor_position + ch.len_utf8())
}

fn line_start(input: &str, cursor_position: usize) -> usize {
    input[..cursor_position.min(input.len())]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

fn line_end(input: &str, cursor_position: usize) -> usize {
    let cursor_position = cursor_position.min(input.len());
    input[cursor_position..]
        .find('\n')
        .map(|idx| cursor_position + idx)
        .unwrap_or(input.len())
}
