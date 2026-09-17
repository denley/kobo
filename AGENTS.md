# Kobo

An open-source Super Mario World ROM editor and build system.
Desktop app for Windows, Linux, and macOS.
Early stage: roadmap step 1 is in progress.

## Principles

1. **Open source.** The community must be able to read, fork, and customise everything.
2. **ROMs are compiled from source.** A project is a directory of level files, sprites, blocks, ASM,
   graphics, overworld definitions, etc, and a manifest. The build takes a clean SMW ROM plus that
   directory and produces a ROM. Builds are repeatable and byte-identical for identical inputs.
   Projects are git-friendly.
3. **Everything is scriptable.** Every operation lives in a core library. The GUI and the CLI are
   thin shells over it. Batchable operations (build, import, export, validate, diff, render) are
   exposed on the CLI. Interactive editing is a GUI concern; programmatic editing goes through a
   scripting API, not CLI flags.
4. **Compatibility with existing tools**. The built ROM must use the same
   hijacks, tables, and data layouts that Lunar Magic installs, so PIXI, GPS, UberASM Tool, AddmusicK,
   Asar patches, and custom sprite display files (.ssc, .mwt, .mw2, .s16) keep working. Importing an 
   existing Lunar Magic hack (MWL, Map16, ExGFX, palettes) is a first-class feature. Exceptions may be
   tolerated but heavily discouraged and scrutinised.

## Guardrails

- **Reuse the open-source toolchain.** Orchestrate Asar, PIXI, GPS, UberASM Tool, and AddmusicK.
  Do not reimplement them. Pin tool versions.
- **Patch-based, not disassembly-based.** the build patches a vanilla ROM. Do not build on a full
  SMW disassembly (e.g. SMWDisX); it breaks every fixed-address patch in the ecosystem. Keep the
  architecture from precluding it, but do not pursue it.
- **The vanilla ROM never lives in a project.** Reference it by hash. Locate it through per-user
  config or an environment variable. Never commit or distribute ROM data. Support BPS patch output.
- **Canonical source formats are textual and merge-friendly.** One object or sprite per line where
  practical. Binary formats (MWL, Map16 exports, raw GFX) are import/export interop only.
  Round-trip fidelity against Lunar Magic exports is tested, not assumed.
- **ROM-side code is source.** Anything the editor installs into the ROM (level format expansion,
  custom sprite loading, and so on) is an Asar patch checked into this repo.
- **Determinism is a requirement.** Fixed tool order, deterministic freespace allocation, pinned
  versions. Design for incremental builds; AddmusicK is slow.
- **Support SA-1 from the start.** Address mapping (LoROM, SA-1, FastROM) is an abstraction, never
  hard-coded.
- **SMW only.** Keep game facts data-driven inside the library, but do not build a game-agnostic
  engine.
- **Compatibility comes from documented formats.** Base Lunar Magic compatibility on the ROM and
  file formats documented by the community (SMWCentral). We can inspect ROM outputs from
  Lunar Magic, but do not disassemble or reverse-engineer the Lunar Magic executable.
- **Scope discipline.** Do not chase Lunar Magic feature parity before shipping something usable.
- **License is MPL-2.0 across the board.** Application, core library, CLI, and ROM-side patches.
  New dependencies must be MPL-compatible; check each tool's license before adopting it.

## Roadmap (in order)

1. Core library and CLI that reads a vanilla or Lunar-Magic-modified ROM and renders any level to
   PNG. Validate parsers against real hacks.
2. Build pipeline with native level, Map16, ExGFX, and palette insertion in the Lunar Magic layout,
   plus MWL import. Lunar Magic users can adopt the build while still editing in Lunar Magic.
3. GUI level editor.
4. Overworld, Layer 3, graphics and palette editing, emulator integration
   (play-from-level, Mesen-S / bsnes-plus debugging).

## Stack and layout

