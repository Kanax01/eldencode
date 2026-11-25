mod aob;
mod app;
mod config;
mod game;
mod items;
mod mem;
mod offsets;
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
    match write_error {
        Some(err) => println!("\nwrite check FAILED: {err}"),
        None => println!("\nwrite check: rewrote {written} field(s) with their current value"),
    }
    if missing > 0 {
        println!("{missing} symbol(s) missing — offsets do not match this game build");
        std::process::exit(1);
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
    -h, --help     print this help
    -V, --version  print the version

OVERVIEW:
    Elden Ring under Steam/Proton is an ordinary Linux process (Wine hosts the
    Windows exe), so eldencode attaches through /proc: it reads the image base,
    resolves the same AOB-scanned base pointers the Hexinton 8.0.4 Cheat Engine
    table uses, and writes the character record through process_vm_writev.
    Runes, attributes, HP/FP/SP, the ChrDbg flags and equipment slots are
    editable. No kernel module, no Cheat Engine, nothing installed into the game.

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

Files live in ~/.eldencode: config.toml and backups/.

Offsets follow the Hexinton All in One 8.0.4 table (game 1.17.1). After a game
patch some AOB scans can miss; the header then reports which symbols failed.",
        version = config::VERSION
    );
}
