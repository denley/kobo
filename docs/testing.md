# Oracles and corpus checks

The tiers themselves (unit, ROM-backed, opt-in oracles), where the vanilla ROM is looked up, and
its reference hash are in `AGENTS.md` under "Test tiers and ROM configuration". This file is
how each oracle is produced, where its data lives, and what is known not to match.

- **Emulator oracle** (`tools/oracle/`): `dump.sh <rom> <outdir> 105,106,...` runs Mesen 2
  headlessly, navigates to each level through the file select, and dumps the tile grid,
  CGRAM, VRAM, and header RAM on the first level frame. `tests/oracle_levels.rs` compares
  `expand::expand_level` against a dump directory when `KOBO_ORACLE_DIR` is set; all 512
  vanilla levels match byte for byte. Dumps live in `~/.local/share/kobo/oracle/`
  and are never committed. `trace_writes.lua` logs who writes a RAM address, for debugging.
  `KOBO_ORACLE_VIDEO=1 dump.sh ...` instead waits for visible video and also writes PPM,
  full WRAM, and PPU state. Keep these later-frame captures in a separate directory.
  `KOBO_BOSS_ORACLE_DIR` enables stable boss graphics comparisons for levels 096, 0CC,
  0D9, and 1C7 (Mode 7 characters, layer 3 GFX, arena tilemap, SP3); either capture mode
  works. The loader override must only run in game mode `$11`: overriding the
  title-screen load in mode `$03` contaminates the graphics cache, and dumps made before
  that guard fail the boss comparison.
  `tests/video_oracle.rs` compares whole rendered pictures (with sprites) against the
  PPM frames of `KOBO_ORACLE_VIDEO=1` dumps listed in `KOBO_VIDEO_ORACLE_DIRS`
  (`:`-separated), cropped at the camera from the dumped WRAM (`$1A`/`$1C`; the PPU
  scroll registers keep ten bits, too few for vertical levels) and below the status bar.
  Mesen's frame is 239 lines with the picture 6 rows down; the test tries paddings 4-9.
  Agreement is 95-99.9% on most levels; tides that have moved and sprites that have
  animated account for the rest, and the threshold is 85%. Captures live in
  `~/.local/share/kobo/oracle/layer3-video/` and `colormath-video/`. In the `1D4` and
  `1D9` captures the picture shows no OAM object at all (no Mario, no candle flames)
  although the dumped OAM holds them where the player pass puts them; treat those
  two frames' object layer as unreliable.
- Lunar Magic exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
- **CPU suite**: `tests/cpu_single_step.rs` runs the 65816 core against SingleStepTests
  (10,000 native-mode tests per opcode, about a second in release) when `KOBO_65816_TESTS`
  points at the suite's `v1` directory. The native files are in
  `~/.local/share/kobo/cpu-tests/65816/v1` (sparse clone of `SingleStepTests/65816`, 1.7 GiB,
  no licence, never committed). `BRK`, `COP`, `WAI`, and `STP` are left out (the core stops
  on them by design), and the suite's block moves are cut off after 100 cycles, so those
  are compared over the bytes moved. All other opcodes pass in full. SMW itself never sets
  decimal mode; custom code may.
- **Lunar Magic hacks**: `tests/layer2_background.rs` runs on every ROM listed in `KOBO_LM_ROMS`
  (`:`-separated paths) as well as the vanilla ROM. It rebuilds the layer 2 tilemap the game
  uploaded to VRAM from the captured background buffer and BG Map16 table, which catches a
  clobbered buffer or a table read from the wrong place without external data. Hacks whose
  headerless SHA-1 is in `fixtures/lunar_magic_map16_bg_export.txt` also have their BG table
  hashed against Lunar Magic's `-ExportAllMap16` output (file tile index `8000`-`81FF`).
  Known exceptions in the corpus: `Smb2dx` (LM 1.63; 173 levels fail, its mode `$00`
  levels carry object layer 2 pointers) and `Super Hark Bros 2` level `00A` (mode `$0C`,
  896 of 2048 words), both failing before the vertical-level checks were added.
- **SA-1**: there is no emulator oracle yet (`dump_levels.lua` reads `$7E` addresses). The
  check is the reference ROM from [sa1.md](sa1.md) against the vanilla ROM with
  `render_hashes`: the marker column must match on every level but the three boss arenas,
  and the known differences in the drawn column are listed in
  [known-gaps.md](known-gaps.md). The corpus has 39 SA-1 hacks: `Super Diagonal Mario 2`
  and 38 QLDC 2021 and 2022 entries, which are BPS patches and have to be applied first.
  `render_hashes` on each says whether the code ran, not whether the pictures are right: 33
  render all 512 levels, and the rest are in [known-gaps.md](known-gaps.md).
- **Picture hashes**: `cargo run --release --example render_hashes -- rom.smc` prints a SHA-1
  of every level's picture, with sprites drawn and again as markers without the player. A
  change to `expand` or `render` that should leave every picture alone is checked by diffing
  its output before and after, on the vanilla ROM and a handful of hacks from the corpus.
