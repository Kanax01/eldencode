mod aob;
mod app;
mod config;
mod game;
mod items;
mod mem;
mod offsets;
#[allow(dead_code)]
mod pe;
mod scripts;
mod spawn;
mod process;
mod ui;

use std::io::{self, IsTerminal};

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;
use config::{Config, Paths};

struct Cli {
    home: Option<String>,
    no_anim: bool,
    no_attach: bool,
    scan: bool,
    probe: bool,
    give: Option<u32>,
    script: Option<String>,
    dlc: bool,
    params: bool,
    limit: Option<usize>,
    skip: usize,
    qty: u32,
    level: u32,
    help: bool,
    version: bool,
}

fn main() {
    let cli = match parse_cli() {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("eldencode: {err}");
            eprintln!("try `eldencode --help`");
            std::process::exit(2);
        }
    };

    if cli.help {
        print_help();
        return;
    }
    if cli.version {
        println!("eldencode {}", config::VERSION);
        return;
    }

    let paths = Paths::discover(cli.home.as_deref());
    let mut cfg = Config::load(&paths);
    if cli.no_anim {
        cfg.animations = false;
    }
    if cli.no_attach {
        cfg.auto_attach = false;
    }

    if cli.scan {
        print_scan(&cfg);
        return;
    }
    if cli.probe {
        print_probe(&cfg);
        return;
    }
    if let Some(id) = cli.give {
        print_give(&cfg, id, cli.qty, cli.level);
        return;
    }
    if cli.params {
        print_params(&cfg);
        return;
    }
    if let Some(name) = &cli.script {
        print_script(&cfg, name, cli.skip, cli.limit, cli.dlc);
        return;
    }

    if let Err(err) = config::ensure_home(&paths) {
        eprintln!("eldencode: cannot prepare {}: {err}", paths.home.display());
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        eprintln!(
            "eldencode: this is an interactive TUI and needs a terminal on stdin and stdout\n\
             try running it directly in a terminal, or see `eldencode --help`"
        );
        std::process::exit(2);
    }

    if let Err(err) = run(paths, cfg) {
        eprintln!("eldencode: {err}");
        std::process::exit(1);
    }
}

fn print_scan(cfg: &Config) {
    let found = process::scan(&cfg.process);
    if found.is_empty() {
        println!("no process matches '{}'", cfg.process);
        if process::anti_cheat_running() {
            println!("EasyAntiCheat is running — the game must be started with EAC disabled");
        }
        return;
    }
    for proc in found {
        println!(
            "{}  pid {:<7} base {:#012X}  {:.1} MiB",
            proc.comm,
            proc.pid,
            proc.base,
            proc.size_mb()
        );
        if let Some(prefix) = &proc.prefix {
            println!("    prefix {}", prefix.display());
            for save in process::saves(prefix) {
                println!("    save   {}", save.display());
            }
        }
        println!("    cmd    {}", proc.cmdline);
    }
}

/// Show the param tables found in the running game and check ids against them.
/// This only reads: nothing is called inside the game.
fn print_params(cfg: &Config) {
    let Some(proc) = process::scan(&cfg.process).into_iter().next() else {
        println!("no process matches '{}'", cfg.process);
        std::process::exit(1);
    };
    let game = match game::Game::attach(proc) {
        Ok(game) => game,
        Err(err) => {
            println!("attach failed: {}", err.message);
            if let Some(hint) = err.hint {
                println!("hint: {hint}");
            }
            std::process::exit(1);
        }
    };
    for cat in items::Cat::ALL {
        match game.item_param(cat) {
            Some(table) => println!(
                "{:<9} {:<7} rows at {:#X}",
                format!("{cat:?}"),
                table.rows,
                table.head
            ),
            None => println!("{:<9} not found", format!("{cat:?}")),
        }
    }
    println!();
    let checks = [
        (items::Cat::Weapon, 8_030_000u32, "Bloodhound's Fang +0"),
        (items::Cat::Weapon, 8_030_010, "Bloodhound's Fang +10"),
        (items::Cat::Weapon, 8_030_200, "Fang variant row (bogus)"),
        (items::Cat::Weapon, 8_030_210, "Fang variant +10 (bogus)"),
        (items::Cat::Weapon, 1_000_000, "Dagger +0"),
        (items::Cat::Weapon, 1_000_205, "variant row 1000200 +5"),
        (items::Cat::Weapon, 1_000_225, "variant row 1000200 +25"),
        (items::Cat::Goods, 50_000_000, "Arrow"),
    ];
    for (cat, id, label) in checks {
        let verdict = match game.has_item(cat, id) {
            Some(true) => "present",
            Some(false) => "ABSENT",
            None => "uncheckable",
        };
        println!("  {label:<26} {id:>10}  {verdict}");
    }
}

