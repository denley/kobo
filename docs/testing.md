# Oracles and corpus checks

The tiers themselves (unit, ROM-backed, opt-in oracles), where the vanilla ROM is looked up, and
its reference hash are in `AGENTS.md` under "Test tiers and ROM configuration". This file is
how each oracle is produced, where its data lives, and what is known not to match.

- **Emulator oracle** (`tools/oracle/`): `dump.sh <rom> <outdir> 105,106,...` runs Mesen 2
  headlessly, navigates to each level through the file select, and dumps the tile grid,
  CGRAM, VRAM, header RAM, and the sprite slot tables on the first level frame. The script
  zeroes work RAM, save RAM, video memory, and an SA-1's I-RAM before the game starts, as
  `expand`'s machine starts, whatever power-on state Mesen is set to (random by default):
  dumps repeat byte for byte, VRAM included, and hacks that read memory they never wrote
  behave the same each run.
  `tests/oracle_levels.rs` compares `expand::expand_level` against a dump directory when
  `KOBO_ORACLE_DIR` is set; the dumps are of the vanilla ROM unless `KOBO_ORACLE_ROM` names
  another. All 512 vanilla levels match byte for byte in the tile grid, and in which sprite
  is in which slot (the sprite and its place, not whether it is alive: the dump comes a
  frame or so into the level, and a sprite spawned beyond the despawn range is erased and
  spawned again as the loader comes round). Test level `132` is the exception: its Lakitu
  has thrown two Spinies by the end of preparation here and none in the emulator, for a
  reason not found. Dumps live in `~/.local/share/kobo/oracle/`
  and are never committed. `trace_writes.lua` logs who writes a RAM address, for debugging.
  A full run of the vanilla ROM takes 20 minutes and an SA-1 ROM emulates slower; four
  runs of 128 levels each into separate directories, moved together afterwards, take a
  quarter of that.
  `KOBO_ORACLE_VIDEO=1 dump.sh ...` instead waits for visible video and also writes PPM,
  full WRAM, and PPU state. Keep these later-frame captures in a separate directory. The
  screen buffer the script reads is a frame or two behind the PPU state, which of the two
  varying from run to run, so it waits for four frames at full brightness; captures made
  before that wait was added can be a fade step darker than the render and match nothing.
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
  `~/.local/share/kobo/oracle/layer3-video/`, `colormath-video/`, and `entry-video/` (the
  levels SA-1 Pack changes, the boss arenas, and vertical levels `0DB`, `12A`, and `1ED`).
  In the `1D4` and `1D9` captures the picture shows no OAM object at all (no Mario, no
  candle flames) although the dumped OAM holds them where the player pass puts them; treat
  those two frames' object layer as unreliable. A vanilla boss arena's frame sometimes comes
  without its background or objects (`098` or `0D9`, one or the other from run to run) and
  then falls just under the threshold; the SA-1 ROM's do not.
- **Emulator oracle on hacks**: `dump_hack.sh <vanilla> <hack> <outdir> [count]` picks the
  levels whose layer 1 pointer is not vanilla's and dumps `count` (12) of them, spread over
  the hack; `KOBO_ORACLE_ROM=<hack> KOBO_ORACLE_DIR=<outdir>` then runs
  `tests/oracle_levels.rs` on it. The script gets into a level in every hack of the corpus:
  besides the vanilla title screen and file select it handles a hack that boots straight
  into a level (`RHRS1C`), one that skips the intro level and starts on the overworld (it
  presses A on the level the player stands on: Super Diagonal Mario 2, Super Sheffy World
  2), and a "No Yoshi" intro that the ROM's tables would not predict (Grand Poo World 2
  plays it before levels of any tileset), which is why it watches the game choose the intro
  instead of predicting it. When it gives up it writes `stuck.ppm`, the screen it was on.
  On 12 levels of each of the 42 `.smc` hacks the tile grids and layer 3 tilemaps all match,
  and the sprite slots in 38; the rest are sprites already moving at the dump (`apes1.13`
  `02E`, Luminescent `103`, `SMW_2021-4-24` `0C5`/`1C5`: 8 to 10 pixels on, where the test
  allows 4) and one the emulator has not spawned yet (Super Hark Bros 2 `138` slot 5).
  The 38 SA-1 entries of QLDC 2021 and 2022 (BPS patches, applied first) were dumped the
  same way, six levels each: 37 get into a level (`28_Kitikuchan`'s title screen is a room
  to play through), and 35 of those match throughout. `61_Wakana_Sariel` level `13B`
  differs in the layer 3 tilemap, which its per-frame status bar code has drawn into by
  the time of the dump, and `43_gui` level `105` has its custom sprites `99` and `91` one
  slot further on than the emulator.
  Dumps live in `~/.local/share/kobo/oracle/hacks/`. With `KOBO_ORACLE_VIDEO=1` the same
  dumps give whole frames; of 27 levels of four hacks whose pictures changed when the
  faults below were fixed, the entry screen of 23 went from 10-56% of pixels matching to
  91-99.7% (Luminescent `148` only to 50%: an HDMA sky, see [known-gaps.md](known-gaps.md);
  the other three barely moved).
  What the hack dumps found, all in code no vanilla level runs: Lunar Magic's graphics
  upload reading work RAM back out of VRAM, `TM` written past its mirror, a game loop in a
  FastROM bank ([lunar-magic.md](lunar-magic.md)), and layer 2 left wherever level-init
  code put it ([smw.md](smw.md)).
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
  The rows checked are the 16 the game uploads from the layer 2 position, or from a
  position one row either side: the upload comes before the level loop's first camera
  update, which settles layer 2 by a few pixels in some levels.
  Known exception in the corpus: `Smb2dx` (LM 1.63; 173 levels fail, its mode `$00`
  levels carry object layer 2 pointers), failing before the vertical-level checks were
  added. `Super Hark Bros 2` level `00A` used to fail with 896 of 2048 words: its
  level-init code leaves layer 2 at `$5D` and the game had uploaded for `$C0`, which the
  camera update `expand` now runs after preparation restores.
- **SA-1**: the oracle script reads SA-1 Pack's RAM map (`ram()` in `dump_levels.lua` is
  `RamMap::Sa1Pack` for what it touches, and the full-WRAM dump is laid out as vanilla's)
  and hooks the pointer lookup on the SA-1 too, where the level loader runs. With
  `KOBO_ORACLE_ROM` on the reference ROM from [sa1.md](sa1.md), the tile grids of all 512
  levels, the layer 3 tilemaps, and the sprite slots match `sa1-all/`, and the 13 frames in
  `sa1-video/` match at 94.9-99.7%. `render_hashes` against the vanilla ROM is the other
  check: the marker column must match on every level but the three boss arenas, and the
  differences in the drawn column are SA-1 Pack's own ([sa1.md](sa1.md)). The corpus has
  39 SA-1 hacks: `Super Diagonal Mario 2` and 38 QLDC 2021 and 2022 entries, which are BPS
  patches and have to be applied first. `render_hashes` on each says whether the code ran,
  not whether the pictures are right; what fails is in [known-gaps.md](known-gaps.md).
- **Picture hashes**: `cargo run --release --example render_hashes -- rom.smc` prints a SHA-1
  of every level's picture, with sprites drawn and again as markers without the player. A
  change to `expand` or `render` that should leave every picture alone is checked by diffing
  its output before and after, on the vanilla ROM and a handful of hacks from the corpus.
