# EldenCode

A terminal mod menu for **Elden Ring** on Linux, where the game runs through
Steam/Proton. It gives you runes, edits attributes and vitals, flips the game's
debug flags and slots weapons, armour and talismans straight into your
character — all from a ratatui interface in your terminal.

```
 ELDENCODE   elden ring mod menu · Proton aware
╭──────────────────────────────────────────────────────────────────────────╮
│ ▸1 Overview  2 Runes  3 Attributes  4 Vitals  5 Flags  6 Items  …        │
╰──────────────────────────────────────────────────────────────────────────╯
╭ session ─────────────────────────╮╭ tarnished ───────────────────────────╮
│ ● attached  eldenring.exe        ││ Name        Melina                   │
│ Process id      48213            ││ Level       137   runes 4200318      │
│ Image base      0x140000000      ││ HP       ████████████░░░░  1450/1780 │
│ Symbols         6/6 resolved     ││ FP       ██████████████░░   220/240  │
╰──────────────────────────────────╯╰──────────────────────────────────────╯
```

## What it does

| Screen | What you get |
| --- | --- |
| **1 Overview** | attach state, process id, image base, resolved symbols, Proton prefix, live character snapshot |
| **2 Runes** | runes held, lifetime runes, rune arc; `a` adds 1,000,000, `m` sets 999,999,999 |
| **3 Attributes** | level, vigor, mind, endurance, strength, dexterity, intelligence, faith, arcane, scadutree blessing, revered spirit ash |
| **4 Vitals** | live HP / FP / stamina and their caps |
| **5 Flags** | the table's ChrDbg block: no dead, no damage, no FP consume, no stamina consume, no goods consume, no AI, plus per-character "disable gravity / map collision" |
| **6 Items** | 6,523 weapons, armour pieces, talismans, items, ashes of war and spells with their ids, equipped into a ChrAsm slot of your choice |
| **7 Process** | every matching game process, its Proton prefix, the saves found there, diagnostics, save backup |
| **8 Help** | the same information as text, including the permission fix |

Every screen reads values back from the process, so what you see is what the
game currently holds.

## Recommend

- Elden Ring started **with EasyAntiCheat disabled** (offline). The menu is for
  offline play; edited saves and online play do not mix, and EAC will block or
  ban the account.
- Permission to read another process's memory. On most distros the kernel's yama
  ptrace scope blocks this for a normal user, so either relax it for the session:

  ```sh
  sudo sysctl -w kernel.yama.ptrace_scope=0     # resets on reboot
  ```

  or run the menu itself with privileges:

  ```sh
  sudo -E eldencode
  ```

  Without this the attach fails with `EPERM` and the Process screen tells you so.
- Rust (only to build; the installer does it for you).

## Install

```sh
./install.sh
```

This does a fresh `cargo build --release`, installs into `~/.eldencode`
(`bin/eldencode`, `config.toml`, `backups/`), adds a marked `PATH` block to your
shell rc files and links the binary into `~/.local/bin` when that is usable.

```
  --home DIR     install somewhere else      --no-build   reuse target/release
  --link DIR     extra symlink               --uninstall  remove binary + PATH block
  --purge        with --uninstall, delete the whole home (guarded)
```

Manual build: `cargo build --release && ./target/release/eldencode`.

## Keys

```
1-8 / tab / shift-tab   switch screens          F5   attach or re-scan
up/down, k/j            move                    enter  edit / toggle / equip
a, m        (Runes)     add 1M / set max         b     back up save
/           (Items)     search                   x     clear search
left/right  (Items)     category                 s/S   change equip slot
t                       theme                    r     refresh values
q / ctrl-c              quit
```

Diagnostics without the UI: `eldencode --scan` prints every process that matches
`process` from the config, its image base and its Proton prefix, and
`eldencode --probe` attaches, resolves each symbol, prints the live values and
exits non-zero if a signature no longer matches.

