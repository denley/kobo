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
  Every ROM read takes a `SnesAddr` and goes through the ROM's `Mapping` (LoROM, SA-1, or
  SA-1 over 4 MiB); the bus follows an SA-1's bank registers (`SuperMmc`) once the game
  has written them.
  Conversions mirror Asar's conventions so addresses agree with the rest of the toolchain.
- `kobo_core::ram` is the only place that knows where the game keeps its variables. A
  `RamAddr` names a variable by its vanilla `$7E`/`$7F` address, a `RamMap` resolves it to a
  bus address (`Vanilla`, or `Sa1Pack` with its I-RAM and BW-RAM addresses and 22 sprite
  slots), and `Ram` is the memory itself: `bus.ram.u8(ram::LEVEL_MODE)`, never
  `wram[0x1925]`. Tables are indexed from their resolved start (`u8_at`). A `Ram` clone is a
  snapshot, which on an SA-1 cartridge includes the SA-1 (`cpu::sa1::Sa1`: its registers and
  its `Cpu`), so that restoring one never leaves it mid-handler; video memory is not part of
  it, since the game never reads it back.
- `kobo_core::cpu` has one 65816 core and two instances of it. `Cpu::run` takes IRQs from
  the `Bus` and hands over to `Bus::wait` when the CPU stops to wait (`WAI`, or a loop that
  changes nothing); `SmwBus` gives the SA-1 its turn there, through `Sa1View`, the bus as
  the SA-1 sees it, and failing that the NMI `Bus::vblank` offers, a bounded number of
  times. A wait nothing answers is `CpuError::Waiting`, not 200 million steps.
  `KOBO_CPU_TRACE=<n>` prints the last `n` instructions before a routine fails or a
  `KOBO_RAM_WATCH` address is written; `KOBO_VRAM_WATCH` reports writes to a VRAM word.
- `kobo_core::expand` runs ROM code. `machine` owns the CPU, the bus, and `Call` (the register
  state a routine is entered with; every call starts from reset registers, and
  `try_call_to` stops one at an address with the call still open); `Machine::interrupt`
  runs the ROM's NMI or IRQ handler whole, from its vector to its `RTI`, since patches
  replace both ends of them. `load` runs the loader phases in game mode `$11`'s own order,
  and `sprite_capture`, `player`, `boss`, `layer3`, and `map16` are the passes over the
  loaded level. `oam` reads a frame's objects as the PPU gets them: the ROM's OAM upload
  runs after every frame and the bus keeps what arrives at `$2102`-`$2104`, so the ROM
  decides which object is in front; do not read `$0200` and `$3F` instead. A sprite pass
  runs the ROM's whole NMI handler for it, on video memory of its own, since some sprites
  have no tiles until it has run; what a pass uploaded for its objects is kept with them
  (`SpriteScene::dynamic`). `routines` holds the ROM addresses. `expand_level` returns a
  `LoadedLevel` of four parts: `tiles` (`LevelTiles`: the grid, Map16 definitions, and layer
  layouts, which is what the level *is*), `video` (`VideoMemory`: VRAM, CGRAM, `BGnSC`,
  `OBSEL`), `scene` (`LevelScene`: screen setup, camera, layer 3, player, boss arena), and
  `ram`. A pass the CPU core gives up on is recorded as a `Diagnostic`
  (`LoadedLevel::diagnostics`, `SpriteScene::diagnostics`) instead of failing the level;
  `expand::summarize` turns them into one line per distinct error.
- `render::render_level(rom, level, options)` is the one way to a level's picture (and
  `render_loaded` for a level already loaded): it owns the drawing order (layers, sprite
  scene, the player behind the sprites, compose, markers on top; a boss arena skips the
  sprite capture). The CLI and `tests/video_oracle.rs` both call it; do not rebuild that
  sequence in a shell.
