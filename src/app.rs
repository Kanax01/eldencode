use std::collections::HashMap;
use std::io::Stdout;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::config::{Config, Paths};
use crate::game::{Game, Snapshot, Value};
use crate::items::{Cat, Catalog};
use crate::offsets::{self, Field, Kind};
use crate::process;
use crate::scripts::{Give, Kind as ScriptKind, Library, SCRIPTS};
use crate::ui;

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Overview,
    Runes,
    Attributes,
    Vitals,
    Flags,
    Items,
    Scripts,
    Process,
    Help,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::Overview,
        Tab::Runes,
        Tab::Attributes,
        Tab::Vitals,
        Tab::Flags,
        Tab::Items,
        Tab::Scripts,
        Tab::Process,
        Tab::Help,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Runes => "Runes",
            Tab::Attributes => "Attributes",
            Tab::Vitals => "Vitals",
            Tab::Flags => "Flags",
            Tab::Items => "Items",
            Tab::Scripts => "Scripts",
            Tab::Process => "Process",
            Tab::Help => "Help",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }

    pub fn rows(self) -> usize {
        match self {
            Tab::Runes => offsets::RUNES.len(),
            Tab::Attributes => offsets::ATTRIBUTES.len(),
            Tab::Vitals => offsets::VITALS.len(),
            Tab::Flags => offsets::FLAGS.len() + offsets::PLAYER_FLAGS.len(),
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Good,
    Warn,
    Bad,
    Info,
}

pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    Field(&'static str),
    Search,
}

pub struct Edit {
    pub kind: EditKind,
    pub buf: String,
    pub cursor: usize,
}

pub struct GiveMenu {

    pub family: u32,
    pub name: String,
    pub level: u32,
    pub max_level: u32,
    pub qty: u32,
    pub cat: Cat,
}

impl GiveMenu {

    pub fn id(&self) -> u32 {
        self.family
    }
}

pub struct ItemsView {
    pub cat: usize,
    pub query: String,
    pub sel: usize,
    pub scroll: usize,
    pub slot: usize,

    pub level: u32,
    pub qty: u32,
    pub filtered: Vec<usize>,
}

pub struct ScriptsView {
    pub sel: usize,

    pub include_dlc: bool,

    pub pending: Option<usize>,
    pub progress: Option<Progress>,
    pub last: Option<(String, bool)>,
}

pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub name: String,
}

pub struct App {

    pub give_menu: Option<GiveMenu>,
    pub paths: Paths,
    pub cfg: Config,
    pub theme: ui::Theme,
    pub tab: Tab,
    pub quit: bool,
    pub game: Option<Game>,
    pub attach_error: Option<String>,
    pub attach_hint: Option<String>,
    pub found: Vec<process::GameProcess>,
    pub eac: bool,
    pub catalog: Catalog,
    pub focus: usize,
    pub edit: Option<Edit>,
    pub items: ItemsView,
    pub scripts: ScriptsView,
    pub script_lib: Library,
    pub script_counts: Vec<usize>,
    run_request: Option<usize>,
    pub toast: Option<Toast>,
    pub cache: HashMap<&'static str, Value>,
    pub cache_error: Option<String>,
    pub snapshot: Snapshot,
    pub tick_count: u64,
    last_poll: Instant,
    pub backup_note: Option<String>,
    pub help_scroll: usize,
}

pub fn screen_fields(tab: Tab) -> Vec<&'static Field> {
    match tab {
        Tab::Runes => offsets::RUNES.iter().collect(),
        Tab::Attributes => offsets::ATTRIBUTES.iter().collect(),
        Tab::Vitals => offsets::VITALS.iter().collect(),
        Tab::Flags => offsets::FLAGS
            .iter()
            .chain(offsets::PLAYER_FLAGS.iter())
            .collect(),
        _ => Vec::new(),
    }
}

fn parse_number(buf: &str, kind: Kind) -> Result<u64, String> {
    let text = buf.trim().replace('_', "");
    if text.is_empty() {
        return Err("empty value".into());
    }
    if kind == Kind::F32 {
        return text
            .parse::<f32>()
            .map(|v| v as u64)
            .map_err(|_| format!("'{buf}' is not a number"));
    }
    let parsed = if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16)
    } else {
        text.parse::<u64>()
    };
    parsed.map_err(|_| format!("'{buf}' is not a number"))
}

