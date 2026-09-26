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
  spawned again as the loader comes round). Test level `132` used to differ: its Lakitu's
  cloud had thrown two Spinies here and none in the emulator. The cloud throws when the
  frame counter's low seven bits are clear (`$01E98D`), and the loader's counter started
  at zero; it now starts at `$40` ([smw.md](smw.md)). Dumps live in `~/.local/share/kobo/oracle/`
  and are never committed. `trace_writes.lua` logs who writes a RAM address, for debugging.
  A level the game itself cannot load ends the script with `stuck in stage level (game
  mode $xx)` and a garbage mode: that, with the same routine failing in `expand`, is how a
  hack's own defect is told from a bug here (QLDC 2021 `34_idol`, nine slots). On the
  `expand` side, `KOBO_CPU_TRACE=<n>` prints the last `n` instructions of the CPU (either
  one, on an SA-1 ROM), with registers, before any routine fails, fatal or not.
  `KOBO_RAM_WATCH=$40D5D5` reports every write to a bus address with the instruction that
  made it, and the `n` before it when the trace is on: it found PIXI's offscreen routine
  erasing a sprite in `43_gui` on the SA-1. `KOBO_VRAM_WATCH=$3A1E` does the same for a
  VRAM word, naming the DMA source: it found the row updater behind Akogare2 `111`.
  A full run of the vanilla ROM takes 20 minutes and an SA-1 ROM emulates slower; four
  runs of 128 levels each into separate directories, moved together afterwards, take a
  quarter of that.
  `KOBO_ORACLE_VIDEO=1 dump.sh ...` instead waits for visible video and also writes PPM,
  full WRAM, and PPU state. Keep these later-frame captures in a separate directory. The
  screen buffer the script reads is a frame or two behind the PPU state, which of the two
  varying from run to run, so it waits for four frames at full brightness
  (`KOBO_ORACLE_VISIBLE_FRAMES=<n>` waits for another count, to see a level's first frames
  one by one); captures made before that wait was added can be a fade step darker than
  the render and match nothing.
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
  the time of the dump, and `43_gui` level `105` differs in slots 10 to 13 by the parity
  of the frame its sprites first ran on. The hack places sprites beyond the camera's right
  edge, which PIXI's offscreen routine (`$10FEDC`, one side per frame by `$13 & 1`) erases
  on even frames and the loader brings back; the dump's first sprite frame was odd and
  kept them all, the loader's here is even (`$13 = $40`, [smw.md](smw.md)) and one had gone
  before a spawner's child took its slot. Captures of the level one to twelve frames on
  (`KOBO_ORACLE_VISIBLE_FRAMES`) show the emulator erasing them from its second frame.
  Dumps live in `~/.local/share/kobo/oracle/hacks/`. With `KOBO_ORACLE_VIDEO=1` the same
  dumps give whole frames; of 27 levels of four hacks whose pictures changed when the
  faults below were fixed, the entry screen of 23 went from 10-56% of pixels matching to
  91-99.7% (Luminescent `148` only to 50%: an HDMA sky, see [known-gaps.md](known-gaps.md);
  the other three barely moved).
  What the hack dumps found, all in code no vanilla level runs: Lunar Magic's graphics
  upload reading work RAM back out of VRAM, `TM` written past its mirror, a game loop in a
  FastROM bank ([lunar-magic.md](lunar-magic.md)), and layer 2 left wherever level-init
  code put it ([smw.md](smw.md)).
- **Per-sprite comparison on hacks**: `examples/sprite_oracle.rs` takes a ROM and
  directories of `KOBO_ORACLE_VIDEO=1` dumps of it, renders each level with sprites, and
  scores the pixels each captured sprite entry's objects cover against the frame, where
  the entry is on the emulator's screen, along with the whole visible picture; the final
  table is per sprite number and extra bits, worst first. It exists to find a custom sprite
  drawn with the wrong graphics, colours, or not at all, which scores far below one that has
  merely animated or moved. Run on 2026-09-24 over 12 `dump_hack.sh` levels each of Akogare2,
  Grand Poo World 2, Luminescent, Invictus, Super Hark Bros 2, and QLDC 2021 `70_DPBOX`,
  `77_NerDose` and `44_Daizo Dee Von` (84 levels, 108 sprite scores; captures and
  scores in `~/.local/share/kobo/oracle/sprite-video/`), every custom sprite that scored
  under 85% was looked at side by side and found drawn as the emulator draws it, at its
  first-frame position (Akogare2 `008`'s piranha plant is up its stem, NerDose `003`'s
  mushrooms have fallen). The whole-picture scores under 85% are layer 2 parallax positions
  (Akogare2 `11A`, GPW2 `107`, DPBOX `102`), an HDMA sky (Luminescent `154`), a player still
  in his pipe in the frame (Invictus `152`, Luminescent `142`), and Invictus `030`'s layer 3
  fog. NerDose `104` scores its info box (`B9`) at 0% because the emulator's frame masks the
  main screen with a window where it and Mario stand (the dumped `windowMaskMain` has BG1,
  BG3 and objects on): the object is in the dumped OAM where the capture puts it. Daizo's
  capture got one level: the emulator stays in game mode `$14` on `026`, the cutscene level
  whose sprite waits for a button ([known-gaps.md](known-gaps.md)).
