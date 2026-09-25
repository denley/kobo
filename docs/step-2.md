# Step 2: the build pipeline

The plan for roadmap step 2 and the decisions it rests on, settled 2026-09-25. The rules
that follow from them are in [AGENTS.md](../AGENTS.md); this file has the reasoning and the
work order. Fold what stays true into the other docs as step 2 lands, and delete this file
when it is done.

## Goal

A project directory builds into a ROM from a clean SMW ROM. Levels, Map16, palettes, and
ExGFX are written natively in Lunar Magic's layout; existing work is imported from MWL files
and from ROMs; Asar, PIXI, GPS, UberASM Tool, and AddmusicK run in a fixed order. What Kobo
does not cover yet (overworld, title screen, credits, messages) is finished in Lunar Magic
on the built ROM.

## Decisions

### Clean room: interface, not implementation

- Interface, which Kobo matches exactly: hook addresses, where each table and block lives
  and its format, the RAM values a hook leaves for other code (`$13D7`, `$7FC00B`), and the
  bytes other tools check for.
- Implementation, which Kobo writes itself: the code behind each hook.
- Allowed evidence: SMWCentral and SNESLab documentation, Lunar Magic's readme and help
  file, the sources of open tools (PIXI, GPS, UberASM Tool, SA-1 Pack), byte diffs of a ROM
  before and after a Lunar Magic operation, and running Lunar Magic-saved ROMs to observe
  which addresses they read and write, with what values.
- Not allowed: reading Lunar Magic's instructions, as a disassembly of the ROM or the
  executable or as an instruction trace (`KOBO_CPU_TRACE` over its code), and copying its
  bytes. Step 1's findings describe data layouts and stay.
- Why: reading code to interoperate is likely lawful. The concern is provenance. Hook code
  is small and constrained, so code written after reading Lunar Magic's comes out nearly
  the same, and an agent with a disassembly in context reproduces it. Every patch has to be
  licensable under MPL-2.0, and the project depends on the community's trust (compare
  Wine's contribution policy, or ReactOS halting for an audit in 2006).
- If Lunar Magic recognises its hooks by their code bytes, Kobo cannot match them; Lunar
  Magic then installs its own over Kobo's in the copy it saves, which the next decision
  allows.

### Lunar Magic and Kobo builds

- Required: Lunar Magic opens a Kobo-built ROM, shows every piece of Kobo-managed content
  correctly, and saves without losing any of it.
- Not a goal: pulling Lunar Magic edits back into a project. Import from a ROM exists for
  migration, and if it is stable a round trip falls out for supported features, but effort
  goes into supporting a feature, not into working around its absence.
- `kobo build` always overwrites its output. The ROM is a build artefact.
- Whether a build carries the `Lunar Magic Version` string at `$0FF0A0` is decided by the
  hook spike; omitted until then.
- Kobo writes the layout of the current Lunar Magic release and pins that release for the
  Lunar Magic checks.

### No base

- A build always starts from the clean ROM. No ROM or BPS belongs in a project.
- Baseroms are supported as projects: import from a ROM, and template projects for widely
  used baseroms. A template references third-party patches by URL and hash where their
  licence does not allow redistribution.
- Import reports what it did not carry over: regions that differ from vanilla which Kobo
  does not model, and patched hook sites.
- A level the project does not list keeps the clean ROM's content, so a project holds only
  the levels it defines, and no Nintendo level data unless its author changed it. An empty
  level file blanks a level.

### Source formats

- `kobo.toml` is the manifest, with `format = N`. Kobo refuses a newer format and migrates
  an older one.
- TOML throughout, edited through `toml_edit` so that comments survive. Kobo owns the
  formatting; `kobo fmt` is idempotent.
- One object or sprite per line, as an inline table. File order is data order, which is
  draw order; it is never sorted.