- **Rust** (pinned in `rust-toolchain.toml` and `mise.toml`), edition 2024, cargo workspace.
  - `crates/kobo-core`: the library. All logic lives here.
  - `crates/kobo-cli`: the `kobo` binary. Thin shell over the core; no logic of its own.
- `kobo_core::addr` is the only place that knows how SNES addresses map to file offsets.
  Every ROM read takes a `SnesAddr` and goes through the ROM's `Mapping` (LoROM or SA-1).
  Conversions mirror Asar's conventions so addresses agree with the rest of the toolchain.
- `kobo_core::rom::Rom` strips and remembers the 512-byte copier header; `data()` is always
  headerless. Identity is by SHA-1 of the headerless image.

## Commands

```
cargo build --workspace
cargo test --workspace                       # ROM-backed tests skip if no ROM is configured
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -- rom info [-r rom]               # header, checksum, hash, identity
cargo run -- gfx list|export|png [-r rom]    # GFX files: table, LM-layout .bin export, tile sheet
cargo run -- level info 105                  # primary header and data pointers
cargo run -- palette png --level 105 out.png # 16x16 swatch of the assembled level palette
cargo run -- map16 png --level 105 out.png   # all 0x400 Map16 tiles in colour
cargo run -- level png 105 out.png           # render a level by running the ROM's own loader
cargo run -- level tiles|dump 105 [dir]      # the expanded Map16 grid as hex, or raw planes
cargo run -- level sprites|map16|wram|reads  # sprite list, resolved Map16, RAM dump, read trace
cargo run -- addr '$05E000' [--sa1]          # SNES <-> file offset
```

Lunar Magic runs headlessly under Wine for reference exports, e.g.
`wine "Lunar Magic.exe" -ExportGFX rom.smc` (also `-ExportAllMap16`, `-ExportSharedPalette`,
`-ExportLevel`). Always run it on a copy of the ROM. Export hashes, never the exported bytes,
go in `crates/kobo-core/tests/fixtures/`.

CI (`.github/workflows/ci.yml`) runs fmt, clippy with warnings denied, and tests on Linux,
Windows, and macOS. Keep all three green.

## Test tiers and ROM configuration

- **Unit tests** use synthetic data and always run.
- **ROM-backed tests** (`crates/kobo-core/tests/`) load the vanilla ROM through
  `kobo_core::config::vanilla_rom_path()`: the `KOBO_SMW_ROM` env var, else `roms.smw` in
  `$XDG_CONFIG_HOME/kobo/config.toml`. They print `skipping: ...` and pass when no ROM is
  configured, so CI never needs ROM data. Run them locally before pushing.
- The vanilla reference is No-Intro "Super Mario World (USA)", headerless SHA-1
  `6b47bb75d16514b6a476aa0c73a683a2a4c18765`, checksum `$A0DA`.
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
- Lunar Magic exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
- **Lunar Magic hacks**: `tests/layer2_background.rs` runs on every ROM listed in `KOBO_LM_ROMS`
  (`:`-separated paths) as well as the vanilla ROM. It rebuilds the layer 2 tilemap the game
  uploaded to VRAM from the captured background buffer and BG Map16 table, which catches a
  clobbered buffer or a table read from the wrong place without external data. Hacks whose
  headerless SHA-1 is in `fixtures/lunar_magic_map16_bg_export.txt` also have their BG table
  hashed against Lunar Magic's `-ExportAllMap16` output (file tile index `8000`-`81FF`).

## Reference material

- `~/.local/share/kobo/docs/smwdisx/`: the SMWDisX disassembly banks and `SMW_U.sym` (downloaded
  from GitHub, not committed). Use it to read how the game consumes a table; never build on it.
  SMW Central is behind a JavaScript challenge and cannot be fetched from tools.