fn print_probe(cfg: &Config) {
    let Some(proc) = process::scan(&cfg.process).into_iter().next() else {
        println!("no process matches '{}'", cfg.process);
        if process::anti_cheat_running() {
            println!("EasyAntiCheat is running — start the game with EAC disabled");
        }
        std::process::exit(1);
    };
    println!(
        "{}  pid {}  base {:#012X}  {:.1} MiB",
        proc.comm,
        proc.pid,
        proc.base,
        proc.size_mb()
    );
    let game = match game::Game::attach(proc) {
        Ok(game) => game,
        Err(err) => {
            println!("attach failed: {}", err.message);
            if let Some(hint) = err.hint {
                println!("hint: {hint}");
            }
            std::process::exit(1);
        }
    };
    println!(
        "scanned {:.1} MiB over {} regions",
        game.scanned_mb, game.regions
    );
    match spawn::callable_thread(&game.proc) {
        Some(tid) => println!("call thread: tid {tid} (parked game thread)"),
        None => println!("call thread: none parked (the main thread is never used)"),
    }
    let mut missing = 0;
    for (name, addr) in game.symbols_report() {
        match addr {
            Some(addr) => println!("  {name:<18} {addr:#012X}"),
            None => {
                missing += 1;
                println!("  {name:<18} MISSING");
            }
        }
    }
    println!();
    for field in offsets::all_fields() {
        match game.read(field) {
            Ok(value) => println!("  {:<18} {}", field.key, value.display()),
            Err(err) => println!("  {:<18} error: {err}", field.key),
        }
    }
    // Write check: every writable field is rewritten with the value it already
    // holds, so write access is exercised without changing the save.
    let mut written = 0;
    let mut write_error = None;
    for field in offsets::RUNES.iter().chain(offsets::EQUIP.iter()) {
        let Ok(value) = game.read(field) else {
            continue;
        };
        let Some(number) = value.u32() else { continue };
        if offsets::EQUIP.iter().any(|f| f.key == field.key) && number == offsets::EMPTY_SLOT {
            continue;
        }
        match game.set_number(field, number as u64) {
            Ok(()) => written += 1,
            Err(err) => {
                write_error = Some(format!("{}: {err}", field.key));
                break;
            }
        }
    }
    println!();
    match game.inventory_manager() {
        Some(addr) => println!("  inventory manager  {addr:#012X}"),
        None => println!("  inventory manager  null — load a character first"),
    }
    match game.add_item_addr() {
        Some(addr) => println!("  add item routine   {addr:#012X}"),
        None => println!("  add item routine   MISSING"),
    }
    match game.event_flag_setter() {
        Some(addr) => println!("  flag set routine   {addr:#012X}"),
        None => println!("  flag set routine   MISSING"),
    }
    match game.event_flag_manager() {
        Some(addr) => println!("  flag manager       {addr:#012X}"),
        None => println!("  flag manager       null — load a save first"),
    }
    match write_error {
        Some(err) => println!("\nwrite check FAILED: {err}"),
        None => println!("\nwrite check: rewrote {written} field(s) with their current value"),
    }
    if missing > 0 {
        println!("{missing} symbol(s) missing — offsets do not match this game build");
        std::process::exit(1);
    }
}