- Lunar Magic exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
  The fixtures were made with 3.21. Lunar Magic 3.70's `-ExportGFX` of the vanilla ROM
  differs in one file: `GFX17` has `$FF` where 3.21 and `gfx export` have `$00`, in 32
  bytes from offset `$11`. Regenerating the fixtures with 3.70 needs that explained first.
  3.70 ships a 64-bit build (`x64/Lunar Magic.exe`) that runs under 64-bit Wine alone.
- `tests/map16_sheet.rs` checks that the foreground Map16 a loaded vanilla level resolves
  (`LevelTiles::foreground_map16`, what `map16 png --level` draws) is the vanilla table of
  its tileset, and its background definitions the BG table. The sheet a level shows differs
  from the tileset's only where a tile references the animated area of VRAM (`$040`-`$07F`
  and the coin, water, and scenery frames), which the loaded level has as the first frame
  uploaded it, and in the palette entries the vanilla assembly leaves black (Mario's row).
- `tests/render_levels.rs` checks that vanilla level `105`'s dragon coins use the
  ROM's flashing yellow palette after the NMI, even with sprites and Mario hidden.
  It also changes the animation colours in an in-memory ROM copy to check that the
  capture follows those colours instead of substituting yellow in the renderer.
- **Decompression**: `tests/gfx_decompression.rs` decodes every GFX file of the pointer
  tables natively and has the ROM decompress the same file on the headless CPU
  (`expand::decompress_gfx_file`, the game's `PrepareGraphicsFile` with whatever routine a
  hack put behind it); the two must agree. It runs on the vanilla ROM and on `KOBO_LM_ROMS`,
  skipping locked ROMs. QLDC 2021 `34_idol` (a BPS patch) is the one LC_LZ3 hack in the
  corpus, so list it to exercise that decoder; all 50 of its files agree, as do those of
  LC_LZ2 hacks on LoROM, SA-1, and a 6 MiB SA-1 image. Lunar Magic's `-ExportGFX` of it
  agrees on all 52 files as well (`fixtures/lunar_magic_gfx_export.txt`, by ROM hash).
  Lunar Magic asks before it touches a headerless ROM, which a headless run never gets
  past: a patched BPS comes out headerless, so export from a copy with 512 zero bytes in
  front.
- **Compression**: `tests/lz2_compression.rs` recompresses the 52 vanilla GFX files with
  `compress::lz2::compress`, writes them over the originals in a copy of the ROM, and
  checks that the native decoder and the game's routine (the 50 table files through
  `decompress_gfx_file`, `GFX32` and `GFX33` through a load of level `105`) read them back,
  and that none is larger than Nintendo's (121,663 bytes against 130,317 in all). The unit
  tests check the parse against a brute-force search of every command, length, and source
  on small inputs.
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
  clobbered buffer or a table read from the wrong place without external data. The corpus
  is `~/.local/share/kobo/roms`: loose `.smc` hacks (Lunar Magic 1.62 to 3.33), QLDC 2021
  and 2022 as BPS patches, and `corpus_more/`, later downloads distributed as BPS (3.21 to
  3.51, among them the corpus's first 3.40 and 3.51 saves). `apply_bps.py` in that
  directory writes each patch's ROM next to it, headered, after checking every CRC the patch
  carries; `~/.config/kobo/env.sh` puts the loose ROMs and `corpus_more`'s in
  `KOBO_LM_ROMS`. All 512 levels of each `corpus_more` hack render without a fatal error
  (2026-09-25); none has Lunar Magic export hashes in the fixtures yet. Hacks whose
  headerless SHA-1 is in `fixtures/lunar_magic_map16_bg_export.txt` also have their BG table
  hashed against Lunar Magic's `-ExportAllMap16` output (file tile index `8000`-`81FF`).
  The rows checked are the 16 the game uploads from the layer 2 position, or from a
  position one row either side: the upload comes before the level loop's first camera
  update, which settles layer 2 by a few pixels in some levels.
  A 2026-09-22 rerun also found Akogare2 levels `0F8` (1792/1920 words) and
  `111` (21/2048 words) failing this tilemap check. Both reproduced at `9b69ab8`, before
  the input hardening and operation control. `0F8` passes since the loader runs the ROM's
  NMI at the frame boundaries (its picture was garbage without the upload the third
  blank does). `111` failed because the player pass kept every VRAM upload of its
  entrance frames, among them the rows Lunar Magic's row updater (`$1FA6xx`) uploads as
  the level's own code pans layer 2 upward from its second dozen frames, while the
  level's layer 2 position stayed the loader's; the pass now puts the tilemaps back
  ([smw.md](smw.md)), and an emulator frame of the level agreed with the expected words.
  The failure message lists the words that differ.
  Known exception in the corpus: `Smb2dx` (LM 1.63; 173 levels fail, its mode `$00`
  levels carry object layer 2 pointers), failing before the vertical-level checks were
  added. `Super Hark Bros 2` level `00A` used to fail with 896 of 2048 words: its
  level-init code leaves layer 2 at `$5D` and the game had uploaded for `$C0`, which the
  camera update `expand` now runs after preparation restores.
- **Project build**: `tests/project_build.rs` imports every vanilla level into a
  project, requires Kobo's formatting to be a fixed point on every file, builds it (twice,
  for byte-identical output), and requires every level to read back as vanilla's, its
  layer 1 data in the expanded ROM, and seven levels of different kinds to render the
  same picture. A build through the stage cache must equal one without, cold, warm, and
  after an edit. `a_synthetic_build_is_the_same_everywhere` needs no ROM: it builds
  `fixtures/synthetic_level.toml` onto `common::synthetic_base()` and pins the output's
  SHA-1, so CI shows whether all three platforms build the same bytes; a change to what a
  build writes changes the hash on purpose. The check of all 512 pictures is by hand, as for any change that should
  not change a picture:

  ```sh
  kobo import "$KOBO_SMW_ROM" /tmp/p --all && kobo build /tmp/p -o /tmp/built.sfc
  cargo run --release --example render_hashes -- "$KOBO_SMW_ROM" > vanilla.txt
  cargo run --release --example render_hashes -- /tmp/built.sfc > built.txt
  cmp vanilla.txt built.txt
  ```
