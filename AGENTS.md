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
- Planned oracles for the renderer: emulator RAM dumps of the expanded tile grid (`$7EC800` /
  `$7FC800`) for level decoding, and Lunar Magic exports for GFX, palette, and Map16 formats.

## Reference material

- `~/.local/share/kobo/docs/smwdisx/`: the SMWDisX disassembly banks and `SMW_U.sym` (downloaded
  from GitHub, not committed). Use it to read how the game consumes a table; never build on it.
  SMW Central is behind a JavaScript challenge and cannot be fetched from tools.
- Asar 1.91 built from source: `~/.local/bin/asar`, `libasar.so` in `~/.local/lib`.
- Mesen 2.1.1: `~/.local/share/kobo/tools/mesen/Mesen`. Headless use is
  `Mesen --testRunner script.lua rom.sfc --timeout=N`; the script ends with `emu.stop(code)`.
  Lua enums are lower-camel-cased C++ names: `emu.memType.snesMemory`, `emu.eventType.endFrame`.

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
  low byte = value - `$24`; `$7E1F11` non-zero sets the high byte. The title screen uses this
  with `$EB` (level `C7`), and game mode 3 falls straight into the level loader.

## Decisions

- **Rust core.** Chosen for single-binary distribution, compile-time address typing, C FFI to
  Asar, and the ability to expose the core to Python, Lua, JS, and WebAssembly later.
- **Headless 65816 core is the preferred route for object rendering.** Execute the ROM's own
  level-loading routines rather than re-implementing every object. Small formats (LC_LZ2, GFX,
  palettes) are hand-written because the build must also encode them. Still to be prototyped.

## Open decisions

- GUI toolkit. Deferred until the library exists.
- At what level can/will baseroms be supported?

## Prior art to know

- Lunar Helper / Callisto (build orchestration), Lunar Monitor (auto-export for git).
- pokeemerald + Porymap (the source-first editor model for another game).
- SMWCentral documentation of Lunar Magic's ROM formats and hijacks.

