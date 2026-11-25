# EldenCode

A terminal mod menu for **Elden Ring** on Linux, where the game runs through
Steam/Proton. It gives you runes, edits attributes and vitals, flips the game's
debug flags, adds any item to your inventory at the upgrade level you pick, and
slots weapons, armour and talismans straight onto your character — all from a
ratatui interface in your terminal.

```
 ELDENCODE   elden ring mod menu · Proton aware
╭──────────────────────────────────────────────────────────────────────────╮
│ ▸1 Overview  2 Runes  3 Attributes  4 Vitals  5 Flags  6 Items  …        │
╰──────────────────────────────────────────────────────────────────────────╯
╭ session ─────────────────────────╮╭ tarnished ───────────────────────────╮
│ ● attached  eldenring.exe        ││ Name        Melina                   │
│ Process id      48213            ││ Level       137   runes 4200318      │
│ Image base      0x140000000      ││ HP       ████████████░░░░  1450/1780 │
│ Symbols         9/9 resolved     ││ FP       ██████████████░░   220/240  │
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
| **6 Items** | sub-tabs per item type over 6,523 ids (weapons, armour, talismans, items, ashes of war, spells); `enter` gives the item to your inventory at the chosen upgrade level and quantity, `e` equips it into a ChrAsm slot of your choice |
| **7 Scripts** | bulk actions that run the give-routine once per item: all weapons (plain or maxed), all armour, all talismans (every variant or one strongest per family), all sorceries, all incantations, all ashes of war, every crafting material at 500 each, a stack of every rune item — with a progress bar and `esc` to stop |
| **8 Process** | every matching game process, its Proton prefix, the saves found there, diagnostics, save backup |
| **9 Help** | the same information as text, including the permission fix |

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
1-9 / tab / shift-tab   switch screens          F5    attach or re-scan
up/down, k/j            move                    enter give item (Items) / edit
a, m        (Runes)     add 1M / set max         b     back up save
/           (Items)     search                   x     clear search
left/right  (Items)     item type (sub-tabs)     s/S   change equip slot
- and +     (Items)     upgrade level            ,/.   quantity
e           (Items)     equip into the slot      t     theme
enter       (Scripts)   run (enter twice)         esc   cancel / stop
q           (anywhere)  quit, also during a run
r                       refresh values           q     quit (ctrl-c too)
```

Diagnostics without the UI: `eldencode --scan` prints every process that matches
`process` from the config, its image base and its Proton prefix, and
`eldencode --probe` attaches, resolves each symbol, prints the live values and
exits non-zero if a signature no longer matches. `eldencode --give <id>` adds one
item from the command line — `--give 2010000 --level 5 --qty 2` is a Short Sword
+5, twice — which is also the quickest way to check the spawn path after a patch.

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

## Giving items

The Items screen adds real inventory items. On Windows the table does this with a
code cave and `CREATETHREAD`; Linux has no remote-thread call, so EldenCode
borrows the game's own main thread with `ptrace` for a single call:

1. The AOB scans from the table resolve `InventoryAccessor` and `AddItemFunc`,
   and the inventory manager pointer is read the way the table's stub reads it
   (`[InventoryAccessor+0x19]` indexes the pointer at `InventoryAccessor+0x1D`).
2. The item struct is filled from the table's `itembuffer` template: id at
   `+0x24`, quantity at `+0x28`, ash of war at `+0x30`. The id is
   `(base | type << 28) + level`, using the table's `{0,1,2,4,8}` type table, so
   `2010000` (Short Sword) at +5 is sent as `2010005`.
3. The main thread is attached, its registers and SSE/AVX state are saved, `RIP`
   is pointed at `AddItemFunc` with `rcx=manager, rdx=&buffer+0x20,
   r8=&buffer, r9=0`, and the return address is a `0xCC` byte found in the
   image, so the thread traps as soon as the routine returns.
4. Registers, extended state and the borrowed stack bytes are restored and the
   thread is released with signal 0. Nothing is written into executable memory
   and no code is injected.

Upgrade levels are capped per item: `+25` for smithing weapons, `+10` for somber
ones, nothing for armour, talismans, items and spells.

## Bulk scripts

Screen 7 runs the table's "give everything" actions through the same item
routine, one call per item, redrawing a progress bar every few items (`esc`
stops a run, `enter` twice starts one):

| Script | What arrives |
| --- | --- |
| all weapons & shields | 479 weapons and shields, base game + DLC, at +0 |
| all weapons & shields, maxed | the same list at its top level — +25 smithing, +10 somber |
| all armour | 706 helms, chest pieces, gauntlets and greaves |
| all talismans | every talisman, including each +1 / +2 variant |
| all talismans, strongest only | one per family, the highest +N variant |
| all sorceries | 70 sorceries |
| all incantations | 101 incantations |
| all ashes of war | 116 ashes, ready to apply at a grace |
| all crafting materials | 106 materials, 500 of each |
| all arrows & bolts | 69 kinds of ammunition, 500 of each |
| all consumables | 119 greases, pots, throwing items and food, 500 of each |
| all reusable items | 27 reusables, one of each |
| all upgrade materials | 39 smithing/somber stones and glovewort, 999 of each |
| all maps, cookbooks & whetblades | 134 map fragments, cookbooks and whetblades |
| money bags | 99 of every rune item: golden, hero's, numen's, lord's |
| unlock all graces | sets the event flag of every grace, base game + DLC |
| activate all summoning pools | switches on every martyr effigy pool |

The id lists in `assets/scripts.tsv` are the table's own bulk dropdowns (base and
DLC lists merged, duplicates dropped), so each script adds the same set its
counterpart does. Long runs mean thousands of ptrace calls; the game stays
playable but expects a few seconds of hitching, and your inventory has finite
room — anything over the cap is simply not added.

The two flag scripts work differently: they walk a param list through the param
repository (`[base+0x3C80158]` → list offset → `+0x80` → `+0x80`, rows from
`+0x40` at a stride of `0x18`), then call the game's event-flag setter once per
row with `rcx` = flag manager, `rdx` = pointer to the id, `r8b` = 1 — the same
stub the table's unlock scripts use. Rows are validated before anything is
written (sane count, plausible flag ids) and the run aborts on the first
surprise.

When FromSoftware ships a patch, base pointers move. The AOB scan then misses and
the Overview/Process screens say which symbols failed instead of writing to a
guess.

## What it deliberately does not do

- **No item removal or inventory editing.** Items are added through the game's
  own routine; changing or deleting what you already own would need the
  inventory struct's layout, which the table does not expose.
- **No injected code.** Equipment slots are written directly as well, so `e`
  puts a weapon in your hand even if you never gave it to yourself.
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
assets/scripts.tsv    id lists behind the bulk scripts (base + DLC, deduped)
src/main.rs           CLI, terminal lifecycle
src/config.rs         ~/.eldencode paths and config.toml
src/process.rs        /proc discovery, image base, regions, Proton prefix, save backup
src/mem.rs            process_vm_readv/writev + /proc/<pid>/mem
src/aob.rs            Cheat Engine style AOB pattern scanner
src/offsets.rs        symbols, pointer chains, field tables
src/game.rs           attach, chain resolution, read/write, snapshot, runes, equip, give_item
src/spawn.rs          ptrace remote call into the game's item routine
src/scripts.rs        bulk script lists and their resolution
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
