# Kobo

Kobo is an open-source Super Mario World ROM editor and build system in development.
What exists today is a **Rust library and command-line renderer**. The build pipeline
and desktop editor are planned; they are not implemented.

The long-term model is a git-friendly project of text source files and assets, compiled
by patching a separately supplied clean ROM. See [AGENTS.md](AGENTS.md) for the project
principles, roadmap, and development conventions.

## What it can do today

- **Read any SMW ROM**: vanilla, Lunar Magic hacks, and SA-1 hacks, including images over
  4 MiB, with or without a copier header. Address mapping is typed (LoROM, SA-1), and
  SNES addresses convert to and from file offsets the way Asar does.
- **Render any level to PNG** by running the ROM's own level loader on a headless 65816
  core (two of them for SA-1), so whatever a hack patched into the loader, Lunar Magic's
  Map16 pages, expanded level heights, custom palettes, ExGFX, and custom sprite loaders
  (PIXI) come out as the game produces them. Layers 1, 2, and 3 are composed with the
  PPU's screen designation and colour math; sprites are drawn by the game's own sprite
  engine, one at a time, or as ID markers; Mario stands at the entrance; Mode 7 boss
  arenas are rendered with their window.
- **Inspect a level**: its header and pointers, its sprite list with extension bytes, the
  expanded Map16 grid as hex or raw planes, the Map16 definitions it resolved, a RAM dump
  after loading, and a trace of which ROM pages the loader read.
- **Show a level's palette and Map16 tiles** as the loader produced them, which is what
  Lunar Magic's palette and Map16 editors show, or the vanilla tables of any tileset.
- **Work with graphics**: list, decompress (LC_LZ2 and LC_LZ3), and export GFX files in
  Lunar Magic's layout, or render one as a tile sheet.
- **Run under a budget**: long operations take an instruction budget, cancellation, and a
  progress stage, and report hardware the machine does not model instead of guessing.

Validation is against emulators and Lunar Magic rather than by eye: every vanilla level
and every level of the SA-1 reference ROM match Mesen 2 dumps in tile grid, layer 3, and
sprite slots; whole pictures match emulator frames on a sample; all 512 slots of 173 hack
ROMs were rendered, with the remaining failures traced to the hacks themselves; and GFX,
palette, and Map16 output is hashed against Lunar Magic's exports.
What a picture does not reproduce is in [known gaps](docs/known-gaps.md).

## Build and try it

Install [Rust](https://www.rust-lang.org/tools/install) through rustup. The repository
pins its compiler in `rust-toolchain.toml`; Cargo selects that toolchain automatically.

```sh
cargo build --release --workspace
cargo run --release -- rom info -r /path/to/smw.sfc
cargo run --release -- level png 105 /tmp/level-105.png -r /path/to/smw.sfc
```

Level numbers are hexadecimal. Replace the example output path on Windows. Run
`cargo run -- --help` or `cargo run -- level png --help` for available commands.

Supply your own ROM. ROMs, exported Nintendo assets, and emulator dumps must never be
committed or distributed with Kobo. A copier header is accepted and stripped for reads
and identity checks. The vanilla reference is Super Mario World (USA), headerless SHA-1
`6b47bb75d16514b6a476aa0c73a683a2a4c18765`.

To omit `-r`, set `KOBO_SMW_ROM` to your vanilla ROM path, or create `kobo/config.toml`
in your platform's user configuration directory (`$XDG_CONFIG_HOME`, normally
`~/.config`, on Linux):

```toml
[roms]
smw = "/path/to/smw.sfc"
```

Other useful commands:

```sh
cargo run --release -- level png 105 level.png --no-player
cargo run --release -- level png 105 markers.png --markers
cargo run --release -- level png 105 level.png --max-instructions 500000000
cargo run -- rom info
cargo run -- level info 105
cargo run -- gfx list
cargo run -- gfx export /path/to/export-directory
cargo run -- palette png --level 105 palette.png
cargo run -- map16 png --level 105 map16.png
```

With `--level`, the palette and Map16 sheet commands show what the ROM's loader produced
for that level, as Lunar Magic's editors do; without it they use the vanilla tables. Full
level rendering follows
the ROM's loader and captured video state for Lunar Magic modifications. Rendering is a
static editor view, not full emulation: animation, HDMA, and some interactions differ.
See [known gaps](docs/known-gaps.md). A successful PNG can carry warnings; inspect them
when assessing compatibility.

## Develop and validate

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Synthetic tests always run. ROM-backed tests skip when no ROM is configured, and
external oracle suites run only when selected, so a green default run **does not
imply that ROM compatibility was tested**. With a vanilla ROM configured,
`KOBO_REQUIRE_ROM=1 cargo test --workspace` fails instead of skipping. See
[testing](docs/testing.md) for the oracle tiers, mutation fuzzing, and known
exceptions. CI runs the Rust checks on Linux, Windows, and macOS.

## Library and architecture

- `crates/kobo-core`: parsers, typed address mapping, RAM relocation, the headless
  65816/SA-1 machine, level loading, sprite capture, drawing, and PNG output.
- `crates/kobo-cli`: argument handling, command dispatch, and human-readable output.
- `tools/oracle`: reference capture scripts for Mesen 2, using a private ROM copy.

`render::render_level` is the entry point for a complete picture;
`render_level_with_control` takes an `operation::Operation` for cancellation,
progress, and an instruction budget. `gfx::GfxReader` reads many graphics files
from one ROM. The module documentation in `kobo-core` describes each contract.

Game and compatibility references:
[vanilla SMW](docs/smw.md), [Lunar Magic](docs/lunar-magic.md), [SA-1](docs/sa1.md).

Kobo is licensed under [MPL-2.0](LICENSE).