impl App {
    pub fn new(paths: Paths, cfg: Config) -> App {
        let theme = ui::Theme::by_name(&cfg.theme);
        let catalog = Catalog::load();
        let script_lib = Library::load();
        let script_counts: Vec<usize> = SCRIPTS
            .iter()
            .map(|script| script_lib.resolve(script, &catalog, false).len())
            .collect();
        let mut app = App {
            paths,
            cfg,
            theme,
            tab: Tab::Overview,
            quit: false,
            game: None,
            attach_error: None,
            attach_hint: None,
            found: Vec::new(),
            eac: false,
            catalog,
            focus: 0,
            edit: None,
            items: ItemsView {
                cat: 0,
                query: String::new(),
                sel: 0,
                scroll: 0,
                slot: 0,
                level: 0,
                qty: 1,
                filtered: Vec::new(),
            },
            give_menu: None,
            scripts: ScriptsView {
                sel: 0,
                include_dlc: false,
                pending: None,
                progress: None,
                last: None,
            },
            script_lib,
            script_counts,
            run_request: None,
            toast: None,
            cache: HashMap::new(),
            cache_error: None,
            snapshot: Snapshot::default(),
            tick_count: 0,
            last_poll: Instant::now(),
            backup_note: None,
            help_scroll: 0,
        };
        app.refresh_items();
        if app.cfg.auto_attach {
            app.attach();
        } else {
            app.found = process::scan(&app.cfg.process);
        }
        app
    }

    pub fn run(&mut self, terminal: &mut Tui) -> std::io::Result<()> {
        while !self.quit {
            self.tick();
            terminal.draw(|frame| ui::draw(self, frame))?;
            let timeout = if self.cfg.animations {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(150)
            };
            if event::poll(timeout)? {
                loop {
                    match event::read()? {
                        Event::Key(key)
                            if key.kind == KeyEventKind::Press
                                || key.kind == KeyEventKind::Repeat =>
                        {
                            self.on_key(key)
                        }
                        _ => {}
                    }
                    if self.quit || !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            if let Some(index) = self.run_request.take() {
                self.execute_script(index, terminal)?;
            }
        }
        Ok(())
    }

    pub fn attached(&self) -> bool {
        self.game.is_some()
    }

    pub fn mode_label(&self) -> String {
        match &self.game {
            Some(game) => format!("pid {}", game.proc.pid),
            None => "detached".to_string(),
        }
    }

    pub fn value_of(&self, field: &Field) -> String {
        match self.cache.get(field.key) {
            Some(Value::Name(name)) => name.clone(),
            Some(value) => value.display(),
            None => "—".to_string(),
        }
    }

    pub fn flag_on(&self, field: &Field) -> bool {
        match self.cache.get(field.key) {
            Some(Value::U8(v)) => *v != 0,
            Some(Value::U32(v)) => *v != 0,
            _ => false,
        }
    }

    pub fn slot_cat(&self) -> Cat {
        match self.current_slot().key {
            "helmet" | "chest" | "gauntlets" | "legs" => Cat::Armor,
            "talisman_1" | "talisman_2" | "talisman_3" | "talisman_4" | "talisman_5" => {
                Cat::Talisman
            }
            _ => Cat::Weapon,
        }
    }

    pub fn item_name(&self, id: u32) -> Option<String> {
        self.catalog.name_of(id, self.slot_cat())
    }

    pub fn current_slot(&self) -> &'static Field {
        &offsets::EQUIP[self.items.slot.min(offsets::EQUIP.len() - 1)]
    }

    pub fn slot_value(&self) -> String {
        let field = self.current_slot();
        match self.cache.get(field.key).and_then(|v| v.u32()) {
            Some(0) | Some(offsets::EMPTY_SLOT) => "empty".to_string(),
            Some(id) => match self.item_name(id) {
                Some(name) => format!("{name} ({id})"),
                None if id == offsets::UNARMED => "Unarmed".to_string(),
                None => format!("id {id}"),
            },
            None => "—".to_string(),
        }
    }

    fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
    }

    pub fn refresh_items(&mut self) {
        let cat = Cat::ALL[self.items.cat.min(Cat::ALL.len() - 1)];
        self.items.filtered = self.catalog.filter(cat, &self.items.query);
        if self.items.sel >= self.items.filtered.len() {
            self.items.sel = self.items.filtered.len().saturating_sub(1);
        }
        let max = self.selected_max_level();
        if self.items.level > max {
            self.items.level = max;
        }
    }

    pub fn selected_item(&self) -> Option<&crate::items::Item> {
        let idx = *self.items.filtered.get(self.items.sel)?;
        self.catalog.items.get(idx)
    }

    pub fn selected_max_level(&self) -> u32 {
        self.selected_item()
            .map(|item| self.catalog.max_level(item.id))
            .unwrap_or(0)
    }

    pub fn attach(&mut self) {
        self.eac = process::anti_cheat_running();
        let scanned = process::scan(&self.cfg.process);
        self.found = scanned;
        self.attach_error = None;
        self.attach_hint = None;
        self.game = None;
        self.cache.clear();
        self.snapshot = Snapshot::default();

        let Some(proc) = self.found.first().cloned() else {
            self.attach_error = Some(format!(
                "{} is not running — start Elden Ring (EAC disabled) and press F5",
                self.cfg.process
            ));
            return;
        };
        match Game::attach(proc) {
            Ok(game) => {
                let failed = game.failed_symbols.clone();
                let pid = game.proc.pid;
                self.game = Some(game);
                if !failed.is_empty() {
                    self.attach_error = Some(format!(
                        "{} symbols not found: {}",
                        failed.len(),
                        failed.join(", ")
                    ));
                }
                self.repoll();
                self.toast(format!("attached to pid {pid}"), ToastKind::Good);
                if self.cfg.backup_on_attach {
                    self.backup_save();
                }
            }
            Err(err) => {
                self.attach_error = Some(err.message);
                self.attach_hint = err.hint;
                self.toast("attach failed", ToastKind::Bad);
            }
        }
    }