/// `--script`: run a bulk script without the UI, one line per item, stopping at
/// the first failure. `--skip` and `--limit` cut the list down for testing.
fn print_script(
    cfg: &Config,
    name: &str,
    skip: usize,
    limit: Option<usize>,
    include_dlc: bool,
) {
    let catalog = items::Catalog::load();
    let library = scripts::Library::load();
    let needle = name.to_lowercase();
    let Some(script) = scripts::SCRIPTS.iter().find(|script| {
        script.name.to_lowercase().contains(&needle)
    }) else {
        eprintln!("eldencode: no script matches '{name}'. Available:");
        for script in scripts::SCRIPTS.iter() {
            eprintln!("  {}", script.name);
        }
        std::process::exit(2);
    };

    let Some(proc) = process::scan(&cfg.process).into_iter().next() else {
        eprintln!("no process matches '{}'", cfg.process);
        std::process::exit(1);
    };
    let game = match game::Game::attach(proc) {
        Ok(game) => game,
        Err(err) => {
            eprintln!("attach failed: {}", err.message);
            if let Some(hint) = err.hint {
                eprintln!("hint: {hint}");
            }
            std::process::exit(1);
        }
    };

    println!("script: {}", script.name);
    match script.kind {
        scripts::Kind::Items { .. } => {
            let gives = library.resolve(script, &catalog, include_dlc);
            let total = gives.len();
            println!("DLC items: {}", if include_dlc { "on" } else { "off" });
            let take = limit.unwrap_or(usize::MAX);
            println!("{total} items in the list, starting at {skip}, limit {take}");
            let mut given = 0usize;
            let mut absent = 0usize;
            for (index, (cat, id, qty, level)) in gives.into_iter().enumerate().skip(skip) {
                if given >= take {
                    break;
                }
                let label = catalog
                    .name_of(id, cat)
                    .unwrap_or_else(|| format!("id {id}"));
                print!("  [{index}] {label} ({id}) x{qty} +{level} ... ");
                use std::io::Write;
                let _ = std::io::stdout().flush();
                match game.give_item(cat, id, qty, level) {
                    Ok(returned) => {
                        println!("ok {returned:#X}");
                        given += 1;
                        // The game rebuilds its inventory on the main thread after
                        // each item; give it room, and a longer beat every so
                        // often, instead of hammering it for a whole list.
                        std::thread::sleep(std::time::Duration::from_millis(25));
                        if given % 20 == 0 {
                            std::thread::sleep(std::time::Duration::from_millis(250));
                        }
                    }
                    // A stale id in the table's list is not a reason to stop a
                    // bulk give: the game has no param row for it, nothing was
                    // sent, and the rest of the list is still good.
                    Err(err) if err.contains("no item param") => {
                        println!("skipped (no param row)");
                        absent += 1;
                    }
                    Err(err) => {
                        println!("FAILED: {err}");
                        break;
                    }
                }
            }
            if absent > 0 {
                println!("done: {given} given ({absent} ids with no param row skipped)");
            } else {
                println!("done: {given} given");
            }
        }
        scripts::Kind::UnlockGraces => run_flags(&game, "graces", offsets::BONFIRE_LIST, true, skip, limit),
        scripts::Kind::SummoningPools => {
            run_flags(&game, "summoning pools", offsets::PUDDLE_LIST, false, skip, limit)
        }
    }
}

fn run_flags(
    game: &game::Game,
    label: &str,
    list: u64,
    flag_in_sub: bool,
    skip: usize,
    limit: Option<usize>,
) {
    let flags = match game.param_flag_ids(list, flag_in_sub) {
        Ok(flags) => flags,
        Err(err) => {
            eprintln!("{label}: {err}");
            std::process::exit(1);
        }
    };
    let take = limit.unwrap_or(usize::MAX);
    println!("{label}: {} flags, starting at {skip}, limit {take}", flags.len());
    let mut set = 0usize;
    let mut failed = 0usize;
    for (index, flag) in flags.iter().enumerate().skip(skip) {
        if set >= take {
            break;
        }
        print!("  [{index}] flag {flag} ... ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        match game.set_event_flag(*flag, true) {
            Ok(()) => {
                println!("ok");
                set += 1;
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
            Err(err) => {
                println!("FAILED: {err}");
                failed += 1;
                break;
            }
        }
    }
    println!("done: {set} flags set, {failed} failed");
}

/// `--give`: add an item to the inventory from the command line, the same path
/// the Items screen uses.
fn print_give(cfg: &Config, id: u32, qty: u32, level: u32) {
    let catalog = items::Catalog::load();
    let found = catalog
        .items
        .iter()
        .find(|item| item.id == id)
        .or_else(|| {
            catalog
                .items
                .iter()
                .find(|item| item.id == items::base_id(id))
        })
        .map(|item| (item.cat, item.id, item.name.to_string()));
    let Some((cat, base, name)) = found else {
        eprintln!("eldencode: item id {id} is not in assets/items.tsv");
        std::process::exit(2);
    };
    let level = level.min(catalog.max_level(base));
    let gib = items::gib_id(cat, base, level);
    let suffix = if level > 0 {
        format!(" +{level}")
    } else {
        String::new()
    };
    println!("giving {qty}× {name}{suffix}  id {gib:#010X}");

    let Some(proc) = process::scan(&cfg.process).into_iter().next() else {
        eprintln!("no process matches '{}'", cfg.process);
        std::process::exit(1);
    };
    let game = match game::Game::attach(proc) {
        Ok(game) => game,
        Err(err) => {
            eprintln!("attach failed: {}", err.message);
            if let Some(hint) = err.hint {
                eprintln!("hint: {hint}");
            }
            std::process::exit(1);
        }
    };
    match game.give_item(cat, base, qty, level) {
        Ok(returned) => println!("the item routine returned {returned:#X}"),
        Err(err) => {
            eprintln!("give failed: {err}");
            std::process::exit(1);
        }
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
    }
}

fn run(paths: Paths, cfg: Config) -> io::Result<()> {
    install_panic_hook();
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen, Hide)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(paths, cfg);
    app.run(&mut terminal)
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        previous(info);
    }));
}