- `render` has one of each PPU primitive, all public so a GUI can redraw a tile or a viewport
  through them; extend these instead of adding a second:
  `Tilemap::pixel` reads any background tilemap (layer 3, an arena's layer 1),
  `character_pixel` any character data, `draw_objects` rasterises `SpriteObject`s,
  `tile_pixels` walks a tile for both the image and the layer drawers
  (`LevelLayers::draw_map16` and `draw_tile_ref`, styled by a `LayerStyle`), and
  `LevelLayers::compose` applies screen designation, colour math, and a fixed screen's
  `video::Window`. A boss arena is drawn into `LevelLayers` like any level.
- `kobo_core::operation::Operation` is the handle a long operation runs under: an
  instruction budget shared by both CPUs and every pass, cancellation, and a stage for
  progress. The `_with_control` variants of `expand_level`, `capture_sprites`,
  `render_level`, and `render_loaded` take one; cancellation and an exhausted budget fail
  the operation rather than returning a partial picture.
- `cpu::access::UnsupportedAccesses` is the bus's bounded report of hardware it does not
  model and a picture may be missing something for; it reaches the caller as
  `Diagnostic::Unsupported`. Accesses with nothing to model (open bus, read-only
  registers, controllers) are counted as stubs on the bus, never reported: classify a new
  register in `SmwBus::read_register` or `write_register` instead of letting it be reported.
- `kobo_core::level::LevelMode` is the only place that says what a level mode means to the
  library (`layer2()`: background, horizontal or vertical objects, or none). Do not match on
  mode numbers anywhere else; what the ROM's own per-mode tables decide is read from RAM.
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
cargo run -- palette png --level 105 out.png # 16x16 swatch of the palette the level loaded
cargo run -- map16 png --level 105 out.png   # the level's Map16 tiles in colour (--layer 2: BG)
cargo run -- level png 105 out.png           # render a level by running the ROM's own loader
cargo run -- level png 105 out.png --markers # ID boxes instead of sprite graphics (--no-sprites: none)
cargo run -- level png 105 out.png --no-player # leave Mario out of the entrance
cargo run -- level tiles|dump 105 [dir]      # the expanded Map16 grid as hex, or raw planes
cargo run -- level sprites|map16|wram|reads  # sprite list, resolved Map16, RAM dump, read trace
cargo run -- addr '$05E000' [--sa1]          # SNES <-> file offset
cargo run --release --example sprite_census -- rom.smc  # sprite numbers that draw nothing, by level
cargo run --release --example render_hashes -- rom.smc  # a hash per level picture, to diff across a change
cargo run --release --example sprite_oracle -- rom.smc dumpdir...  # per-sprite scores against emulator frames
cargo run --release --example fuzz_inputs -- 10000      # seeded parser mutation cases, no ROM needed
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
  configured, so CI never needs ROM data; `KOBO_REQUIRE_ROM=1` makes that a failure. A
  configured ROM must be the vanilla reference. Run them locally before pushing.
- The vanilla reference is No-Intro "Super Mario World (USA)", headerless SHA-1
  `6b47bb75d16514b6a476aa0c73a683a2a4c18765`, checksum `$A0DA`.
- **Oracle tiers** are opt-in by environment variable and compare against data that is never
  committed: `KOBO_ORACLE_DIR` (Mesen 2 dumps of every vanilla level, `tools/oracle/`; of
  another ROM, the SA-1 reference ROM say, with `KOBO_ORACLE_ROM`),
  `KOBO_BOSS_ORACLE_DIR`, `KOBO_VIDEO_ORACLE_DIRS` (whole pictures), `KOBO_65816_TESTS`
  (SingleStepTests), and `KOBO_LM_ROMS` (`:`-separated Lunar Magic hacks). Lunar Magic
  exports (hashes in `tests/fixtures/`) are the oracle for GFX, palette, and Map16.
  `docs/testing.md` has how each is produced, where the data lives, and the known exceptions.
- A change to `expand` or `render` that should not change any picture is checked with
  `examples/render_hashes.rs`: run it before and after on the vanilla ROM and a few hacks and
  diff the output.

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

## Knowledge base

This file is loaded into every agent context, so it holds rules, layout, and pointers. Facts
live in `docs/`: read the file for the area you are working in before you start, and record
what you find out there in the same change (or in the module's documentation, when it
describes that module's code rather than the game). Do not grow this file with them.

- `docs/smw.md`: the vanilla game. ROM tables (level pointers, GFX, palettes, Map16), how a
  level is entered and loaded and which routines `expand` runs, the tile grid and layer 2
  layouts, level modes, screen designation and colour math, layer 2 and 3 positions, the
  player and sprite capture passes, boss arenas and their window.
- `docs/lunar-magic.md`: what Lunar Magic changes in a ROM. Map16 pages and the `$06F540`
  routine, BG Map16 tables, per-level flags, expanded level heights, custom palettes, sprite
  data formats and PIXI extension bytes, the 255-sprite load flags.
- `docs/sa1.md`: SA-1 Pack. How its two processors hand work over and how the bus schedules
  them, the RAM it moves, MaxTile and the OAM, the work RAM port, SA-1 DMA, images over
  4 MiB, the reference ROM, what it changes in the vanilla levels, what is not modelled.
- `docs/testing.md`: the emulator oracle and its capture modes, the video oracle, the CPU
  suite, the Lunar Magic hack corpus checks and their known exceptions.
- `docs/known-gaps.md`: what a rendered level does not reproduce.

## Decisions

- **Rust core.** Chosen for single-binary distribution, compile-time address typing, C FFI to
  Asar, and the ability to expose the core to Python, Lua, JS, and WebAssembly later.
- **Headless 65816 core for object rendering.** `kobo_core::cpu` executes the ROM's own
  level-loading routines rather than re-implementing every object; validated against emulator
  dumps of every vanilla level. Small formats (LC_LZ2 and LC_LZ3, GFX, palettes) are hand-written because
  the build must also encode them.

## Open decisions

- GUI toolkit. Deferred until the library exists.
- At what level can/will baseroms be supported?

## Prior art to know

- Lunar Helper / Callisto (build orchestration), Lunar Monitor (auto-export for git).
- pokeemerald + Porymap (the source-first editor model for another game).
- SMWCentral documentation of Lunar Magic's ROM formats and hijacks.
