use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use crate::app::{App, Tab};
use crate::ui::Theme;

pub fn panel(title: &str, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(theme.muted),
        ))
        .style(Style::default().bg(theme.panel))
}

/// Fill a panel with pre-built lines, clipped to its inner height.
pub fn lines_in(
    f: &mut Frame,
    area: Rect,
    block: Block<'static>,
    lines: Vec<Line<'static>>,
    theme: &Theme,
    scroll: usize,
) {
    let inner_h = area.height.saturating_sub(2) as usize;
    let visible: Vec<Line<'static>> = lines
        .into_iter()
        .skip(scroll)
        .take(inner_h.max(1))
        .collect();
    f.render_widget(
        Paragraph::new(visible)
            .block(block)
            .style(Style::default().fg(theme.text).bg(theme.panel)),
        area,
    );
}

pub fn bar(fraction: f32, width: usize, theme: &Theme) -> Span<'static> {
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * width as f32).round() as usize;
    let color = if fraction <= 0.25 {
        theme.bad
    } else if fraction <= 0.5 {
        theme.warn
    } else {
        theme.accent2
    };
    Span::styled(
        format!(
            "{}{}",
            "█".repeat(filled.min(width)),
            "░".repeat(width.saturating_sub(filled))
        ),
        Style::default().fg(color),
    )
}

pub fn label(text: &str) -> Span<'static> {
    Span::styled(
        format!("{text:<22}"),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

pub fn dim(text: impl Into<String>, theme: &Theme) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(theme.muted))
}

pub fn good(text: impl Into<String>, theme: &Theme) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default().fg(theme.good).add_modifier(Modifier::BOLD),
    )
}

pub fn bad(text: impl Into<String>, theme: &Theme) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(theme.bad))
}

pub fn warn(text: impl Into<String>, theme: &Theme) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(theme.warn))
}

pub fn accent(text: impl Into<String>, theme: &Theme) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    )
}

pub fn key_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.tab {
        Tab::Overview => vec![
            ("F5", "attach"),
            ("b", "backup save"),
            ("2", "runes"),
            ("1-8", "screens"),
            ("q", "quit"),
        ],
        Tab::Runes => vec![
            ("↑↓", "move"),
            ("enter", "edit"),
            ("a", "+1M runes"),
            ("m", "max runes"),
            ("b", "backup"),
        ],
        Tab::Attributes => vec![
            ("↑↓", "move"),
            ("enter", "edit"),
            ("r", "refresh"),
            ("t", "theme"),
            ("q", "quit"),
        ],
        Tab::Vitals => vec![
            ("↑↓", "move"),
            ("enter", "edit"),
            ("r", "refresh"),
            ("q", "quit"),
        ],
        Tab::Flags => vec![
            ("↑↓", "move"),
            ("enter", "toggle"),
            ("r", "refresh"),
            ("q", "quit"),
        ],
        Tab::Items => vec![
            ("↑↓", "move"),
            ("←→", "category"),
            ("s", "slot"),
            ("/", "search"),
            ("enter", "equip"),
        ],
        Tab::Process => vec![
            ("enter", "re-attach"),
            ("d", "detach"),
            ("r", "rescan"),
            ("b", "backup"),
        ],
        Tab::Help => vec![("↑↓", "scroll"), ("1-8", "screens"), ("q", "quit")],
    }
}

pub fn cursor_at(buf: &str, cursor: usize) -> String {
    let mut out = String::new();
    for (i, ch) in buf.char_indices() {
        if i == cursor {
            out.push('▏');
        }
        out.push(ch);
    }
    if cursor >= buf.len() {
        out.push('▏');
    }
    out
}
