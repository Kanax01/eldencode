use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::app::{screen_fields, App, EditKind};
use crate::config;
use crate::items::Cat;
use crate::offsets;
use crate::ui::widgets::{self, accent, bad, bar, dim, good, label, panel, warn};
use crate::ui::Theme;

pub fn overview(app: &App, f: &mut Frame, area: Rect) {
    let wide = area.width >= 96 && area.height >= 20;
    if wide {
        let cols = Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)])
            .split(area);
        session(app, f, cols[0]);
        tarnished(app, f, cols[1]);
    } else {
        let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(9)]).split(area);
        session(app, f, rows[0]);
        tarnished(app, f, rows[1]);
    }
}

fn session(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let mut lines: Vec<Line<'static>> = Vec::new();
    match &app.game {
        Some(game) => {
            lines.push(Line::from(vec![
                Span::styled("  ● ", Style::default().fg(theme.good)),
                good("attached", &theme),
                dim(format!("  {}", game.proc.comm), &theme),
            ]));
            lines.push(Line::from(vec![
                label("Process id"),
                Span::raw(game.proc.pid.to_string()),
            ]));
            lines.push(Line::from(vec![
                label("Image base"),
                Span::raw(game.proc.base_hex()),
                dim(format!("   {:.1} MiB image", game.proc.size_mb()), &theme),
            ]));
            lines.push(Line::from(vec![
                label("Scanned"),
                Span::raw(format!("{:.1} MiB", game.scanned_mb)),
                dim(format!("   {} readable regions", game.regions), &theme),
            ]));
            let resolved = offsets::SYMBOLS.len() - game.failed_symbols.len();
            lines.push(Line::from(vec![
                label("Symbols"),
                if game.failed_symbols.is_empty() {
                    good(
                        format!("{resolved}/{} resolved", offsets::SYMBOLS.len()),
                        &theme,
                    )
                } else {
                    warn(
                        format!(
                            "{resolved}/{} resolved (missing {})",
                            offsets::SYMBOLS.len(),
                            game.failed_symbols.join(", ")
                        ),
                        &theme,
                    )
                },
            ]));
            lines.push(Line::from(vec![
                label("Proton prefix"),
                match &game.proc.prefix {
                    Some(prefix) => Span::raw(config::display(prefix)),
                    None => dim("not detected", &theme),
                },
            ]));
            if let Some(image) = &game.proc.image {
                lines.push(Line::from(vec![
                    label("Executable"),
                    dim(config::display(image), &theme),
                ]));
            }
            if let Some(error) = &app.cache_error {
                lines.push(Line::from(bad(format!("  read: {error}"), &theme)));
            }
        }
        None => {
            lines.push(Line::from(vec![
                Span::styled("  ● ", Style::default().fg(theme.muted)),
                dim("detached", &theme),
            ]));
            lines.push(Line::from(vec![
                label("Looking for"),
                Span::raw(app.cfg.process.clone()),
            ]));
            lines.push(Line::from(vec![
                label("Matches"),
                Span::raw(app.found.len().to_string()),
            ]));
            if let Some(error) = &app.attach_error {
                for chunk in wrap(error, area.width.saturating_sub(6) as usize) {
                    lines.push(Line::from(bad(format!("  {chunk}"), &theme)));
                }
            }
            if let Some(hint) = &app.attach_hint {
                for chunk in wrap(hint, area.width.saturating_sub(6) as usize) {
                    lines.push(Line::from(warn(format!("  {chunk}"), &theme)));
                }
            }
            if app.found.is_empty() {
                lines.push(Line::from(dim(
                    "  start Elden Ring through Steam with EAC disabled, then F5",
                    &theme,
                )));
            } else {
                lines.push(Line::from(accent(
                    "  a process matched — press F5 to attach",
                    &theme,
                )));
            }
            for (i, proc) in app.found.iter().take(3).enumerate() {
                lines.push(Line::from(dim(
                    format!("  [{}] {} · pid {}", i + 1, proc.comm, proc.pid),
                    &theme,
                )));
            }
        }
    }
    if app.eac {
        lines.push(Line::from(vec![
            Span::styled("  ▲ ", Style::default().fg(theme.bad)),
            bad(
                "EasyAntiCheat is running — memory edits will not stick",
                &theme,
            ),
        ]));
    }
    if let Some(note) = &app.backup_note {
        lines.push(Line::from(dim(format!("  save copy: {note}"), &theme)));
    }
    lines.push(Line::from(dim(
        format!("  offsets: built for {}", offsets::TABLE),
        &theme,
    )));
    widgets::lines_in(f, area, panel("session", &theme), lines, &theme, 0);
}

