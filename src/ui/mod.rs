mod screens;
mod theme;
mod widgets;

pub use theme::{next_theme, Theme};

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Tab, ToastKind};
use crate::ui::widgets::{dim, key_hints};

const WORDMARK: &str = "ELDENCODE";

pub fn draw(app: &App, f: &mut Frame) {
    let theme = app.theme;
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(theme.bg)), area);

    if area.width < 52 || area.height < 12 {
        let lines = vec![
            Line::from(Span::styled(
                " ELDENCODE ",
                Style::default()
                    .fg(theme.bg)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(dim(
                format!(
                    " terminal too small ({}×{}) — needs at least 52×12",
                    area.width, area.height
                ),
                &theme,
            )),
        ];
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    header(app, f, rows[0]);
    match app.tab {
        Tab::Overview => screens::overview(app, f, rows[1]),
        Tab::Runes => screens::runes(app, f, rows[1]),
        Tab::Attributes => screens::fields(
            app,
            f,
            rows[1],
            "attributes",
            "saved character record · written on rest and quit",
        ),
        Tab::Vitals => screens::fields(
            app,
            f,
            rows[1],
            "vitals",
            "live values · max caps also readable/writable",
        ),
        Tab::Flags => screens::fields(
            app,
            f,
            rows[1],
            "flags",
            "ChrDbg flags · enter toggles, values are re-read live",
        ),
        Tab::Items => screens::items(app, f, rows[1]),
        Tab::Process => screens::process(app, f, rows[1]),
        Tab::Help => screens::help(app, f, rows[1]),
    }
    footer(app, f, rows[2], rows[3]);

    if app.toast.is_some() {
        toast(app, f, area);
    }
}

fn header(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(wordmark(&theme))
        .style(Style::default().fg(theme.text));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let status = format!(
        "{} · {}",
        if app.attached() {
            "attached"
        } else {
            "detached"
        },
        app.mode_label()
    );
    // " ● " + status + " " so the badge and the pid are never clipped.
    let right_w = (status.chars().count() as u16 + 4)
        .min((inner.width / 2).max(10))
        .min(inner.width);
    let cols = Layout::horizontal([Constraint::Min(1), Constraint::Length(right_w)]).split(inner);
    let tabs_w = cols[0].width as usize;

    let active = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let full: Vec<String> = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            if i == active {
                format!("▸{} {} ", i + 1, tab.title())
            } else {
                format!(" {} {} ", i + 1, tab.title())
            }
        })
        .collect();
    let mut spans: Vec<Span> = Vec::new();
    let full_w: usize = full.iter().map(|item| item.chars().count()).sum();
    if full_w <= tabs_w {
        for (i, item) in full.into_iter().enumerate() {
            let style = if i == active {
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.grad(0.35))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.muted)
            };
            spans.push(Span::styled(item, style));
        }
    } else {
        let style = Style::default()
            .fg(Color::Black)
            .bg(theme.grad(0.35))
            .add_modifier(Modifier::BOLD);
        spans.push(Span::styled(
            format!(" {} {}  ", active + 1, Tab::ALL[active].title()),
            style,
        ));
        spans.push(dim(format!("{} screens", Tab::ALL.len()), &theme));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), cols[0]);

    let color = if app.attached() {
        theme.good
    } else {
        theme.muted
    };
    let right = Line::from(vec![
        Span::styled(
            format!(" {} ", if app.attached() { "●" } else { "○" }),
            Style::default().fg(color),
        ),
        Span::styled(
            status,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), cols[1]);
}

fn wordmark(theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::styled(" ", Style::default()));
    let count = WORDMARK.chars().count();
    for (i, ch) in WORDMARK.chars().enumerate() {
        let t = i as f32 / (count.max(2) - 1) as f32;
        spans.push(Span::styled(
            ch.to_string(),
            Style::default()
                .fg(theme.grad(t))
                .add_modifier(Modifier::BOLD),
        ));
        if i + 1 < count {
            spans.push(Span::styled(" ", Style::default().fg(theme.border)));
        }
    }
    spans.push(Span::styled(
        "  elden ring mod menu",
        Style::default().fg(theme.muted),
    ));
    spans.push(Span::styled(" ", Style::default()));
    Line::from(spans)
}

fn footer(app: &App, f: &mut Frame, message_area: Rect, keys_area: Rect) {
    let theme = app.theme;
    let message = if let Some(toast) = &app.toast {
        let color = match toast.kind {
            ToastKind::Good => theme.good,
            ToastKind::Warn => theme.warn,
            ToastKind::Bad => theme.bad,
            ToastKind::Info => theme.accent2,
        };
        Line::from(vec![
            Span::styled(" ● ", Style::default().fg(color)),
            Span::styled(
                toast.text.clone(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(" config ", Style::default().fg(theme.border)),
            Span::styled(
                crate::config::display(&app.paths.config()),
                Style::default().fg(theme.muted),
            ),
            Span::styled("   ·   offsets ", Style::default().fg(theme.border)),
            Span::styled(
                crate::offsets::TABLE.to_string(),
                Style::default().fg(theme.muted),
            ),
        ])
    };
    f.render_widget(Paragraph::new(message), message_area);

    let mut spans: Vec<Span> = Vec::new();
    for (key, label) in key_hints(app) {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::default()
                .fg(Color::Black)
                .bg(theme.border)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label} "),
            Style::default().fg(theme.muted),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), keys_area);
}

fn toast(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let Some(toast) = &app.toast else {
        return;
    };
    let color = match toast.kind {
        ToastKind::Good => theme.good,
        ToastKind::Warn => theme.warn,
        ToastKind::Bad => theme.bad,
        ToastKind::Info => theme.accent2,
    };
    let width = (toast.text.chars().count() as u16 + 6).min(area.width.saturating_sub(4));
    let rect = Rect::new(
        area.x + area.width.saturating_sub(width + 2),
        area.y + area.height.saturating_sub(7),
        width,
        3,
    );
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .style(Style::default().bg(theme.panel));
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}", toast.text),
            Style::default().fg(color),
        )))
        .block(block),
        rect,
    );
}