- **Kobo's ROM-side code**: `tests/install.rs` applies `kobo_core::install`'s patches to
  vanilla (Asar's library needed) and runs the ROM: Map16 lookups for pages 0 and 1 as the
  game's, pages past 1 from tables written where Lunar Magic's layout points, and a few
  levels drawn as vanilla. After a change to a patch, compare `render_hashes` of all 512
  levels with vanilla's by hand (`kobo rom expand 1M`, then `kobo asm` each patch).
- **Block contact probe**: `tools/lunar-magic/block-probe/make-rom out/` builds vanilla
  saved once by Lunar Magic with GPS's logging probe block in level `105` (needs Wine,
  Lunar Magic, and GPS 1.4.4 built for the system in `KOBO_GPS`;
  `~/.local/share/kobo/tools/gps-1.4.4` here), and `cargo run --release --example
  contact_probe -- run out/probe.sfc` plays the level with the player placed against the
  block in each scenario and prints the actions that ran. The same run on a Kobo build with
  the same block must print the same. `contact_probe -- stand rom level x y tile...
  [addr=value...]` drops the player onto each tile and prints whether they landed and
  `$1693`, in any ROM.
- **A hack's content through Kobo's code**: transfer a hack into a Lunar Magic-saved
  vanilla ROM with Lunar Magic's command line (`-ImportMultLevels` of its MWL exports,
  `-ImportAllMap16`, `-ImportAllGraphics` of its `-ExportGFX`/`-ExportExGFX`,
  `-ImportSharedPalette`, `-TransferLevelGlobalExAnim`), then `tools/lunar-magic/with-kobo`
  swaps Kobo's bank `$06` code in, keeping the tables, and `render_hashes` and
  `ramdiff.py --summary` compare the two over all 512 levels. Do not import into a Kobo
  install instead: Lunar Magic's save then installs most of its own code over it
  ([lunar-magic-install.md](lunar-magic-install.md)). Kaizo Kindergarten passes.