fn tarnished(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let snap = &app.snapshot;
    let mut lines: Vec<Line<'static>> = Vec::new();
    let num = |value: Option<u32>| match value {
        Some(v) => v.to_string(),
        None => "—".to_string(),
    };
    lines.push(Line::from(vec![
        label("Name"),
        Span::raw(snap.name.clone().unwrap_or_else(|| "—".into())),
    ]));
    lines.push(Line::from(vec![
        label("Level"),
        accent(num(snap.level), &theme),
        dim(format!("   runes {}", num(snap.runes)), &theme),
    ]));
    if let (Some(hp), Some(max)) = (snap.hp, snap.max_hp) {
        let fraction = if max == 0 {
            0.0
        } else {
            hp as f32 / max as f32
        };
        lines.push(Line::from(vec![
            Span::raw("HP       "),
            bar(fraction, 20, &theme),
            dim(format!("  {hp}/{max}"), &theme),
        ]));
    }
    if let (Some(fp), Some(max)) = (snap.fp, snap.max_fp) {
        let fraction = if max == 0 {
            0.0
        } else {
            fp as f32 / max as f32
        };
        lines.push(Line::from(vec![
            Span::raw("FP       "),
            bar(fraction, 20, &theme),
            dim(format!("  {fp}/{max}"), &theme),
        ]));
    }
    if let (Some(st), Some(max)) = (snap.stamina, snap.max_stamina) {
        let fraction = if max == 0 {
            0.0
        } else {
            st as f32 / max as f32
        };
        lines.push(Line::from(vec![
            Span::raw("Stamina  "),
            bar(fraction, 20, &theme),
            dim(format!("  {st}/{max}"), &theme),
        ]));
    }
    lines.push(Line::from(vec![
        label("Journey"),
        Span::raw(format!("NG+{}", num(snap.ng))),
        dim(
            format!("   lifetime runes {}", num(snap.total_runes)),
            &theme,
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(dim(
        "  2 runes · 3 attributes · 6 items · b backup",
        &theme,
    )));
    widgets::lines_in(f, area, panel("tarnished", &theme), lines, &theme, 0);
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    if width < 10 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > width {
            if !current.is_empty() {
                out.push(current.clone());
                current.clear();
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

pub fn fields(app: &App, f: &mut Frame, area: Rect, title: &str, subtitle: &str) {
    let theme = app.theme;
    let fields = screen_fields(app.tab);
    let inner_h = area.height.saturating_sub(3) as usize;
    let scroll = if inner_h == 0 || app.focus < inner_h {
        0
    } else {
        app.focus + 1 - inner_h
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(dim(format!("  {subtitle}"), &theme)));
    for (i, field) in fields.iter().enumerate() {
        let selected = i == app.focus;
        let marker = if selected { "▸ " } else { "  " };
        let label_style = if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::BOLD)
        };
        let editing = matches!(&app.edit, Some(edit) if edit.kind == EditKind::Field(field.key));
        let value_span = if editing {
            let buf = app.edit.as_ref().map(|e| e.buf.clone()).unwrap_or_default();
            let cursor = app.edit.as_ref().map(|e| e.cursor).unwrap_or(buf.len());
            Span::styled(
                format!("{:<14}", widgets::cursor_at(&buf, cursor)),
                Style::default().fg(theme.accent2),
            )
        } else if app.tab == crate::app::Tab::Flags {
            if app.flag_on(field) {
                Span::styled(format!("{:<14}", "ON"), Style::default().fg(theme.good))
            } else {
                Span::styled(format!("{:<14}", "off"), Style::default().fg(theme.muted))
            }
        } else {
            Span::styled(
                format!("{:<14}", app.value_of(field)),
                Style::default().fg(theme.text),
            )
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
            Span::styled(format!("{:<20}", field.label), label_style),
            value_span,
        ];
        if !field.note.is_empty() {
            spans.push(dim(field.note, &theme));
        }
        lines.push(Line::from(spans));
    }
    widgets::lines_in(f, area, panel(title, &theme), lines, &theme, scroll);
}

pub fn runes(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let rows = Layout::vertical([Constraint::Min(7), Constraint::Length(6)]).split(area);
    fields(
        app,
        f,
        rows[0],
        "runes",
        "Held runes are written straight into the character record",
    );
    let lines = vec![
        Line::from(vec![
            Span::raw("  "),
            accent("a", &theme),
            dim("  add 1,000,000 runes", &theme),
        ]),
        Line::from(vec![
            Span::raw("  "),
            accent("m", &theme),
            dim("  set 999,999,999 runes", &theme),
        ]),
        Line::from(vec![
            Span::raw("  "),
            accent("enter", &theme),
            dim("  edit the selected value", &theme),
        ]),
        Line::from(dim(
            "  stays in memory only — rest and quit to write it to the save",
            &theme,
        )),
    ];
    widgets::lines_in(f, rows[1], panel("quick", &theme), lines, &theme, 0);
}

/// The upgrade sheet: enter on a weapon picks a level first.
pub fn give_menu(app: &App, f: &mut Frame, area: Rect) {
    let Some(menu) = &app.give_menu else {
        return;
    };
    let theme = app.theme;
    let width = 48.min(area.width.saturating_sub(2));
    let height = 11.min(area.height.saturating_sub(2));
    if width < 34 || height < 9 {
        return;
    }
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, popup);
    let block = panel(&format!("upgrade {}", menu.name.to_lowercase()), &theme);
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let row = |name: &str, value: String, extra: String, active: bool| -> Line<'static> {
        let mut spans = vec![
            if active {
                accent(" › ", &theme)
            } else {
                dim("   ", &theme)
            },
            if active {
                Span::styled(
                    format!("{name:<9}"),
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                dim(format!("{name:<9}"), &theme)
            },
            Span::raw(value),
        ];
        if !extra.is_empty() {
            spans.push(dim(format!("  {extra}"), &theme));
        }
        Line::from(spans)
    };

    let level = if menu.max_level == 0 {
        "fixed".to_string()
    } else {
        format!("+{}", menu.level)
    };
    let level_note = if menu.max_level == 0 {
        String::new()
    } else {
        format!("max +{}", menu.max_level)
    };
    let gib = crate::items::gib_id(menu.cat, menu.id(), menu.level);
    let variant = app
        .catalog
        .name_of(menu.id(), menu.cat)
        .unwrap_or_else(|| menu.name.clone());

    let lines = vec![
        Line::from(""),
        row("level", format!("‹ {level} ›"), level_note, true),
        row(
            "amount",
            format!("×{}", menu.qty),
            "from the items screen".into(),
            false,
        ),
        Line::from(""),
        Line::from(vec![
            dim("   gives ", &theme),
            good(format!("{variant} {level}"), &theme),
            dim(format!("  · {gib:#010X}"), &theme),
        ]),
        Line::from(""),
        Line::from(vec![dim(
            "   ←→ level · enter give · esc cancel",
            &theme,
        )]),
    ];
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme.panel)),
        inner,
    );
}