    pub fn detach(&mut self) {
        self.game = None;
        self.cache.clear();
        self.snapshot = Snapshot::default();
        self.toast("detached", ToastKind::Info);
    }

    pub fn backup_save(&mut self) {
        let Some(game) = &self.game else {
            self.toast("backup needs the game running", ToastKind::Warn);
            return;
        };
        match process::backup_saves(&game.proc, &self.paths.backups()) {
            Ok(files) => {
                let first = files
                    .first()
                    .map(|p| crate::config::display(p))
                    .unwrap_or_default();
                self.backup_note = Some(first.clone());
                self.toast(format!("save backed up → {first}"), ToastKind::Good);
            }
            Err(err) => self.toast(err, ToastKind::Bad),
        }
    }

    fn repoll(&mut self) {
        let Some(game) = &self.game else {
            return;
        };
        let mut cache = HashMap::new();
        let mut error = None;
        for field in offsets::all_fields() {
            match game.read(field) {
                Ok(value) => {
                    cache.insert(field.key, value);
                }
                Err(err) => {
                    if error.is_none() && field.kind != Kind::Name {
                        error = Some(format!("{}: {err}", field.label));
                    }
                }
            }
        }
        self.cache = cache;
        self.cache_error = error;
        self.snapshot = game.snapshot();
    }

    fn tick(&mut self) {
        self.tick_count = self.tick_count.wrapping_add(1);
        if let Some(toast) = &self.toast {
            if toast.born.elapsed().as_secs_f32() > 4.0 {
                self.toast = None;
            }
        }
        if self.last_poll.elapsed() >= Duration::from_millis(350) {
            self.last_poll = Instant::now();
            if self.game.is_some() {
                self.repoll();
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.edit.is_some() {
            self.edit_key(key);
            return;
        }
        if self.give_menu.is_some() {
            self.give_menu_key(key);
            return;
        }
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q')) {
            self.quit = true;
            return;
        }
        if key.code == KeyCode::F(5) {
            self.attach();
            return;
        }

        match key.code {
            KeyCode::Char('q') => {
                self.quit = true;
                return;
            }
            KeyCode::Char('t') => {
                self.theme = ui::Theme::by_name(ui::next_theme(self.theme.name));
                self.cfg.theme = self.theme.name.to_string();
                let _ = self.cfg.save(&self.paths);
                return;
            }
            KeyCode::Char('b') => {
                self.backup_save();
                return;
            }
            KeyCode::Tab => {
                self.goto(self.tab.next());
                return;
            }
            KeyCode::BackTab => {
                self.goto(self.tab.prev());
                return;
            }
            KeyCode::Char(c @ '1'..='9') => {
                self.goto(Tab::ALL[(c as u8 - b'1') as usize]);
                return;
            }
            _ => {}
        }

        match self.tab {
            Tab::Runes | Tab::Attributes | Tab::Vitals | Tab::Flags => self.field_key(key),
            Tab::Items => self.items_key(key),
            Tab::Scripts => self.scripts_key(key),
            Tab::Process => self.process_key(key),
            Tab::Help => match key.code {
                KeyCode::Down | KeyCode::Char('j') => self.help_scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.help_scroll = self.help_scroll.saturating_sub(1)
                }
                _ => {}
            },
            Tab::Overview => {}
        }
    }

    fn goto(&mut self, tab: Tab) {
        self.tab = tab;
        self.focus = 0;
        self.edit = None;
    }