## How it works (Linux + Proton)

Elden Ring on Linux is a Wine-hosted Windows exe, so `eldenring.exe` is an
ordinary Linux process. There is no Windows API here, no kernel driver and no
Cheat Engine:

1. **Discovery** — `/proc/*/comm` and `/proc/*/cmdline` are scanned for the exe
   name. The Proton prefix comes from `STEAM_COMPAT_DATA_PATH` in
   `/proc/<pid>/environ`, which is also where saves are found.
2. **Image base** — `/proc/<pid>/maps` gives the mapping of the exe; the lowest
   offset-0 mapping is the image base, the rest of the mappings are its size.
3. **Memory** — reads and writes go through `process_vm_readv` /
   `process_vm_writev`, with `/proc/<pid>/mem` as the fallback for partial
   region edges.
4. **Symbols** — the base pointers (`GameDataMan`, `WorldChrMan`, `GameMan`,
   `EventFlagMan`, `CHR_DBG`, `CHR_DBG_FLAGS`) are found by AOB-scanning the
   image with the same signatures and rip-relative fixups the cheat table uses.
5. **Fields** — each field is a Cheat Engine pointer chain (`GameDataMan → +8 →
   +0x6C` for held runes), resolved the same way CE does: dereference the
   global, walk the offsets from last to first, then add the first offset.

## Offsets and item ids

`src/offsets.rs` is transcribed from the **Hexinton All in One 8.0.4** Cheat
Engine table (game 1.17.1, table built 2026-09-10): the AOB list, the
`LocalPlayerOffset`/`CHRSP` defines, the `GameDataMan` / `WorldChrMan` chains,
the ChrDbg flag bytes and the ChrAsm equipment slots.

`assets/items.tsv` was extracted from the same table's `itemgibv2` dropdowns
(weapon, armour, talisman, goods, ashes of war lists), so every id in the Items
screen is an id that table would write.

When FromSoftware ships a patch, base pointers move. The AOB scan then misses and
the Overview/Process screens say which symbols failed instead of writing to a
guess.

## What it deliberately does not do

- **It cannot create a new inventory item.** That needs a call into the game's
  own item-give routine, which the Cheat Engine table does with a code cave and
  `CreateRemoteThread`-style injection. EldenCode writes equipment slots
  directly instead, so a weapon is in your hand without being owned.
- No teleport/warp, no param patching of weapon stats, no enemy editing. Those
  parts of the table need either injected code or per-patch param addresses.
- No save editing: values live in memory and are written to the save by the game
  when it saves (rest at a grace / quit).

## Safety

- Press **b** (or let `backup_on_attach` do it) to copy `ER0000.sl2` into
  `~/.eldencode/backups/<timestamp>/` before editing.
- Stay offline. Do not bring edited characters online.

## Layout

```
Cargo.toml            ratatui 0.29, crossterm 0.28, serde/toml, libc
install.sh            crux-style installer (fresh build, ~/.eldencode, PATH block)
assets/items.tsv      item catalog extracted from the cheat table
src/main.rs           CLI, terminal lifecycle
src/config.rs         ~/.eldencode paths and config.toml
src/process.rs        /proc discovery, image base, regions, Proton prefix, save backup
src/mem.rs            process_vm_readv/writev + /proc/<pid>/mem
src/aob.rs            Cheat Engine style AOB pattern scanner
src/offsets.rs        symbols, pointer chains, field tables
src/game.rs           attach, chain resolution, read/write, snapshot, runes, equip
src/items.rs          item catalog
src/app.rs            state, input, editing
src/ui/               theme, widgets, screens
```

## Credit

Feature set, AOB signatures and offsets are derived from **Hexinton's All in One
8.0.4** Cheat Engine table by Team Hexinton. EldenCode is not affiliated with
FromSoftware, Bandai Namco, Hexinton or Cheat Engine. For offline, single-player
use on saves you own.

MIT licensed.