pub fn items(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let block = panel("items", &theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 4 || inner.width < 24 {
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(2),
    ])
    .split(inner);

    // Sub-tabs, one per item type; left/right switches them.
    let mut tabs: Vec<Span> = vec![Span::raw(" ")];
    for (i, kind) in Cat::ALL.iter().enumerate() {
        let active = i == app.items.cat;
        let label = format!(" {} {} ", kind.label(), app.catalog.count(*kind));
        if active {
            tabs.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::Black)
                    .bg(theme.grad(0.35))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            tabs.push(Span::styled(label, Style::default().fg(theme.muted)));
        }
        tabs.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(tabs)), rows[0]);

    let slot = app.current_slot();
    let max_level = app.selected_max_level();
    let level = app.items.level.min(max_level);
    let level_text = if max_level == 0 {
        "—".to_string()
    } else {
        format!("+{level} / +{max_level}")
    };
    let search = if matches!(&app.edit, Some(e) if e.kind == EditKind::Search) {
        let buf = app.edit.as_ref().map(|e| e.buf.clone()).unwrap_or_default();
        let cursor = app.edit.as_ref().map(|e| e.cursor).unwrap_or(buf.len());
        Span::styled(
            widgets::cursor_at(&buf, cursor),
            Style::default().fg(theme.accent2),
        )
    } else if app.items.query.is_empty() {
        dim("(/ to filter)", &theme)
    } else {
        Span::styled(app.items.query.clone(), Style::default().fg(theme.accent2))
    };
    let controls = Line::from(vec![
        Span::raw("  "),
        dim("level ", &theme),
        Span::styled(level_text, Style::default().fg(theme.accent2)),
        dim("   qty ", &theme),
        Span::styled(
            app.items.qty.to_string(),
            Style::default().fg(theme.accent2),
        ),
        dim("   equip ", &theme),
        Span::styled(slot.label.to_string(), Style::default().fg(theme.accent)),
        dim(format!(" → {}", app.slot_value()), &theme),
        dim("   ", &theme),
        search,
    ]);
    f.render_widget(Paragraph::new(controls), rows[1]);

    let height = rows[2].height as usize;
    let sel = app
        .items
        .sel
        .min(app.items.filtered.len().saturating_sub(1));
    let scroll = if height == 0 || sel < height {
        0
    } else {
        sel + 1 - height
    };
    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.items.filtered.is_empty() {
        lines.push(Line::from(dim("  no item matches that filter", &theme)));
    }
    for (row, idx) in app
        .items
        .filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(height.max(1))
    {
        let item = &app.catalog.items[*idx];
        let selected = row == sel;
        let marker = if selected { "▸ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let mut name = item.name.to_string();
        let cap = app.catalog.max_level(item.id);
        if cap > 0 {
            let level = app.items.level.min(cap);
            if level > 0 {
                name.push_str(&format!(" +{level}"));
            }
        }
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
            Span::styled(format!("{:>9}", item.id), Style::default().fg(theme.muted)),
            Span::raw("  "),
            Span::styled(name, name_style),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme.text).bg(theme.panel)),
        rows[2],
    );
    give_menu(app, f, area);
}