- **Tool stages**: `tests/tool_stages.rs` builds two Asar patches, early and late, one
  including a file, onto the synthetic base when Asar's library is configured (no ROM):
  they apply in order, the output repeats, and changing the included file changes the
  build. With `KOBO_ADDMUSICK` (an AddmusicK folder; `~/src/addmusick` here) and the
  vanilla ROM, a project with an empty music folder gets AddmusicK's default music
  (`@AMK` at `$0E8000`), the same bytes twice.
- **UberASM Tool**: with `KOBO_UBERASM` (a folder with the program built for the platform
  and its files; `~/.local/share/kobo/tools/uberasm-x64` here, which needs
  `DOTNET_ROOT=~/.dotnet`), `tool_stages.rs` inserts one level's code, the same bytes twice.
- **SA-1 builds**: with `KOBO_SA1PACK` (`~/src/sa1pack` here), `tool_stages.rs` imports
  every level of the SA-1 base and builds it back as an SA-1 project, which must read back
  the same and render four levels the same. All 512 pictures, by hand: make the reference
  ROM as [sa1.md](sa1.md) says (`~/.local/share/kobo/roms/sa1/smw-sa1.sfc` here), `kobo
  import` it `--all`, `kobo build`, and compare `render_hashes` of the two.
- **Lunar Magic check**: `tools/lunar-magic/save-check built.sfc [level [project]]` has Lunar Magic
  3.70 save a copy of a ROM (exporting a level and importing it back) and runs `kobo diff`
  on the two: every level must read the same. A build of the whole vanilla import passes
  with levels `105` and `106`. The Lunar Magic features of step 2b are each to be checked
  this way.
- **Level data**: `tests/level_data.rs` decodes and encodes every level's object data,
  sprite list, and distinct background of the vanilla ROM and of every `KOBO_LM_ROMS` ROM
  but the locked ones, and requires the same objects, sprites, and tiles back, an encoding
  no longer than the stored one, and a stored length within the RATS block holding it. On
  vanilla all 538 object lists and 512 sprite lists but 18 object lists come out byte for
  byte ([smw.md](smw.md)). In the corpus, 70% to 100% of each ROM's lists do; the rest are
  Lunar Magic's encoding choices (every run on 2026-09-25 passed).
