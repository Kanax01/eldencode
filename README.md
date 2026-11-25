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
d           (Scripts)   include DLC item lists
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
borrows one of the game's threads with `ptrace` for a single call.

Enter on a **weapon** opens an upgrade sheet: pick the level with `←`/`→` and
Enter gives it. The level lives in the item id (`Dagger` is row `1000000`, and
`+25` is sent as `1000025`), so the sheet is just the level added to the family
id. The amount still comes from `,`/`.`, and `esc` cancels.

Only the plain item is offered. The table's item list also spells out infused
variants (`1000100 Heavy Dagger`, `1000200 Keen Dagger`, … `1001200 Occult
Dagger`), but the game builds an infused weapon from an ash of war, not from an
id: sending one of those variants is what makes it fall over, so the item list
drops them at load and the sheet never shows them.

1. The AOB scans from the table resolve `InventoryAccessor` and `AddItemFunc`,
   and the inventory manager pointer is read the way the table's stub reads it
   (`[InventoryAccessor+0x19]` indexes the pointer at `InventoryAccessor+0x1D`).
2. The buffer lives in memory the game allocated for us, not on a stack. The
   table does the same (`alloc(itembuffer,4096,eldenring.exe)`) and only frees
   it a tick after the run, because the game keeps reading that struct once
   `AddItemFunc` returns. EldenCode builds the struct in the 4 KiB it borrows
   from the parked thread's own stack, below the stack pointer, and the restore
   never touches those bytes, so the struct is still there when the game reads
   it. (An earlier version asked the game for its own 4 KiB through
   `NtAllocateVirtualMemory`; that call was never reliable under Proton and
   could take the game down, so it is gone — nothing is called inside the game
   except the item routine itself.)
3. The item struct is filled from the table's `itembuffer` template: id at
   `+0x24`, quantity at `+0x28`, ash of war at `+0x30`. The id is
   `(base | type << 28) + level`, using the table's `{0,1,2,4,8}` type table, so
   `2010000` (Short Sword) at +5 is sent as `2010005`.
4. The call runs on a **parked** game thread, not the main one. The table runs
   `itemgib` on a thread it creates with `executeCode`, so the game's loop keeps
   running while the item is added — and that loop is what services the work the
   routine sets off. Stealing the main thread both blocks the loop and risks
   grabbing it while it holds a lock the routine needs. EldenCode picks a
   secondary thread that is asleep in a wait (`/proc/<tid>/stat` state `S`/`D`)
   and falls back to the main thread only if there is none.
5. That thread is attached, its registers and SSE/AVX state are saved, `RIP` is
   pointed at `AddItemFunc` with `rcx=manager, rdx=&buffer+0x20, r8=&buffer,
   r9=0`, and the return address is a `0xCC` byte found in the image, so the
   thread traps as soon as the routine returns.
6. Registers, extended state and the borrowed stack bytes are restored and the
   thread is released with signal 0. Nothing is written into executable memory
   and no code is injected, and the item buffer is never touched by the restore.

   The thread has to be one of the game's own Windows threads. Proton is Wine,
   and Wine also runs unix-side helper threads that have no TEB at all; the item
   routine reads the TEB through `gs:[0x30]`, so running on one of those
   segfaults on address `0x30` within a few milliseconds. EldenCode therefore
   attaches to a parked thread, reads its GS base, and releases it untouched when
   that is zero, moving on to the next candidate. A real fault stops the search
   instead of retrying. Every give is appended to
   `~/.local/share/eldencode/call.log` with the thread used, its GS base and what
   the routine returned.

Ids are checked against the game's own params before anything is sent. On the
first give EldenCode finds the tables in the game's memory: it walks the arena
the game loads them into (from the top, because they are allocated late),
recognises `EquipParamWeapon` by the `99999999` sentinel row every param ends
with, and then reads every param header near it, keeping the tables that hold
the ids it knows belong to each category. No offset out of the cheat table is
used for this — the game's own data decides, so it keeps working when the game
updates and the table's static offsets go stale (they already have).

A row id is the item without its upgrade level: a Keen Dagger is row `1000200`,
so `+5` is sent as `1000205` and checked as `1000200`. Anything that is not a
row is refused, which is what the fabricated weapon ids were:

```
the game has no item param for 0x7A86C8 — refusing to send it
```

So an id the table has no row for is refused instead of crashing the game, and
the item list only carries ids that have a plain row. If a category's table
cannot be found — goods ids in this build are spread over several params — the
check is skipped rather than blocking gives. `--params` prints the tables it found and checks a few ids, and only
reads memory.

Upgrade levels are capped per weapon from the table's own max-level list
(`assets/weapon_levels.tsv`, generated from it): `+25` for smithing weapons,
`+10` for somber ones, nothing for armour, talismans, items and spells. The cap
is applied where the item struct is built, so nothing can send a somber weapon
past +10 — that crashes the game. Consumables stack to 99 and nothing else
stacks, so quantities are clamped to 99 for items and 1 for everything else,
including in the bulk scripts.

## Bulk scripts

Screen 7 runs the table's "give everything" actions through the same item
routine, one call per item, redrawing a progress bar every few items (`esc`
stops a run, `enter` twice starts one). DLC items are **off by default** — press
`d` on that screen to include them, because DLC item ids only exist in the item
params when the expansion is actually loaded.