pub fn process(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let rows = Layout::vertical([Constraint::Min(8), Constraint::Length(8)]).split(area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        label("Looking for"),
        Span::raw(app.cfg.process.clone()),
    ]));
    lines.push(Line::from(vec![
        label("Matches"),
        Span::raw(app.found.len().to_string()),
    ]));
    for proc in &app.found {
        lines.push(Line::from(vec![
            Span::raw("  "),
            accent(proc.comm.clone(), &theme),
            dim(format!("  pid {}", proc.pid), &theme),
            dim(format!("  base {}", proc.base_hex()), &theme),
            dim(format!("  {:.1} MiB", proc.size_mb()), &theme),
        ]));
        lines.push(Line::from(dim(
            format!("      {}", truncate(&proc.cmdline, 120)),
            &theme,
        )));
        if let Some(prefix) = &proc.prefix {
            lines.push(Line::from(dim(
                format!("      prefix {}", config::display(prefix)),
                &theme,
            )));
        }
    }
    if app.found.is_empty() {
        lines.push(Line::from(dim(
            "  no process matched — is the game running?",
            &theme,
        )));
    }
    if let Some(error) = &app.attach_error {
        for chunk in wrap(error, area.width.saturating_sub(8) as usize) {
            lines.push(Line::from(bad(format!("  {chunk}"), &theme)));
        }
    }
    if let Some(hint) = &app.attach_hint {
        for chunk in wrap(hint, area.width.saturating_sub(8) as usize) {
            lines.push(Line::from(warn(format!("  {chunk}"), &theme)));
        }
    }
    widgets::lines_in(f, rows[0], panel("processes", &theme), lines, &theme, 0);

    let mut notes = vec![
        Line::from(vec![
            Span::raw("  "),
            accent("enter", &theme),
            dim("  scan again and attach to the first match", &theme),
        ]),
        Line::from(vec![
            Span::raw("  "),
            accent("d", &theme),
            dim("  detach (no writes afterwards)", &theme),
        ]),
        Line::from(vec![
            Span::raw("  "),
            accent("b", &theme),
            dim("  copy ER0000.sl2 into ", &theme),
            dim(config::display(&app.paths.backups()), &theme),
        ]),
        Line::from(dim(
            "  Proton: the Windows exe runs as a Linux process, so /proc is enough.",
            &theme,
        )),
        Line::from(dim(
            "  If attach fails with EPERM, see Help → permissions.",
            &theme,
        )),
    ];
    if let Some(note) = &app.backup_note {
        notes.push(Line::from(good(format!("  last backup: {note}"), &theme)));
    }
    widgets::lines_in(f, rows[1], panel("actions", &theme), notes, &theme, 0);
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