- Positions are absolute tile coordinates in decimal. Fields are decoded where the library
  knows their meaning (an object's size nibbles as width and height, or length) and raw hex
  where it does not, including data Kobo can build but does not interpret; a comment can
  say what it is believed to be.
- Ids are the numbers the game or tool uses. Kobo writes the name as a trailing comment on
  the entry's line and refreshes it; a user's comments go on their own lines.
- A level table maps numbers to files, one `0x105 = "world1/yoshis-island-1.toml"` per
  line. It is the only place level numbers live; file names and folders are free. Numbers
  are explicit because they leak into the overworld's translevels, hard-coded levels, ASM,
  UberASM lists, and save files. `auto` entries for sublevels can come once exits and the
  overworld refer to levels by file.
- Palettes are `#RRGGBB` with each channel the SNES 5-bit value times 8, one 16-colour row
  per line, rows labelled by generated comments.
- Graphics are indexed PNG: the pixel index is the colour index, the PNG's palette is only
  a preview. `.bin` is import and export.
- Round-trip fidelity is semantic: decoding, encoding, and decoding again gives the same
  objects in the same order, and the level renders the same. Kobo's encoder chooses its own
  new-screen bits and screen jumps.

### The build

- Stages run in a fixed order, from rarely changed and slow to often changed and fast, so
  that editing a level never reruns AddmusicK or moves a tool's code. Provisional until the
  tool survey confirms the constraints:
  1. Check the clean ROM's hash; expand to the manifest's size and mapping.
  2. Kobo's ROM-side patches.
  3. User Asar patches, early group.
  4. AddmusicK.
  5. Graphics and ExGFX, palettes, Map16.
  6. PIXI, GPS, UberASM Tool.
  7. User Asar patches, late group (patches that hook the tools' code).
  8. Levels.
- A ROM snapshot is kept after each stage, keyed by the hash of the previous key, the
  stage's inputs, and its tool version, in the user's cache directory. A build reruns from
  the first stage whose key changed. That a cached build equals a clean one is tested.
- The output is a function of the clean ROM's hash, the project files, the Kobo version, and
  the tool versions alone, and is the same on Windows, Linux, and macOS. CI checks it.
- Kobo allocates free space first-fit in a fixed order and tags every block with RATS, so
  the tools' free-space searches skip it.

### Tools

| Tool | Licence | Source | Notes |
|---|---|---|---|
| Asar 1.91 | LGPL-3.0+ | C++ | Dynamic linking is fine; PIXI and UberASM Tool use 1.91 too |
| PIXI 1.43 | GPL-3.0 | C++, CMake | Builds natively on all three platforms |
| UberASM Tool 2.1 (Fernap) | GPL-3.0 | C# | Whether it runs on .NET on Linux and macOS is unverified |
| AddmusicK 1.0.11 (AddMusicKFF) | none | C++, Makefile | Ships samples taken from SMW (`samples/default`) |
| SA-1 Pack 1.40 | none | Asar patch | |
| GPS | unknown | | No public repository found; SMWCentral is not reachable from tools |

- A companion repository builds each licensed tool from a pinned upstream commit on CI for
  all three platforms and publishes the builds with their sources. Kobo downloads the one
  for its platform on first use, checks its SHA-256, and caches it per user.
- A `[tools]` path overrides a tool, for people developing it; the build is then marked as
  not reproducible.
- AddmusicK and SA-1 Pack have no licence and AddmusicK contains Nintendo data: never
  bundled. Kobo fetches them from upstream by hash, or the user supplies them. Asking their
  maintainers, and GPS's, to add a licence is an early action item.
- Each Kobo release pins one set of tool versions. Per-project pins can come later.

## Prework

1. Development environment: Wine with the current Lunar Magic release (for its
   command-line exports and the Lunar Magic checks) and Asar.
2. The hook spike: hand-write one minimal hook in Lunar Magic's layout, open the ROM in
   Lunar Magic under Wine, save, and diff. Does Lunar Magic use the data as it is, reinstall
   its hooks and keep the data, or reset the tables? This settles the `$0FF0A0` string and
   how exact the layout has to be.
3. Write-side research, into [lunar-magic.md](lunar-magic.md): every table and block Lunar
   Magic writes, how they differ by version, the hook sites, and what PIXI, GPS, UberASM
   Tool, and AddmusicK check before accepting a ROM (from their sources).
4. ROM writing: writes through `SnesAddr` and `Mapping`, expansion, header and checksum,
   and a deterministic RATS allocator, tested against synthetic SA-1 images.
5. A level reader and writer for layer 1 and 2 objects, background tilemaps, headers, and
   sprite lists, checked by round trip on every level of vanilla and the corpus and by
   extending `fuzz_inputs`.
6. BPS reading and writing. It also brings the QLDC entries into `KOBO_LM_ROMS`.
7. An LC_LZ2 compressor that always produces the same output.
8. Asar integration: `libasar` through FFI, on all three CI platforms.
9. Name tables for objects, sprites, tilesets, and level modes, as data in the library.
10. Build checks: import, rebuild, and compare `render_hashes`; for a hack whose ASM and
    custom sprites the rebuild lacks, compare `LevelTiles` and layers 1 and 2 without
    sprites instead. A synthetic base image lets CI build without ROM data.

## Work order

- 2a: the pipeline with vanilla formats. Manifest and level table, the level reader and
  writer, ROM writing and the allocator, the staged build and its cache, BPS output, Asar
  for Kobo's own patches, and import from MWL files and ROMs. Milestone: every vanilla
  level imported as text, rebuilt into expanded space, and rendering as vanilla does.
- 2b: Lunar Magic-layout features one at a time, each taken through its source format,
  import from MWL and ROM, build, the Lunar Magic check, and the corpus check together, so
  neither direction anchors the format: Map16 pages 2 and up and background Map16, custom
  palettes, ExGFX, expanded level sizes, the sprite data formats (new sprite system, 255
  sprites, PIXI extension bytes), secondary entrances and exits.
- 2c: running the tools (user Asar patches, PIXI, GPS, UberASM Tool, AddmusicK), the
  companion build repository, and SA-1 builds with SA-1 Pack.

## Risks

- Lunar Magic rejecting or overwriting Kobo's hooks, or resetting tables it thinks are
  uninitialised. The hook spike comes first.
- Clean-room contamination, from an agent or contributor working from Lunar Magic's code.
- Lunar Magic's layout changing between versions: read many, write one.
- Import losing data silently. The import report and raw fields cover it.
- What a render does not show: exits, entrances, midway points, and secondary headers need
  the Lunar Magic check or emulator entry tests.
- The tools: whether each builds and behaves identically on all three platforms, and
  their licences.
- The corpus is ROMs, not projects; every test project is made by exporting from Lunar
  Magic under Wine, and only hashes are committed.
- Drifting towards Lunar Magic parity.