| Script | What arrives |
| --- | --- |
| all weapons & shields | 377 weapons and shields at +0 (479 with DLC) |
| all weapons & shields, maxed | the same list at its top level — +25 smithing, +10 somber |
| all armour | 561 helms, chest pieces, gauntlets and greaves (706 with DLC) |
| all talismans | every talisman, including each +1 / +2 variant (154 with DLC) |
| all talismans, strongest only | one per family, the highest +N variant |
| all sorceries | 70 sorceries |
| all incantations | 101 incantations |
| all ashes of war | 116 ashes, ready to apply at a grace |
| all crafting materials | 67 materials, 99 of each (106 with DLC) |
| all arrows & bolts | 64 kinds of ammunition, 99 of each (69 with DLC) |
| all consumables | 76 greases, pots, throwing items and food, 99 of each (119 with DLC) |
| all reusable items | 19 reusables, one of each (27 with DLC) |
| all upgrade materials | 39 smithing/somber stones and glovewort, 99 of each |
| all maps | 19 map fragments (24 with DLC) |
| all cookbooks | 59 cookbooks (104 with DLC), plus the 58 recipe flags |
| all whetblades | the 5 whetblades plus the whetstone knife |
| all talisman pouches | 3 talisman pouches, the base game's set |
| all memory stones | 8 memory stones, the base game's set |
| all multiplayer items | fingers, effigies, ciphers, taunter's tongue and 99 remedies |
| all wondrous physick items | the flask itself plus 18 crystal tears (19 with DLC) |
| perfume bottles | the ten empty perfume bottles the perfume arts refill from |
| max out flask number | tops the two flask allocations up to the 14-charge cap |
| max out flask charge | takes both flasks to +12 through the game's own tool swap |
| money bags | 99 of every rune item: golden, hero's, numen's, lord's |
| unlock torrent | flags Torrent as obtained and gives the spectral steed whistle |
| unlock roundtable | sets Melina's invitation flags so Roundtable Hold is open |
| reveal all maps | switches the map display on and reveals every map piece |
| kill all bosses | marks all 164 boss entries dead — rewrites quest progress |
| revive all bosses | marks them alive again, back in their arenas |
| unlock all graces | sets every grace flag — 71xxx, 72xxx, 73xxx and 76xxx |
| activate all summoning pools | switches on the whole 670xxx pool block |

Two of these work differently from the item scripts. **Kill/revive bosses** copies
the table's own "Revive/Kill ALL Bosses" entry: 164 byte offsets in
`assets/boss_flags.tsv` are written through `[[EventFlagMan]+0x28]`, `0xFF` for
dead and `0x00` for alive. One byte covers the group of flags a boss defeat
touches, which is why the table writes a byte and not a bit — and why a revive
is a byte write too. It is a memory write only, no code runs in the game, but it
does rewrite quest progress, so back the save up first and reload the area for
the change to show.

**Torrent, Roundtable and maps** set named flags through the game's own setter.
Torrent needs both halves — flag `60100` is the horse unlock (the game refuses to
summon him without it even when the whistle is in the inventory), `4680` marks
Melina's hand-off as done — plus the whistle item itself (goods `130`). The map
display is a second layer on top of the fragment items: the switches `62000`
(overworld), `62001` (underground) and `82001`/`82002`, and the piece flags
`62004`–`62012` and `62020`–`62084`. Map fragments only set some of those, which
is why giving the fragments alone can leave the map looking unchanged.

`all crafting materials` gives the crafting kit first when the game's crafting
flag (`60120`) is off, so the materials are actually usable. That flag is read
straight out of the event flag block, which holds one byte per flag id — the same
block the boss scripts write.

The flask scripts work on `playerGameData` (`[GameDataMan]+8`): the charges are
`EstusFlaskAllocateNum_byHp` and `_byMp` at `+0x101` / `+0x102`, and they add up
to the number of flasks carried. Fourteen is the cap Golden Seeds reach, so the
script only fills the difference (crimson first) and reports the split. Potency
is the flask level, which the game keeps at +12 once every Sacred Tear is used;
the upgrade script reports "nothing to change" when the flasks are already there.

The id lists in `assets/scripts.tsv` are the table's own bulk dropdowns, with a
per-row `dlc` flag so the base-game and DLC lists stay separate and duplicates
are dropped. DLC rows are skipped unless DLC mode is on: those ids are what make
a base-game install fall over when they are added. Long runs mean thousands of ptrace calls; the game stays
playable but expects a few seconds of hitching, and your inventory has finite
room — anything over the cap is simply not added.

The two block scripts work differently: they write the flag block directly,
one byte per id, the same write the table's boss entries use, so a whole block
lands in about a tenth of a second with no calls into the game. **Graces** covers
the four ranges the game gives them — `71000`–`71999` legacy dungeons,
`72000`–`72999` the DLC, `73000`–`73999` caves and catacombs, `76000`–`76999` the
open world. **Summoning pools** covers the `670000`–`670999` block every martyr
effigy flag lives in. A flag no grace or pool uses is a spare byte and does
nothing.

The table reaches the same two lists through the param repository
(`[base+0x3C80158]` → `+0xCA0`/`+0x30A0` → `+0x80` → `+0x80`, rows from `+0x40`
at a stride of `0x18`, flag at `row+4` for graces and at the row id for pools).
That base pointer is version specific: in this build `[base+0x3C80158]` lands in
a string, so the ranges above are used instead. Reading the params is the better
route when the pointer is valid — it only touches the flags that exist — but it
is not worth guessing at an offset for.

**Cookbooks** are the maps story again: the item does not unlock the recipes on
its own, the flags in the 67xxx/68xxx block do. The script sets the table's own
`Cookbook_Ids` list (58 unique flags after dropping its one typo `#27310`, which
belongs to the Armorer's `67310` row, and one repeat) before handing over the
books, so the recipes are cookable straight away.

The single-flag scripts (Torrent, Roundtable, maps) call the game's own setter,
found by the table's `EventFlag_C1` pattern rather than by the exported symbol,
which takes different arguments and writes nothing.

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