pub fn scripts(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let block = panel("scripts", &theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 4 || inner.width < 24 {
        return;
    }
    let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(inner);

    let dlc = if app.scripts.include_dlc {
        Span::styled(
            " dlc items on ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.warn)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        dim(" dlc items off (d) ", &theme)
    };
    let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
        Span::raw("  "),
        dim(
            format!(
                "{} bulk actions · one item per call ·",
                crate::scripts::SCRIPTS.len()
            ),
            &theme,
        ),
        dlc,
    ])];
    for (i, script) in crate::scripts::SCRIPTS.iter().enumerate() {
        let picked = i == app.scripts.sel;
        let marker = if picked { "▸ " } else { "  " };
        let name_style = if picked {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        let count = app.script_counts.get(i).copied().unwrap_or(0);
        let label = match script.kind {
            crate::scripts::Kind::Items { .. } => format!("{count:>4} items"),
            _ => "event flags".to_string(),
        };
        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), Style::default().fg(theme.accent)),
            Span::styled(format!("{:<36}", script.name), name_style),
            Span::styled(label, Style::default().fg(theme.muted)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), rows[0]);

    let sel = app.scripts.sel.min(crate::scripts::SCRIPTS.len() - 1);
    let script = &crate::scripts::SCRIPTS[sel];
    let mut detail: Vec<Line<'static>> = vec![Line::from(vec![
        Span::raw("  "),
        Span::styled(script.blurb.to_string(), Style::default().fg(theme.text)),
    ])];
    if let Some(progress) = &app.scripts.progress {
        let width = 36usize.min(rows[1].width.saturating_sub(24) as usize);
        let filled = if progress.total == 0 {
            0
        } else {
            (progress.done * width / progress.total).min(width)
        };
        detail.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:>4}/{:<4} ", progress.done, progress.total),
                Style::default().fg(theme.accent),
            ),
            Span::styled("█".repeat(filled), Style::default().fg(theme.good)),
            Span::styled("░".repeat(width - filled), Style::default().fg(theme.border)),
            dim(format!("  {}", progress.name), &theme),
        ]));
    } else if app.scripts.pending == Some(app.scripts.sel) {
        detail.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "press enter again to run · esc cancels",
                Style::default()
                    .fg(theme.warn)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else if let Some((text, ok)) = &app.scripts.last {
        detail.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                text.clone(),
                Style::default().fg(if *ok { theme.good } else { theme.warn }),
            ),
        ]));
    } else {
        detail.push(Line::from(vec![
            Span::raw("  "),
            dim("enter runs the highlighted script", &theme),
        ]));
    }
    f.render_widget(Paragraph::new(detail), rows[1]);
}

pub fn help(app: &App, f: &mut Frame, area: Rect) {
    let theme = app.theme;
    let lines = help_lines(&theme);
    widgets::lines_in(
        f,
        area,
        panel("help", &theme),
        lines,
        &theme,
        app.help_scroll,
    );
}