- **MWL files**: `tests/mwl_files.rs` reads Lunar Magic's MWL exports when `KOBO_MWL_DIR`
  is set, a directory of directories each holding one ROM (`.smc` or `.sfc`) and the MWL
  files of its levels, named `level NNN.mwl`. `tools/lunar-magic/export-mwl <outdir>
  rom...` makes them: it copies each ROM to `<outdir>/<name>/<name>.smc` and has Lunar
  Magic 3.70 `-ExportMultLevels` all 512 levels from the copy (flags 0), reporting a ROM
  it refuses. Every file must come back byte for byte from `MwlFile`, decode with the
  ROM's PIXI size table, encode to a file that decodes the same, and agree with the level
  in the ROM section by section, apart from the rewrites Lunar Magic makes on export
  ([lunar-magic.md](lunar-magic.md#mwl-files)), which the test counts per ROM. A ROM
  whose headerless SHA-1 is in `fixtures/lunar_magic_mwl_export.txt` must also have
  exactly the files recorded there (a SHA-1 of the 512 concatenated in level order); the
  test prints the line for one that is not. The export in `~/.local/share/kobo/mwl/`
  (190 MiB, never committed) covers the vanilla ROM and 41 loose and `corpus_more` hacks,
  every one Lunar Magic opens (it refuses the seven locked ROMs and Smb2dx); all 21,504
  files passed on 2026-09-25, in two seconds:

  ```sh
  tools/lunar-magic/export-mwl ~/.local/share/kobo/mwl ~/.local/share/kobo/roms/*.smc \
    ~/.local/share/kobo/roms/corpus_more/*.smc
  KOBO_MWL_DIR=~/.local/share/kobo/mwl cargo test --release --test mwl_files -- --nocapture
  ```
  `vanilla_exports_import_and_build` imports all 512 vanilla exports into one project
  with `import_mwl`, builds it, and allows only those rewrites in `kobo diff` against
  vanilla: the background of the 276 levels on the shared empty level, `0C5`'s header,
  and layer 1 of eleven levels.
- **SA-1**: the oracle script reads SA-1 Pack's RAM map (`ram()` in `dump_levels.lua` is
  `RamMap::Sa1Pack` for what it touches, and the full-WRAM dump is laid out as vanilla's)
  and hooks the pointer lookup on the SA-1 too, where the level loader runs. With
  `KOBO_ORACLE_ROM` on the reference ROM from [sa1.md](sa1.md), the tile grids of all 512
  levels, the layer 3 tilemaps, and the sprite slots match `sa1-all/`, and the 13 frames in
  `sa1-video/` match at 94.9-99.7%. `render_hashes` against the vanilla ROM is the other
  check: the marker column must match on every level but the three boss arenas, and the
  differences in the drawn column are SA-1 Pack's own ([sa1.md](sa1.md)). The corpus has
  40 SA-1 hacks: `Super Diagonal Mario 2`, `corpus_more`'s `Extended Interactions`, and 38
  QLDC 2021 and 2022 entries, which are BPS patches and have to be applied first. `render_hashes` on each says whether the code ran,
  not whether the pictures are right; what fails is in [known-gaps.md](known-gaps.md).
- **Picture hashes**: `cargo run --release --example render_hashes -- rom.smc` prints a SHA-1
  of every level's picture, with sprites drawn and again as markers without the player. A
  change to `expand` or `render` that should leave every picture alone is checked by diffing
  its output before and after, on the vanilla ROM and a handful of hacks from the corpus.
  It does not report nonfatal `LevelRender` diagnostics: a hash can describe a picture
  whose player or sprite passes failed. Use the CLI's warnings or inspect the returned
  diagnostics when checking execution coverage.

## Full hack render sweep

The 2026-09-22 sweep at revision `25cca50e847ee679c500e2a287ef3e71faf3322f` ran
`kobo level png` in release mode on every slot `000`–`1FF`, with default sprite and
player rendering. It recursively included the local hack collection's ROMs and all
129 BPS patches, including QLDC entries and development projects. All patches applied
successfully to the headerless vanilla ROM. Grouping identical headerless ROM SHA-1s
and excluding five unmodified vanilla copies left 173 distinct hacks.

All 88,576 slots were attempted: 88,522 PNGs, 54 fatal failures, and 209 PNGs with
warnings; no attempt reached the export script's 120-second timeout. The failures and
warnings affect ten hacks. Their status and investigation priorities are recorded in
[known-gaps.md](known-gaps.md#full-hack-render-sweep-2026-09-22). This was not an emulator
comparison or a visual review of every PNG, and includes slots that may not be playable.

The local, uncommitted output is `~/Pictures/Kobo-level-renders/2026-09-22/`:

- `manifest.json`: source paths, headerless ROM hashes, duplicate aliases, exclusions,
  and the renderer revision. Temporary patched-ROM paths no longer exist; reapply the
  source BPS patch when reproducing one of those entries.
- `results.jsonl`: every attempted slot's status and complete CLI diagnostics.
- `run-report.md`, `diagnostics.tsv`: per-hack totals and the failures and warnings.
- `index.html`: the PNG gallery, with diagnostic filters and optional filters for
  pictures identical to vanilla or to another slot in the same hack.
- `scripts/`: the one-off export, gallery, reporting and verification scripts. The
  export script starts a fresh run; use a separate output directory to keep this snapshot.

To reproduce an individual slot with its diagnostics, apply its patch if needed, then run:

```sh
cargo run --release -- level png 105 /tmp/kobo-level-105.png -r /path/to/hack.sfc
```

The export verification checked PNG headers and dimensions, preview presence, all
512 results per hack, and gallery links. These checks establish output completeness,
not correctness of the rendered game state.

## Strict runs

`cargo test --workspace` skips the ROM-backed tests when no vanilla ROM is configured, so
a green default run says nothing about ROM compatibility. `KOBO_REQUIRE_ROM=1` turns that
skip into a failure. A configured vanilla ROM must have the reference headerless SHA-1, a
malformed configuration is an error rather than a skip, and an opt-in tier whose variable
is set but names no ROMs, dumps, or frames fails instead of passing without checking
anything.

## Parser mutation checks

`tests/input_robustness.rs` runs 512 repeatable synthetic mutation cases in CI, covering
header size codes, mapped pointers, overflowing reads, truncated LC_LZ2, LC_LZ3, and
LC_RLE1 streams, sprite lists, object data, which must also encode back to the same
objects, and MWL files, a small valid one with bytes changed or cut short, which must
encode to a file that decodes the same, and round-trips noise and generated runs and
repeats through the LC_LZ2 compressor. The same generator runs for longer as an example:

```sh
cargo run --release --example fuzz_inputs -- 10000
cargo run --release --example fuzz_inputs -- 1 123   # reproduce a failing seed
```

This is deterministic mutation smoke fuzzing, not coverage-guided fuzzing, and it needs
no ROM. Targeted regressions separately cover out-of-file GFX pointers, invalid header
size codes, broken PIXI pointers, and unterminated sprite lists.