    fn field_key(&mut self, key: KeyEvent) {
        let rows = self.tab.rows();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.focus = self.focus.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus + 1 < rows {
                    self.focus += 1;
                }
            }
            KeyCode::Home => self.focus = 0,
            KeyCode::End => self.focus = rows.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_field(),
            KeyCode::Char('a') if self.tab == Tab::Runes => self.add_runes(1_000_000),
            KeyCode::Char('m') if self.tab == Tab::Runes => self.add_runes(999_999_999),
            KeyCode::Char('r') => self.repoll(),
            _ => {}
        }
    }

    fn activate_field(&mut self) {
        let fields = screen_fields(self.tab);
        let Some(field) = fields.get(self.focus).copied() else {
            return;
        };
        if self.game.is_none() {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return;
        }
        if field.kind == Kind::Name {
            self.toast("name is read only", ToastKind::Info);
            return;
        }
        if self.tab == Tab::Flags {
            self.toggle_field(field);
            return;
        }
        self.edit = Some(Edit {
            kind: EditKind::Field(field.key),
            buf: self.value_of(field),
            cursor: self.value_of(field).len(),
        });
    }

    fn toggle_field(&mut self, field: &'static Field) {
        let Some(game) = &self.game else {
            return;
        };
        match game.toggle(field) {
            Ok(next) => {
                let state = if next == 0 { "off" } else { "on" };
                self.toast(format!("{} {state}", field.label), ToastKind::Good);
                self.repoll();
            }
            Err(err) => self.toast(format!("{}: {err}", field.label), ToastKind::Bad),
        }
    }

    fn commit_field(&mut self, key: &'static str, buf: &str) {
        let Some(field) = offsets::all_fields().find(|f| f.key == key).copied() else {
            return;
        };
        let Some(game) = &self.game else {
            return;
        };
        match parse_number(buf, field.kind) {
            Ok(number) => match game.set_number(&field, number) {
                Ok(()) => {
                    self.toast(format!("{} = {}", field.label, number), ToastKind::Good);
                    self.repoll();
                }
                Err(err) => self.toast(format!("{}: {err}", field.label), ToastKind::Bad),
            },
            Err(err) => self.toast(err, ToastKind::Bad),
        }
    }

    fn add_runes(&mut self, delta: i64) {
        let Some(game) = &self.game else {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return;
        };
        match game.add_runes(delta) {
            Ok(now) => {
                self.toast(format!("runes: {now}"), ToastKind::Good);
                self.repoll();
            }
            Err(err) => self.toast(err, ToastKind::Bad),
        }
    }

    fn edit_key(&mut self, key: KeyEvent) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {
                self.edit = None;
                return;
            }
            KeyCode::Enter => {
                let edit = self.edit.take().expect("edit checked");
                match edit.kind {
                    EditKind::Search => {
                        self.items.query = edit.buf;
                        self.items.sel = 0;
                        self.refresh_items();
                    }
                    EditKind::Field(key) => self.commit_field(key, &edit.buf),
                }
                return;
            }
            KeyCode::Backspace => {
                if edit.cursor > 0 {
                    let mut idx = edit.cursor - 1;
                    while idx > 0 && !edit.buf.is_char_boundary(idx) {
                        idx -= 1;
                    }
                    edit.buf.remove(idx);
                    edit.cursor = idx;
                }
            }
            KeyCode::Left => {
                let mut idx = edit.cursor.saturating_sub(1);
                while idx > 0 && !edit.buf.is_char_boundary(idx) {
                    idx -= 1;
                }
                edit.cursor = idx;
            }
            KeyCode::Right => {
                if edit.cursor < edit.buf.len() {
                    let mut idx = edit.cursor + 1;
                    while idx < edit.buf.len() && !edit.buf.is_char_boundary(idx) {
                        idx += 1;
                    }
                    edit.cursor = idx;
                }
            }
            KeyCode::Char(c) if !c.is_control() => {
                edit.buf.insert(edit.cursor, c);
                edit.cursor += c.len_utf8();
            }
            _ => return,
        }
        if let Some(Edit {
            kind: EditKind::Search,
            buf,
            ..
        }) = &self.edit
        {
            self.items.query = buf.clone();
            self.items.sel = 0;
            self.refresh_items();
        }
    }

    fn items_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.items.sel = self.items.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                if self.items.sel + 1 < self.items.filtered.len() {
                    self.items.sel += 1;
                }
            }
            KeyCode::PageUp => self.items.sel = self.items.sel.saturating_sub(12),
            KeyCode::PageDown => {
                self.items.sel =
                    (self.items.sel + 12).min(self.items.filtered.len().saturating_sub(1))
            }
            KeyCode::Home => self.items.sel = 0,
            KeyCode::End => self.items.sel = self.items.filtered.len().saturating_sub(1),
            KeyCode::Left | KeyCode::Char('h') => {
                self.items.cat = (self.items.cat + Cat::ALL.len() - 1) % Cat::ALL.len();
                self.items.sel = 0;
                self.items.scroll = 0;
                self.clamp_qty();
                self.refresh_items();
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.items.cat = (self.items.cat + 1) % Cat::ALL.len();
                self.items.sel = 0;
                self.items.scroll = 0;
                self.clamp_qty();
                self.refresh_items();
            }
            KeyCode::Char('/') => {
                self.edit = Some(Edit {
                    kind: EditKind::Search,
                    buf: self.items.query.clone(),
                    cursor: self.items.query.len(),
                });
            }
            KeyCode::Char('x') => {
                self.items.query.clear();
                self.items.sel = 0;
                self.refresh_items();
            }
            KeyCode::Char('s') => {
                self.items.slot = (self.items.slot + 1) % offsets::EQUIP.len();
            }
            KeyCode::Char('S') => {
                self.items.slot =
                    (self.items.slot + offsets::EQUIP.len() - 1) % offsets::EQUIP.len();
            }
            KeyCode::Char('-') => {
                self.items.level = self.items.level.saturating_sub(1);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let max = self.selected_max_level();
                if self.items.level < max {
                    self.items.level += 1;
                } else if max == 0 {
                    self.toast("this item type has no upgrade level", ToastKind::Info);
                } else {
                    self.toast(format!("max upgrade level is +{max}"), ToastKind::Info);
                }
            }
            KeyCode::Char(',') => {
                self.items.qty = self.items.qty.saturating_sub(1).max(1);
            }
            KeyCode::Char('.') => {
                let cap = crate::items::max_stack(
                    Cat::ALL[self.items.cat],
                    self.selected_item().map(|item| item.id).unwrap_or(0),
                );
                self.items.qty = (self.items.qty + 1).min(cap);
            }
            KeyCode::Enter => {
                if self.selected_item().map(|item| item.cat) == Some(Cat::Weapon) {
                    self.open_give_menu();
                } else {
                    self.give_selected();
                }
            }
            KeyCode::Char('e') => self.equip_selected(),
            _ => {}
        }
    }

    fn clamp_qty(&mut self) {
        let cap = crate::items::max_stack(
            Cat::ALL[self.items.cat],
            self.selected_item().map(|item| item.id).unwrap_or(0),
        );
        self.items.qty = self.items.qty.clamp(1, cap);
    }

    fn give_selected(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };
        let (id, name, cat) = (item.id, item.name.to_string(), item.cat);
        let level = self.items.level.min(self.catalog.max_level(id));
        let qty = self.items.qty;
        let gib = crate::items::gib_id(cat, id, level);
        let Some(game) = &self.game else {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return;
        };
        match game.give_item(cat, id, qty, level) {
            Ok(_) => {
                let suffix = if level > 0 {
                    format!(" +{level}")
                } else {
                    String::new()
                };
                self.toast(
                    format!("gave {qty}× {name}{suffix}  ({gib:#010X})"),
                    ToastKind::Good,
                );
                self.repoll();
            }
            Err(err) => self.toast(err, ToastKind::Bad),
        }
    }

    fn open_give_menu(&mut self) {
        let Some(item) = self.selected_item().cloned() else {
            return;
        };
        let family = item.id - item.id % 10_000;
        let max_level = self.catalog.max_level(family);
        self.give_menu = Some(GiveMenu {
            family,
            name: self
                .catalog
                .name_of(family, Cat::Weapon)
                .unwrap_or_else(|| item.name.to_string()),
            level: self.items.level.min(max_level),
            max_level,
            qty: self.items.qty,
            cat: item.cat,
        });
    }

    fn give_menu_key(&mut self, key: KeyEvent) {
        if self.give_menu.is_none() {
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.give_menu = None;
                return;
            }
            KeyCode::Enter => {
                if let Some(menu) = self.give_menu.take() {
                    self.give_with(menu.cat, menu.id(), menu.qty, menu.level);
                }
                return;
            }
            _ => {}
        }
        let Some(menu) = self.give_menu.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => {
                menu.level = menu.level.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('+') | KeyCode::Char('=') => {
                if menu.level < menu.max_level {
                    menu.level += 1;
                }
            }
            _ => {}
        }
    }

    fn give_with(&mut self, cat: Cat, id: u32, qty: u32, level: u32) {
        let name = self
            .catalog
            .name_of(id, cat)
            .unwrap_or_else(|| format!("id {id}"));
        let gib = crate::items::gib_id(cat, id, level);
        let Some(game) = &self.game else {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return;
        };
        match game.give_item(cat, id, qty, level) {
            Ok(_) => {
                let suffix = if level > 0 {
                    format!(" +{level}")
                } else {
                    String::new()
                };
                self.toast(
                    format!("gave {qty}× {name}{suffix}  ({gib:#010X})"),
                    ToastKind::Good,
                );
                self.repoll();
            }
            Err(err) => self.toast(err, ToastKind::Bad),
        }
    }

    fn scripts_key(&mut self, key: KeyEvent) {
        let last = SCRIPTS.len().saturating_sub(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.scripts.sel = self.scripts.sel.saturating_sub(1);
                self.scripts.pending = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.scripts.sel < last {
                    self.scripts.sel += 1;
                }
                self.scripts.pending = None;
            }
            KeyCode::Home => {
                self.scripts.sel = 0;
                self.scripts.pending = None;
            }
            KeyCode::End => {
                self.scripts.sel = last;
                self.scripts.pending = None;
            }
            KeyCode::Esc => self.scripts.pending = None,
            KeyCode::Char('d') => {
                self.scripts.include_dlc = !self.scripts.include_dlc;
                self.refresh_script_counts();
                let text = if self.scripts.include_dlc {
                    "DLC items on — scripts use the DLC lists too"
                } else {
                    "DLC items off — base game lists only"
                };
                self.toast(text, ToastKind::Info);
            }
            KeyCode::Enter => {
                if self.scripts.pending == Some(self.scripts.sel) {
                    self.scripts.pending = None;
                    self.run_request = Some(self.scripts.sel);
                } else {
                    self.scripts.pending = Some(self.scripts.sel);
                    self.scripts.last = None;
                }
            }
            _ => {}
        }
    }

    fn execute_script(&mut self, index: usize, terminal: &mut Tui) -> std::io::Result<()> {
        let Some(script) = SCRIPTS.get(index) else {
            return Ok(());
        };
        if self.game.is_none() {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return Ok(());
        }
        match script.kind {
            ScriptKind::Items { .. } => {
                let gives = self
                    .script_lib
                    .resolve(script, &self.catalog, self.scripts.include_dlc);
                let mut label = if self.scripts.include_dlc {
                    format!("{} (with DLC)", script.name)
                } else {
                    script.name.to_string()
                };
                if let Some(note) = self.ensure_crafting_kit(script) {
                    label.push_str(&format!(" — {note}"));
                }

                let recipes = crate::scripts::unlock_flags(script);
                if !recipes.is_empty() {
                    self.run_flag_list(terminal, &format!("{label} recipes"), recipes)?;
                }
                self.run_gives(terminal, &label, gives)
            }
            ScriptKind::MapMarkings => {
                let flags = crate::scripts::map_marking_flags();
                self.run_flag_bytes(terminal, script.name, &flags)
            }
            ScriptKind::BellBearings => {
                let flags = crate::scripts::bell_bearing_flags();
                self.run_flag_bytes(terminal, script.name, &flags)
            }
            ScriptKind::MaxFlaskNumber => self.run_max_flask_number(),
            ScriptKind::MaxFlaskCharge => self.run_max_flask_charge(),
            ScriptKind::UnlockGraces => {
                let flags = crate::scripts::grace_flags();
                self.run_flag_bytes(terminal, script.name, &flags)
            }
            ScriptKind::SummoningPools => {
                let flags = crate::scripts::summoning_pool_flags();
                self.run_flag_bytes(terminal, script.name, &flags)
            }
            ScriptKind::Bosses { dead } => self.run_boss_flags(terminal, script.name, dead),
            ScriptKind::UnlockTorrent => self.run_unlock_torrent(terminal, script.name),
            ScriptKind::UnlockRoundtable => {
                let flags = crate::scripts::ROUNDTABLE_FLAGS;
                self.run_flag_list(terminal, script.name, &flags)
            }
            ScriptKind::RevealMaps => {
                let flags = crate::scripts::map_reveal_flags();
                self.run_flag_list(terminal, script.name, &flags)
            }
        }
    }

    fn refresh_script_counts(&mut self) {
        let include_dlc = self.scripts.include_dlc;
        self.script_counts = SCRIPTS
            .iter()
            .map(|script| {
                self.script_lib
                    .resolve(script, &self.catalog, include_dlc)
                    .len()
            })
            .collect();
    }

    fn run_gives(
        &mut self,
        terminal: &mut Tui,
        label: &str,
        gives: Vec<Give>,
    ) -> std::io::Result<()> {
        let total = gives.len();
        if total == 0 {
            self.toast(format!("{label}: nothing to give"), ToastKind::Warn);
            return Ok(());
        }

        let mut given = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        let mut aborted = false;
        let mut failure: Option<String> = None;
        for (i, (cat, id, qty, level)) in gives.iter().enumerate() {
            let name = self
                .catalog
                .name_of(*id, *cat)
                .unwrap_or_else(|| format!("id {id}"));
            let outcome = match self.game.as_ref() {
                Some(game) => Some(game.give_item(*cat, *id, *qty, *level)),
                None => None,
            };
            match outcome {
                Some(Ok(returned)) => {
                    if returned == 0 {
                        skipped += 1;
                    } else {
                        given += 1;
                    }

                    std::thread::sleep(Duration::from_millis(25));
                    if given % 20 == 0 {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }

                Some(Err(err)) if err.contains("no item param") => {
                    skipped += 1;
                }
                Some(Err(err)) => {
                    failed += 1;
                    failure = Some(format!("{name} ({id}): {err}"));
                    break;
                }
                None => break,
            }
            if i % 6 == 0 || i + 1 == total {
                self.scripts.progress = Some(Progress {
                    done: i + 1,
                    total,
                    name,
                });
                terminal.draw(|frame| ui::draw(self, frame))?;
                if event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Esc => {
                                aborted = true;
                                break;
                            }
                            KeyCode::Char('q') => {
                                self.quit = true;
                                aborted = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.scripts.progress = None;

        let mut text = match &failure {
            Some(reason) => format!("{label}: stopped after {given}/{total} — {reason}"),
            None => format!("{label}: {given}/{total} given"),
        };
        if skipped > 0 {
            text.push_str(&format!(" ({skipped} already owned or refused)"));
        }
        if aborted {
            text.push_str(" — stopped early");
        }
        let ok = failed == 0 && !aborted;
        self.scripts.last = Some((text.clone(), ok));
        self.toast(text, if ok { ToastKind::Good } else { ToastKind::Warn });
        self.repoll();
        Ok(())
    }

    fn run_flag_bytes(
        &mut self,
        terminal: &mut Tui,
        label: &str,
        flags: &[u32],
    ) -> std::io::Result<()> {
        let total = flags.len();
        if total == 0 {
            self.toast(format!("{label}: no flags"), ToastKind::Warn);
            return Ok(());
        }
        let mut set = 0usize;
        let mut failed = 0usize;
        let mut aborted = false;
        let mut failure: Option<String> = None;
        for (i, flag) in flags.iter().enumerate() {
            let outcome = match self.game.as_ref() {
                Some(game) => Some(game.set_flag_via_game(*flag, true)),
                None => None,
            };
            match outcome {
                Some(Ok(())) => set += 1,
                Some(Err(err)) => {
                    failed += 1;
                    failure = Some(format!("flag {flag}: {err}"));
                    break;
                }
                None => break,
            }
            if i % 256 == 0 || i + 1 == total {
                self.scripts.progress = Some(Progress {
                    done: i + 1,
                    total,
                    name: format!("flag {flag}"),
                });
                terminal.draw(|frame| ui::draw(self, frame))?;
                if event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Esc => {
                                aborted = true;
                                break;
                            }
                            KeyCode::Char('q') => {
                                self.quit = true;
                                aborted = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.scripts.progress = None;

        let mut text = match &failure {
            Some(reason) => format!("{label}: stopped after {set}/{total} — {reason}"),
            None => format!("{label}: {set}/{total} flags set"),
        };
        if aborted {
            text.push_str(" — stopped early");
        }
        let ok = failed == 0 && !aborted;
        self.scripts.last = Some((text.clone(), ok));
        self.toast(text, if ok { ToastKind::Good } else { ToastKind::Warn });
        self.repoll();
        Ok(())
    }

    fn ensure_crafting_kit(&mut self, script: &crate::scripts::Script) -> Option<String> {
        if crate::scripts::prerequisites(script).is_empty() {
            return None;
        }
        let owned = self
            .game
            .as_ref()
            .and_then(|game| game.event_flag(crate::scripts::CRAFTING_KIT_FLAG))
            .unwrap_or(false);
        if owned {
            return Some("crafting kit already owned".to_string());
        }
        let game = self.game.as_ref()?;
        match game.give_item(Cat::Goods, crate::scripts::CRAFTING_KIT, 1, 0) {
            Ok(_) => {
                let _ = game.set_event_flag(crate::scripts::CRAFTING_KIT_FLAG, true);
                Some("crafting kit given".to_string())
            }
            Err(err) => Some(format!("crafting kit refused: {err}")),
        }
    }

    fn run_max_flask_number(&mut self) -> std::io::Result<()> {
        let outcome = match self.game.as_ref() {
            Some(game) => game.max_flask_charges(),
            None => return Ok(()),
        };
        match outcome {
            Ok((hp, mp)) => {
                let text = format!(
                    "flasks: {hp} crimson + {mp} cerulean = {} charges — rest at a grace to refresh",
                    u32::from(hp) + u32::from(mp)
                );
                self.scripts.last = Some((text.clone(), true));
                self.toast(text, ToastKind::Good);
            }
            Err(err) => {
                self.scripts.last = Some((err.clone(), false));
                self.toast(err, ToastKind::Bad);
            }
        }
        self.repoll();
        Ok(())
    }

    fn run_max_flask_charge(&mut self) -> std::io::Result<()> {
        self.scripts.progress = Some(Progress {
            done: 0,
            total: 1,
            name: "upgrading the flasks".to_string(),
        });
        let outcome = match self.game.as_ref() {
            Some(game) => game.max_flask_charge(),
            None => return Ok(()),
        };
        self.scripts.progress = None;
        match outcome {
            Ok((level, swapped)) => {
                let text = if swapped {
                    format!("flasks upgraded to +{level} — rest at a grace to save it")
                } else {
                    format!("flasks were already +{level}, nothing to change")
                };
                self.scripts.last = Some((text.clone(), true));
                self.toast(text, ToastKind::Good);
            }
            Err(err) => {
                self.scripts.last = Some((err.clone(), false));
                self.toast(err, ToastKind::Bad);
            }
        }
        self.repoll();
        Ok(())
    }

    fn run_flag_list(
        &mut self,
        terminal: &mut Tui,
        label: &str,
        flags: &[u32],
    ) -> std::io::Result<()> {
        let total = flags.len();
        if total == 0 {
            self.toast(format!("{label}: nothing to set"), ToastKind::Warn);
            return Ok(());
        }
        let mut set = 0usize;
        let mut failed = 0usize;
        let mut aborted = false;
        let mut failure: Option<String> = None;
        for (i, flag) in flags.iter().enumerate() {
            let outcome = match self.game.as_ref() {
                Some(game) => Some(game.set_event_flag(*flag, true)),
                None => None,
            };
            match outcome {
                Some(Ok(())) => {
                    set += 1;
                    std::thread::sleep(Duration::from_millis(8));
                }
                Some(Err(err)) => {
                    failed += 1;
                    failure = Some(format!("flag {flag}: {err}"));
                    break;
                }
                None => break,
            }
            if i % 4 == 0 || i + 1 == total {
                self.scripts.progress = Some(Progress {
                    done: i + 1,
                    total,
                    name: format!("flag {flag}"),
                });
                terminal.draw(|frame| ui::draw(self, frame))?;
                if event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Esc => {
                                aborted = true;
                                break;
                            }
                            KeyCode::Char('q') => {
                                self.quit = true;
                                aborted = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.scripts.progress = None;

        let mut text = match &failure {
            Some(reason) => format!("{label}: stopped after {set}/{total} — {reason}"),
            None => format!("{label}: {set}/{total} flags set"),
        };
        if aborted {
            text.push_str(" — stopped early");
        }
        let ok = failed == 0 && !aborted;
        self.scripts.last = Some((text.clone(), ok));
        self.toast(text, if ok { ToastKind::Good } else { ToastKind::Warn });
        self.repoll();
        Ok(())
    }

    fn run_boss_flags(
        &mut self,
        terminal: &mut Tui,
        label: &str,
        dead: bool,
    ) -> std::io::Result<()> {
        let offsets = crate::scripts::boss_flag_offsets();
        let total = offsets.len();
        if total == 0 {
            self.toast(format!("{label}: no boss flags"), ToastKind::Warn);
            return Ok(());
        }
        let value = if dead { 0xFF } else { 0x00 };
        let mut written = 0usize;
        let mut failed = 0usize;
        let mut aborted = false;
        let mut failure: Option<String> = None;
        for (i, offset) in offsets.iter().enumerate() {
            let outcome = match self.game.as_ref() {
                Some(game) => Some(game.set_flag_byte(*offset, value)),
                None => None,
            };
            match outcome {
                Some(Ok(())) => {
                    written += 1;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Some(Err(err)) => {
                    failed += 1;
                    failure = Some(format!("flag +{offset:#X}: {err}"));
                    break;
                }
                None => break,
            }
            if i % 32 == 0 || i + 1 == total {
                self.scripts.progress = Some(Progress {
                    done: i + 1,
                    total,
                    name: format!("flag +{offset:#X}"),
                });
                terminal.draw(|frame| ui::draw(self, frame))?;
                if event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Esc => {
                                aborted = true;
                                break;
                            }
                            KeyCode::Char('q') => {
                                self.quit = true;
                                aborted = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        self.scripts.progress = None;

        let mut text = match &failure {
            Some(reason) => format!("{label}: stopped after {written}/{total} — {reason}"),
            None => format!("{label}: {written}/{total} bosses written"),
        };
        if aborted {
            text.push_str(" — stopped early");
        }
        if failed == 0 && !aborted {
            text.push_str(" — reload the area for it to show");
        }
        let ok = failed == 0 && !aborted;
        self.scripts.last = Some((text.clone(), ok));
        self.toast(text, if ok { ToastKind::Good } else { ToastKind::Warn });
        self.repoll();
        Ok(())
    }

    fn run_unlock_torrent(&mut self, terminal: &mut Tui, label: &str) -> std::io::Result<()> {
        let flags = crate::scripts::TORRENT_FLAGS;
        self.run_flag_list(terminal, label, &flags)?;
        let (cat, id, qty, level) = crate::scripts::WHISTLE;
        match self.game.as_ref().map(|game| game.give_item(cat, id, qty, level)) {
            Some(Ok(_)) => {
                let text = format!("{label}: flags set, {qty}× spectral steed whistle given");
                self.scripts.last = Some((text.clone(), true));
                self.toast(text, ToastKind::Good);
                self.repoll();
            }
            Some(Err(err)) => self.toast(
                format!("{label}: flags set but the whistle was refused — {err}"),
                ToastKind::Warn,
            ),
            None => self.toast("attach to the game first (F5)", ToastKind::Warn),
        }
        Ok(())
    }

    fn equip_selected(&mut self) {
        let Some(&idx) = self.items.filtered.get(self.items.sel) else {
            return;
        };
        let item = &self.catalog.items[idx];
        let (id, name) = (item.id, item.name.to_string());
        let slot = self.current_slot();
        let Some(game) = &self.game else {
            self.toast("attach to the game first (F5)", ToastKind::Warn);
            return;
        };
        match game.equip(slot, id) {
            Ok(()) => {
                let label = slot.label.to_string();
                self.toast(format!("{label} = {name} ({id})"), ToastKind::Good);
                self.repoll();
            }
            Err(err) => self.toast(format!("{name}: {err}"), ToastKind::Bad),
        }
    }

    fn process_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter | KeyCode::Char('a') => self.attach(),
            KeyCode::Char('d') => self.detach(),
            KeyCode::Char('r') => {
                self.found = process::scan(&self.cfg.process);
                let count = self.found.len();
                self.toast(
                    format!("{count} match(es) for {}", self.cfg.process),
                    ToastKind::Info,
                );
            }
            _ => {}
        }
    }
}