fn help_lines(theme: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let title = |text: &str| {
        Line::from(Span::styled(
            format!("  {text}"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
    };
    let body = |text: &str| Line::from(dim(format!("  {text}"), theme));

    out.push(title("What this is"));
    out.push(body(
        "A terminal mod menu for Elden Ring. It finds the game's base pointers by AOB",
    ));
    out.push(body(
        "scanning the running image, walks their pointer chains, and writes the",
    ));
    out.push(body(
        "character record directly: runes, attributes, HP/FP/SP, debug flags,",
    ));
    out.push(body("equipment slots and inventory items."));
    out.push(Line::from(""));
    out.push(title("Proton and Linux"));
    out.push(body(
        "Elden Ring on Linux runs as a Wine process: eldenring.exe is an ordinary Linux",
    ));
    out.push(body(
        "process. eldencode finds it through /proc, takes the image base from",
    ));
    out.push(body(
        "/proc/<pid>/maps and reads/writes memory with process_vm_readv/writev.",
    ));
    out.push(body(
        "No Windows API, no kernel module, no Cheat Engine. The prefix is read from",
    ));
    out.push(body(
        "STEAM_COMPAT_DATA_PATH so saves can be backed up before you edit anything.",
    ));
    out.push(Line::from(""));
    out.push(title("Permissions"));
    out.push(body(
        "If attach fails with a permission error, the kernel is refusing cross-process",
    ));
    out.push(body("access (yama). Either relax it for the session:"));
    out.push(Line::from(Span::styled(
        "      sudo sysctl -w kernel.yama.ptrace_scope=0",
        Style::default().fg(theme.accent2),
    )));
    out.push(body("or run eldencode itself with sudo:"));
    out.push(Line::from(Span::styled(
        "      sudo -E eldencode",
        Style::default().fg(theme.accent2),
    )));
    out.push(Line::from(""));
    out.push(title("Before you edit"));
    out.push(body(
        "1. Disable EasyAntiCheat (Steam → Elden Ring → Properties → Launch Options:",
    ));
    out.push(Line::from(Span::styled(
        "      start_protected_game.exe is replaced / EAC toggled off per your usual method",
        Style::default().fg(theme.accent2),
    )));
    out.push(body("2. Back up your save: press b on any screen."));
    out.push(body(
        "3. Play offline. Edited saves and online play do not mix.",
    ));
    out.push(Line::from(""));
    out.push(title("Screens"));
    out.push(body(
        "1 Overview      attach state, image base, character snapshot",
    ));
    out.push(body(
        "2 Runes         runes held / earned, rune arc, quick add",
    ));
    out.push(body(
        "3 Attributes    level, vigor..arcane, scadutree, spirit ash",
    ));
    out.push(body(
        "4 Vitals        live HP / FP / stamina and their caps",
    ));
    out.push(body(
        "5 Flags         ChrDbg flags (no damage, no FP consume, ...)",
    ));
    out.push(body(
        "6 Items         give any item (with level and quantity) or equip it directly",
    ));
    out.push(body(
        "7 Scripts       bulk actions: all weapons, armour, spells, materials, runes",
    ));
    out.push(body(
        "8 Process       detected processes, prefix, backup, diagnostics",
    ));
    out.push(body("9 Help          this page"));
    out.push(Line::from(""));
    out.push(title("Keys"));
    out.push(body(
        "1-9 / tab / shift-tab  switch screens      F5  attach or re-scan",
    ));
    out.push(body(
        "↑↓ / k j               move               enter  edit or toggle",
    ));
    out.push(body(
        "a m (Runes)            add 1M / max        b  back up save",
    ));
    out.push(body(
        "t                      theme               r  refresh values",
    ));
    out.push(body(
        "q / ctrl-c             quit                /  search items",
    ));
    out.push(Line::from(""));
    out.push(title("Bulk scripts"));
    out.push(body(
        "The Scripts screen runs the give-everything actions: all weapons (plain or",
    ));
    out.push(body(
        "maxed), all armour, all talismans (every variant, or one strongest per",
    ));
    out.push(body(
        "family), all sorceries, all incantations, all ashes of war, every crafting",
    ));
    out.push(body(
        "material, arrow, consumable, reusable and upgrade material, every map,",
    ));
    out.push(body(
        "cookbook and whetblade, and a stack of every rune item. Two more scripts set",
    ));
    out.push(body(
        "event flags through the game's own setter: unlock all graces and activate all",
    ));
    out.push(body(
        "summoning pools. enter twice starts, esc stops, q quits. Long runs take a few",
    ));
    out.push(body(
        "seconds because each item is one call into the game. DLC items are skipped",
    ));
    out.push(body(
        "unless you press d on that screen: DLC ids are not in the base game's params.",
    ));
    out.push(Line::from(""));
    out.push(title("Giving items"));
    out.push(body(
        "enter in the Items screen calls the game's own item routine — the one it uses",
    ));
    out.push(body(
        "when it hands you an item. The id carries the category nibble and the",
    ));
    out.push(body(
        "the upgrade level picked with -/+, the quantity comes from ,/. . On a weapon",
    ));
    out.push(body(
        "enter opens an upgrade sheet: pick a level, then enter gives it. Somber",
    ));
    out.push(body(
        "weapons stop at +10 and the rest at +25, and only items stack: 99 of a",
    ));
    out.push(body(
        "consumable, one of anything else. Only the plain item is given — no infused",
    ));
    out.push(body(
        "variants, because the game creates those from an ash of war, not from an id.",
    ));
    out.push(body("Linux has"));
    out.push(body(
        "no CreateRemoteThread, so eldencode borrows the game's main thread with",
    ));
    out.push(body(
        "ptrace for that single call, restores its registers, then continues. The",
    ));
    out.push(body(
        "item lands in your inventory; e instead equips it into a ChrAsm slot.",
    ));
    out.push(Line::from(""));
    out.push(title("Limits, honestly"));
    out.push(body(
        "Offsets follow game 1.17.1 — after a patch the AOB scan can miss and the",
    ));
    out.push(body(
        "header says which symbols failed. Upgrade levels are capped at what the",
    ));
    out.push(body(
        "catalog knows (+25 smithing weapons, +10 somber); weapon ids already carry",
    ));
    out.push(body(
        "their level in the last two digits, so +N is added to the base id.",
    ));
    out
}