fn parse_cli() -> Result<Cli, String> {
    let mut cli = Cli {
        home: None,
        no_anim: false,
        no_attach: false,
        scan: false,
        probe: false,
        give: None,
        script: None,
        dlc: false,
        params: false,
        limit: None,
        skip: 0,
        qty: 1,
        level: 0,
        help: false,
        version: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => cli.help = true,
            "-V" | "--version" => cli.version = true,
            "--home" => cli.home = Some(args.next().ok_or("--home needs a directory")?),
            "--no-anim" => cli.no_anim = true,
            "--no-attach" => cli.no_attach = true,
            "--scan" => cli.scan = true,
            "--probe" => cli.probe = true,
            "--give" => {
                let raw = args.next().ok_or("--give needs an item id")?;
                cli.give = Some(
                    raw.parse()
                        .map_err(|_| format!("'--give {raw}' is not a numeric item id"))?,
                );
            }
            "--qty" => {
                let raw = args.next().ok_or("--qty needs a number")?;
                cli.qty = raw
                    .parse::<u32>()
                    .map_err(|_| format!("'--qty {raw}' is not a number"))?
                    .clamp(1, 99);
            }
            "--dlc" => cli.dlc = true,
            "--params" => cli.params = true,
            "--script" => {
                cli.script = Some(args.next().ok_or("--script needs a script name")?);
            }
            "--limit" => {
                let raw = args.next().ok_or("--limit needs a number")?;
                cli.limit = Some(
                    raw.parse::<usize>()
                        .map_err(|_| format!("'--limit {raw}' is not a number"))?,
                );
            }
            "--skip" => {
                let raw = args.next().ok_or("--skip needs a number")?;
                cli.skip = raw
                    .parse::<usize>()
                    .map_err(|_| format!("'--skip {raw}' is not a number"))?;
            }
            "--level" => {
                let raw = args.next().ok_or("--level needs a number")?;
                cli.level = raw
                    .parse::<u32>()
                    .map_err(|_| format!("'--level {raw}' is not a number"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'"));
            }
            other => return Err(format!("unexpected argument '{other}'")),
        }
    }
    Ok(cli)
}

fn print_help() {
    println!(
        "eldencode {version} — a terminal mod menu for Elden Ring on Linux/Proton

USAGE:
    eldencode [OPTIONS]

OPTIONS:
    --home <DIR>   use DIR instead of ~/.eldencode
    --no-attach    start detached (press F5 to attach)
    --no-anim      lower UI refresh rate for this session
    --scan         print detected game processes and their Proton prefix, then exit
    --probe        attach, resolve every symbol and print the live values, then exit
    --give <ID>    add item ID to the inventory, then exit (needs the game running)
    --qty <N>      how many to add with --give (1-999, default 1)
    --script <N>   run a bulk script by name without the UI, then exit
    --dlc          with --script, include the DLC-only item lists
    --params       show the game's item param tables and check a few ids
    --limit <N>    with --script, stop after N items
    --skip <N>     with --script, start at item N
    --level <N>    upgrade level for --give (capped at +25 / +10 per weapon)
    -h, --help     print this help
    -V, --version  print the version

OVERVIEW:
    Elden Ring under Steam/Proton is an ordinary Linux process (Wine hosts the
    Windows exe), so eldencode attaches through /proc: it reads the image base,
    resolves the same AOB-scanned base pointers the Hexinton 8.0.4 Cheat Engine
    table uses, and writes the character record through process_vm_writev.
    Runes, attributes, HP/FP/SP, the ChrDbg flags and equipment slots are
    editable, and items can be added to the inventory: the Items screen calls the
    game's own item routine the way the table's ItemGib does, borrowing the main
    thread with ptrace for a single call and restoring it immediately. No kernel
    module, no Cheat Engine, nothing installed into the game.

REQUIREMENTS:
    - Elden Ring started with EasyAntiCheat disabled (offline)
    - permission to read another process's memory: either
        sudo sysctl -w kernel.yama.ptrace_scope=0
      for the session, or run eldencode with sudo
    - a save backup before editing: press b, or copy ER0000.sl2 yourself

KEYS:
    1-8 / tab       switch screens        F5       attach or re-scan
    up/down         move                  enter    edit / toggle / equip
    a / m           add 1M / max runes    b        back up save
    t               theme                 r        refresh values
    q / ctrl-c      quit                  /        search items
    - and +         upgrade level         , and .  quantity
    enter           give item             e        equip into the current slot
    left/right      switch item type      s / S    cycle equipment slot

Files live in ~/.eldencode: config.toml and backups/.

Offsets follow the Hexinton All in One 8.0.4 table (game 1.17.1). After a game
patch some AOB scans can miss; the header then reports which symbols failed.",
        version = config::VERSION
    );
}