- Asar 1.91 built from source: `~/.local/bin/asar`, `libasar.so` in `~/.local/lib`.
- Mesen 2: `~/.local/share/kobo/tools/mesen2/Mesen`, built from source against the system
  libstdc++ (`tools/mesen-src/`; .NET SDK in `~/.dotnet`). The official 2.1.1 binary in
  `tools/mesen/` bundles GCC 12's libstdc++ and aborts with `std::bad_cast` at startup about
  half the time; do not use it. Headless use is `Mesen --testRunner script.lua rom.sfc
  --timeout=N`; the script ends with `emu.stop(code)`. Lua enums are lower-camel-cased C++
  names: `emu.memType.snesMemory`, `emu.eventType.endFrame`. Scripts need
  `Debug.ScriptWindow.AllowIoOsAccess` and a controller on `Snes.Port1` in
  `~/.config/Mesen2/settings.json`.

## SMW facts worth remembering

- Level pointer tables: layer 1 at `$05E000` and layer 2 at `$05E600` hold 3-byte pointers,
  0x200 levels each. Sprite pointers at `$05EC00` are 2 bytes each, implicitly bank `$07`.
- A layer 2 pointer with bank `$FF` marks a background tilemap; the game substitutes bank `$0C`.
- Level 105 (Yoshi's Island 1) layer 1 data starts at `$0688DD` in vanilla.
- GFX file pointers for `GFX00`-`GFX31`: low bytes at `$00B992`, high at `$00B9C4`, bank at
  `$00B9F6`. Lunar Magic keeps these tables and rewrites the entries (often into FastROM
  banks `$80+`). Data is LC_LZ2. Vanilla files are 3bpp (2bpp for `27`-`2B`, `2F`); Lunar
  Magic re-inserts files as 4bpp, so bit depth is inferred from decompressed size and the
  file's fixed tile count (128, except `2F`-`31` = 64). `GFX27` is not planar tiles at any depth
  and is treated as raw bytes; its layout is unknown.
- The game's `UploadGFXFile` sets the fourth plane to the tile silhouette for the first 16x16
  block of `GFX01`/`17`/`31` (the berry, drawn with colours 9-F) and for all of `GFX1E` (and
  `GFX08` in tilesets `$11+`). Lunar Magic's export mirrors this except it skips `17` and flags a
  fixed subset of `08`; see `gfx::upper_palette_tiles` vs `gfx::vram_upper_palette_tiles`.
  `GFX32`/`GFX33` are stored differently and are not handled.
- GFX lists: `$00A92B` object tilesets (FG1, FG2, BG1, FG3), `$00A8C3` sprite tilesets (SP1-4),
  4 bytes per row, 26 rows. VRAM: FG1/FG2/BG1/FG3 at 8x8 tiles `$000`/`$080`/`$100`/`$180`;
  SP1-4 at word `$6000`/`$6800`/`$7000`/`$7800`; layer 3 `GFX28`-`2B` at word `$4000`.
  The layer 3 files are uploaded only by `CODE_00A993` on the "Nintendo Presents" screen
  (and the ending) and survive every level load, so `expand` runs `ClearOutLayer3` and
  that routine once after reset; the sprite marker font depends on it.
- Palette: `LoadPalette` (`$00ABED`) fills CGRAM (`$7E0703`) from `$00B0A0` (back area),
  `$00B0B0` (BG, rows 0-1 cols 2-7, `$18` bytes each), `$00B170` (rows 0-1 cols 8-F),
  `$00B190` (FG, rows 2-3 cols 2-7), `$00B250` (rows 4-D cols 2-7), `$00B318` (sprite, rows E-F
  cols 2-7), `$00B674` (berries, rows 2-4 and 9-B cols 9-F). Colour 1 is `$7FDD`/`$7FFF`.
  Lunar Magic's `-ExportSharedPalette` is exactly the ROM bytes from `$00B0A0`.
- Map16: layer 1 pointers are built from `$0D8000` (common) and per-tileset data (`$058000`
  word table into bank `$0D`) using the bitmask at `$0581BB` (bit set = common). Tilesets 0 and
  7 patch `1C4-1C7`/`1EC-1EF` from `$0D8A70` at load time. Layer 2 tiles are `$0D9100`; the game
  numbers them `200-3FF`, Lunar Magic's `.map16` stores them at file index `8000`.
- Level load: `$7E0109` non-zero forces a level: values `< $25` are the level low byte, else
  low byte = value - `$24`; `$7E1F11` non-zero sets the high byte. Zero means no override, so
  levels `000`/`100` and low bytes `$DC+` cannot be selected this way. The title screen uses
  `$EB` (level `C7`); game mode 3 and game mode `$11` both enter `GM11LoadLevel` (`$0096D5`).
  Lunar Magic 2.5x and 3.1x+ ROMs also reroute that path with a `JML` at `CODE_05D83E` and
  resolve the pointers in their own code, so no fixed breakpoint inside `CODE_05D796` exists.
  `expand` therefore enters every level as a screen exit on screen 0: `$141A` (sublevel
  count) non-zero, the low byte in `$19B8`, and the high byte both as the player's submap
  `$1F11` (vanilla) and as `$19D8 = $04 | hi` (Lunar Magic's exit table format, read by the
  `JSL $05DC50` it installs in `CODE_05D796`: bit 2 marks its format, bit 0 is the high
  byte, bit 1 a secondary exit, bit 3 is copied to `$192A`; vanilla only stores the water bit
  there and never reads it). Every ROM in the corpus keeps the screen-exit path's `JMP
  CODE_05D8B7`. The oracle script instead keeps the real overworld entry and patches
  `$0E`-`$0F` at `CODE_05D8B7`, so the two sides reach a level by different routes.
- "No Yoshi" entrance intro: when entering from the overworld with `$141A`, `$141D`, and
  `$141F` all zero and the header tileset 1, 2, 5, 6, or 8, `CODE_05DA24` loads one of six
  one-screen intro rooms (`PtrsLong05D766`, data at `$078000`, modes `$0E`/`$0F`) instead of
  the level. `$141F` comes from bit 7 of the entrance table at `$05F600` (Lunar Magic's
  "disable No-Yoshi intro" flag); the intro ends by setting `$141D` (`ShowMarioStart`) and
  reloading. The screen-exit entry skips it; the oracle script predicts it from the ROM
  tables and dumps the second level frame.
- `expand::expand_level` seeds the RAM-resident OAM routine by running the reset code, then
  runs `CODE_05D796` (header pointers), `CODE_05801E` (clear buffers, `LoadLevel`), the rest
  of game mode `$11` (`CODE_00B888` GFX32/33 to RAM, `CODE_00A635`, `CODE_00A796`), and all of
  game mode `$12` (`GM12PrepLevel`, `$00A59C`), which draws boss floors, sets up layer 3 (tides
  zero rows 16-26 of the layer 2 screens), and uploads GFX, palettes, and initial tilemaps.
  `$0100` is set to `$11` and then `$12` on the way: Lunar Magic's replacement for the initial
  tilemap upload (`CODE_05809E` jumps to `$1FB1E8`) checks the game mode and uploads nothing
  under a stale value.
  The bus captures VRAM/CGRAM port and DMA writes, so rendering uses what the game uploaded:
  ExGFX, custom palettes, and animated tiles come for free. VRAM matches the emulator except
  animated slots (frame-dependent) and tilemap areas filled on later frames; CGRAM matches
  except Mario's row 8 and one per-frame colour.
- Tile grid layout: horizontal levels are 16x27 per screen, screen after screen. Vertical
  levels are 32 wide; each screen is 16 rows stored as a left and a right 16x16 half.
- Layer 2 objects live in the upper part of the same grid planes, with a layout chosen by
  level mode independently of layer 1 (`CODE_058883` dispatch, screen tables at `$00BB08`
  and `$00BC16`): modes `01`-`04`, `0F`, and `1F` use 16 horizontal screens from plane offset
  `$1B00`; modes `05`-`08` use 14 vertical screens from `$1C00`. Modes `03`/`04` pair a
  vertical layer 1 with a horizontal layer 2. The upload resolves the tile numbers through
  the layer 1 Map16 pointer table (`$0FBE`), not the BG table, and ORs `$1000` (palette
  row + 4) into every word when the object tileset is 3. `expand::Layer2Objects` and
  `LevelTiles::layer2_object_tile` encode this; the renderer stacks layers in Mode 1 order
  (layer 2 low priority, layer 1 low, layer 2 high, layer 1 high). The layer 2 background tilemap is decoded into
  `$7EB900`/`$7EBD00` (two screens); its tile numbers index the BG Map16 (`200+`). The buffer
  is captured right after `LoadLevel`: game mode `$12` decompresses GFX into `$7EAD00`, and a
  4bpp file (Lunar Magic) overruns the 3bpp-sized buffer into `$7EB900`. The game has
  uploaded the tilemap to VRAM by then and does not notice.
- Level modes `$09`, `$0B`, `$0F`, and `$10` (boss arenas and the dark rooms sharing their
  tilemap) never upload the decoded background; the renderer skips it.
- Mode 7 boss arenas render a 256x224 scene from captured video registers, with the ROM's
  NMI/IRQ handlers selecting the Mode 1 ceiling/floor bands and Mode 7 transform. One
  game drawing pass supplies packed OAM (including arena walls and Bowser's floor) and
  player/boss VRAM uploads. Its RAM changes are restored so collision grids remain at
  the loader state. This is an initial arena view, not a cycle-timed gameplay screenshot.
- Capture the screen count (`$005D`) immediately after `LoadLevel`: boss preparation
  overwrites it (level `$1C7` ends with `$FF`). `LevelTiles::size()` also bounds dimensions
  to complete screens in the captured grid planes.
- Tilemaps: `BG1SC`-`BG4SC` are captured in `LevelTiles::bg_sc`. Vanilla puts layer 1 at VRAM
  word `$2000` and layer 2 at `$3000`, both 64x64 tiles, and uploads the whole two-screen
  background. Lunar Magic uses `$3000`/`$3800`, 64x32, and uploads only the 15 or 16 rows
  around the initial scroll position. `LevelTiles::vram_written` says which bytes were touched.

## Lunar Magic ROM facts

- Marker: `Lunar Magic Version X.YZ ...` as ASCII at `$0FF0A0` (`Rom::lunar_magic_version`).
- Map16 pages 0-1 stay in the vanilla tables (rewritten in place). Higher pages live in
  RATS-tagged blocks whose layout differs by Lunar Magic version; the routine at `$06F540`
  (called with A = tile*2, 16-bit; returns the pointer's low word in A and bank in `$0C`)
  resolves any layer 1 tile number. Call it on the core instead of parsing the blocks.
- The BG Map16 pages (`200`-`3FF`, Lunar Magic's file index `8000+`) are a separate block from
  layer 1 pages 2-3 and `$06F540` does not find them. Lunar Magic 2.3+ replaces `STA $0A` at
  `$058DA4` in the layer 2 tilemap upload with a `JSL` (to `$0EFD00`) that leaves the level's BG
  table pointer in `$0A`-`$0C`, chosen from a 3-byte pointer table at `$0EFD50` by the level's
  flags in `$7FC00B`. `expand` calls whatever the hook targets; 1.6x ROMs keep `$0D9100`.
  `LevelTiles::map16` holds foreground definitions (including pages 2-3 resolved through
  `$06F540`); `bg_map16` holds background definitions. Keep these separate despite their
  overlapping tile numbers.
- Per-level flags at `$0EF310` (copied to `$7FC00B` by the hook at `$05803B`): bit 1 marks a
  Lunar Magic background stored at the level's own layer 2 pointer, bit 2 a 32-row background
  whose buffer uses `$200` bytes per screen. The hook leaves that stride in `$05`;
  `LevelTiles::layer2_screen_len` carries it. Background indices can exceed `$1FF`; retain
  the raw index and load enough BG definitions instead of OR-ing in `$200`. Background
  presence follows the loaded level mode, since object levels can retain an `$FF` pointer
  bank. Upload rows wrap with a five-bit mask, not modulo 27. Grand Poo World 2's 32-row
  backgrounds pass the tilemap check; its unused level `$09F` has a null BG table pointer
  and is explicitly rejected with `MissingBackgroundTable`.
- Custom level palettes: 3-byte pointers at `$0EF600` per level to `$202` bytes (back area
  colour, then 256 colours); `$000000`/`$FFFFFF` = none. Game mode `$12` loads them itself.
- ExGFX and Lunar Magic's 4bpp re-inserted GFX are handled by the game's own upload code, so
  capturing VRAM during game mode `$12` covers them without knowing the tables.
- Sprite data: header `SBNMMMMM`, entries `yyyyEESY XXXXssss NNNNNNNN`. In vertical levels
  the game reads `Y` as the X position and `screen*16 + X` as the Y position. The header's
  `N` bit (`$20`, "new sprite system") selects the format per level, whatever the Lunar
  Magic version: clear means `$FF` ends the list (most levels in LM 3.x hacks, including
  every untouched one); set means `$FF` starts a command: `$00`-`$7F` sets the Y position's
  upper bits (`y = nn*32 + yyyyy`) for every following sprite, `$FE` ends the list, `$FF` is
  a sprite whose first byte is `$FF`. PIXI extension bytes: if `$0EF30F` is `$42`, a
  `$400`-byte size table at `read3($0EF30C)` indexed by `extra_bits*256 + id` gives the
  entry size. Lunar Magic relocates sprite data into RATS blocks; take the pointer the game
  resolved at `$7E00CE` after loading rather than the vanilla table. `tests/sprite_lists.rs`
  checks every parsed list's length against the RATS tag preceding it, on every ROM in
  `KOBO_LM_ROMS`.
- SA-1 hacks do not run yet: the SA-1 registers and its CPU are not modelled.

## Decisions

- **Rust core.** Chosen for single-binary distribution, compile-time address typing, C FFI to
  Asar, and the ability to expose the core to Python, Lua, JS, and WebAssembly later.
- **Headless 65816 core for object rendering.** `kobo_core::cpu` executes the ROM's own
  level-loading routines rather than re-implementing every object; validated against emulator
  dumps of every vanilla level. Small formats (LC_LZ2, GFX, palettes) are hand-written because
  the build must also encode them.

## Known gaps

- Layer 3 is not rendered: no status bar, layer 3 backgrounds, or tides. The uploaded layer 3
  font is only used for sprite ID markers.
- Sprites in ordinary levels are drawn as ID markers, not graphics. Boss arenas show the
  OAM of the first drawing pass instead.
- Vertical levels skip the layer 2 background tilemap; `tests/layer2_background.rs` skips
  them too.
- Lunar Magic 3 levels with expanded dimensions render wrong. Level 106 of `SMW_2022-4-9`
  (LM 3.31) reports mode `$00` and 6 screens, but its sprite list places sprites at Y 32-36
  and the render is a jumble of chunks, so the level is taller than 27 rows and the grid
  planes are laid out differently from the vanilla per-mode layout. The loader runs fine;
  `LevelTiles::size()` and the plane indexing need the LM 3 level dimension data.
- `GFX27`'s layout is unknown; `GFX32`/`GFX33` are not handled by the GFX tooling.

## Open decisions

- GUI toolkit. Deferred until the library exists.
- At what level can/will baseroms be supported?

## Prior art to know

- Lunar Helper / Callisto (build orchestration), Lunar Monitor (auto-export for git).
- pokeemerald + Porymap (the source-first editor model for another game).
- SMWCentral documentation of Lunar Magic's ROM formats and hijacks.
