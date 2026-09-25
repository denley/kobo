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
- A build does not carry the `Lunar Magic Version` string at `$0FF0A0`. The hook spike
  showed Lunar Magic neither reads it to decide what is installed nor needs it, and writes
  it on its own first save.
- A build always has a correct internal checksum; Lunar Magic warns that a ROM "may be
  Corrupt" otherwise.
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
  that editing a level never reruns AddmusicK or moves a tool's code. The tool survey
  ([toolchain.md](toolchain.md)) fixed the constraints; open to revision if more turn up:
  1. Check the clean ROM's hash. For SA-1, apply SA-1 Pack (with `$0FFFEB` set first for
     LC_LZ3), then its 6 or 8 MiB patch. Expand to the manifest's size, filled with `$00`.
  2. Kobo's ROM-side patches, none of it in AddmusicK's ranges (`$0E8000`-`$0EF0FF`,
     `$0F8000`-`$0FF050`).
  3. User Asar patches, early group.
  4. AddmusicK: it needs `$0E8000` untouched and everything before it RATS-tagged.
  5. Graphics and ExGFX, palettes, Map16 and the acts-like table: GPS rewrites that table,
     and PIXI and GPS refuse a ROM without its pointer at `$06F624`.
  6. PIXI (with `-meimei-off`), GPS, UberASM Tool, in that order: UberASM Tool reads the
     flag PIXI sets at `$0FFFE0`.
  7. User Asar patches, late group (patches that hook the tools' code).
  8. Levels, after PIXI, whose size table sets the length of each sprite entry.
- Every stage runs on the previous stage's snapshot. PIXI, GPS, and SA-1 Pack do not
  repeat their output when run over it.
- Every table a tool repoints and every hook site a tool takes over leads to a RATS block
  of its own, since Asar's `autoclean` erases the whole block behind the old target.
- A ROM snapshot is kept after each stage, keyed by the hash of the previous key, the
  stage's inputs, and its tool version, in the user's cache directory. A build reruns from
  the first stage whose key changed. That a cached build equals a clean one is tested.
- The output is a function of the clean ROM's hash, the project files, the Kobo version, and
  the tool versions alone, and is the same on Windows, Linux, and macOS; CI checks it.
  The exception is what a tool orders by directory listing (PIXI's shared routines, GPS's
  routines, UberASM Tool's library files): a build that uses those is repeatable on one
  file system but may differ on another. Decided 2026-09-25 that this is acceptable for
  now; identical output everywhere is a nice-to-have there, not a requirement.
- Kobo allocates free space first-fit in a fixed order and tags every block with RATS for
  interoperability. A tag does not stop a later tool from writing into a block
  ([toolchain.md](toolchain.md#asar-191-rats-boundary-limitation)), so every tool stage
  checks the blocks that were there before it with a `rats::Snapshot` and fails the
  build on damage, as Asar patches already do.

### Tools

| Tool | Licence | Source | Notes |
|---|---|---|---|
| Asar 1.91 | LGPL-3.0+ | C++ | Dynamic linking is fine; PIXI and UberASM Tool use 1.91 too |
| PIXI 1.43 | GPL-3.0 | C++, CMake | Builds on Linux; its CFG editor's resources are Nintendo data |
| UberASM Tool 2.1 (Fernap) | GPL-3.0 | C# | Built for x86 .NET 8, which Linux and macOS lack; needs an x64 rebuild |
| AddmusicK 1.0.11 (AddMusicKFF) | none | C++, Makefile | Builds on Linux; holds SMW samples and music |
| SA-1 Pack 1.40 | none | Asar patch | Holds code attributed to Lunar Magic |
| GPS 1.4.4 | none | C++ | No repository; release on the Wayback Machine; builds on Linux |

- A companion repository builds each licensed tool from a pinned upstream commit on CI for
  all three platforms and publishes the builds with their sources, leaving out PIXI's CFG
  editor and its Nintendo resources, and with UberASM Tool rebuilt for x64 with a native
  `libasar`. Kobo downloads the one for its platform on first use, checks its SHA-256, and
  caches it per user.
- A `[tools]` path overrides a tool, for people developing it; the build is then marked as
  not reproducible.
- AddmusicK, SA-1 Pack, and GPS have no licence, and AddmusicK contains Nintendo data:
  never bundled. Kobo fetches them from upstream by hash, or the user supplies them.
  Asking their maintainers to add a licence would help, but nothing waits on it.
- GPS runs unmodified, as the user supplies it, and Kobo's bank `$06` code has the shape
  GPS patches (decided 2026-09-25). A licence would let the companion repository patch GPS
  to use the documented `JSL` slots instead.
- Each Kobo release pins one set of tool versions. Per-project pins can come later.

## Prework

1. Development environment: Wine with the current Lunar Magic release (for its
   command-line exports and the Lunar Magic checks) and Asar.
2. The hook spike. Done: [lunar-magic.md](lunar-magic.md) has what Lunar Magic installs
   and how it decides. Lunar Magic keeps Kobo's code behind any hook site that jumps to
   it, but its one-time install is gated by `$06F600` alone and wipes its tables when it
   runs, and a save never adds the 15 one-time hooks to a ROM whose gate is set.
3. Write-side research. The tools are done ([toolchain.md](toolchain.md)). For Lunar
   Magic, [lunar-magic.md](lunar-magic.md) has the footprint and the hook sites, and
   [lunar-magic-install.md](lunar-magic-install.md) the one-time set piece by piece: the
   vanilla code each replaces, the feature (Map16 pages and the acts-like chain, taller
   levels, backgrounds, exits and midway points, the sprite loader, per-level tables,
   3.70's game loop hook), the fixed operands and slots other tools use, and the RAM a
   level load leaves. A save keeps foreign code in the set's areas and never reinstalls
   it, but rewrites four areas next to it (`$03BB00`, `$03BCA0`, `$05DD30`, `$0EF510`)
   and six table pointers inside Kobo's code. Still to find: whether Lunar Magic's
   restorable code reads the RAM the set leaves, the behaviour a level load does not
   exercise (tile changes, block contact, scrolling, the overworld, special exits), the
   custom block slots' empty state, and older versions' pieces.
4. ROM writing. Done: writes through `SnesAddr` and `Mapping`, expansion, header and
   checksum (`Rom`), and `rats::FreeSpace`, tested on synthetic LoROM and SA-1 images.
   Its bank preferences and tag placement follow Asar's, with a deliberate difference:
   Kobo preserves tagged zeros in a bank-boundary case where Asar 1.91 overwrites them.
   The regression test covers allocation both in one stage and after a rescan;
   [toolchain.md](toolchain.md#asar-191-rats-boundary-limitation) has the reproduction.
5. A level reader and writer for layer 1 and 2 objects, background tilemaps, headers, and
   sprite lists, checked by round trip on every level of vanilla and the corpus and by
   extending `fuzz_inputs`. Done for the binary formats: `level::objects`,
   `sprites::encode`, `compress::rle1`, `level::SecondaryHeader`, and `level::read_objects`
   and `read_background` find a level's data from the ROM's tables alone, vanilla or Lunar
   Magic ([lunar-magic.md](lunar-magic.md)). Locked ROMs are out of scope. Interpreting
   Lunar Magic's Map16 objects and custom backgrounds as tiles comes with their 2b features.
6. BPS reading and writing. Done: `kobo_core::bps` applies a patch with every CRC and
   bound checked (`apply_to_rom` takes one made against the headerless or the
   copier-headered image and returns the headerless target) and creates one
   deterministically, smaller than the distributed patch for every corpus hack it was
   tried on; `kobo bps apply|create` on the CLI. `KOBO_LM_ROMS` takes `.bps` entries, so
   the QLDC entries are listed as they are distributed ([testing.md](testing.md)).
7. An LC_LZ2 compressor that always produces the same output. Done:
   `compress::lz2::compress`, an optimal parse over commands 0-4 (dynamic programming,
   longest matches from a suffix array) with a fixed tie-break. It writes only what the
   game's routine and Kobo's decoder read alike ([smw.md](smw.md)); the vanilla GFX files
   come out 121,663 bytes against 130,317, and the game reads them back.
8. Asar integration. Done: `kobo_core::asar` loads `libasar` at run time (LGPL-3.0),
   checks its API version, and applies a patch to a `Rom` in memory, from disk or from
   in-memory files, with include paths and defines and the checksum left to Kobo; its
   errors, warnings, prints, labels, and writes come back as values, one patch at a time
   behind a process-wide lock. Every patch is guarded by a `rats::Snapshot`: a block
   that was there before and is changed without being released fails the patch, which
   catches the boundary corruption ([toolchain.md](toolchain.md#asar-191-rats-boundary-limitation)).
   CI builds Asar 1.91 from source on Linux, Windows, and macOS and runs the tests
   against it. `kobo asm` applies one patch.
9. Name tables for objects, sprites, tilesets, and level modes, as data in the library.
   Done: `kobo_core::names` (from `names.toml`: standard objects by object set, Lunar
   Magic's objects, extended objects, sprites, tilesets, music) and `LevelMode::name`,
   checked against the ROM's dispatch and per-mode tables (`tests/names.rs`).
10. Build checks: import, rebuild, and compare `render_hashes`; for a hack whose ASM and
    custom sprites the rebuild lacks, compare `LevelTiles` and layers 1 and 2 without
    sprites instead. A synthetic base image lets CI build without ROM data.

## Work order

- 2a progress: the level format, manifest, import from ROM, and build are in
  (`kobo_core::source`, `import`, `build`), and the milestone holds: every vanilla level
  imported with `kobo import --all`, built with its layer data in RATS blocks at `$108000`
  and up, re-imports as no changes and renders the same picture on all 512 levels
  (`render_hashes`, 2026-09-25). Sprite lists stay in bank `$07`, where the game reads
  them: unchanged ones keep their place and changed ones go in the bank's unused space
  (4.5 KiB), until 2b's Lunar Magic layout lifts that. The build runs `build::Stage`s
  with snapshots keyed by a chained hash (`build::Cache`, in the user's cache directory);
  a cached build equals an uncached one, and a synthetic base image lets CI check the
  output is the same on every platform. An import reports the ROM's changes it did not
  carry over: ranges of the clean ROM's space that differ outside every level's data and
  the tables it reads, and tagged blocks past the clean ROM that no level uses (Kaizo
  Mario: 148 ranges, 137 KB, Lunar Magic's install among them). `kobo import` takes an
  MWL file too, adding the level to a project: all 512 vanilla exports imported and built
  differ from vanilla only where Lunar Magic changed them on export. Level files carry
  Kobo's names (`kobo_core::names`) as trailing comments. Still to do in 2a: BPS output,
  and Asar for Kobo's own patches, which have none until 2b.
- SA-1 builds: `[rom] sa1 = true` has the base stage apply SA-1 Pack (`tools.sa1pack` or
  `KOBO_SA1PACK`, run through Asar, never bundled) and its 6 or 8 MiB patch for a larger
  image. An empty SA-1 project builds byte for byte what `asar sa1.asm` makes of vanilla;
  every level of that ROM imported and built back as an SA-1 project renders the same
  picture on all 512 levels. Import of an SA-1 ROM compares it with the SA-1 base
  (`build::base_image`). LC_LZ3 (`$0FFFEB`) is not yet set before SA-1 Pack.
- Secondary entrances are in the level they lead to (`[entrances]`), read from a ROM or
  an MWL file and written in the game's format, where an entrance's number must share its
  level's bit 8; a level's list replaces what the base ROM had leading to it.
- 2c progress, ahead of 2b where nothing waits on Lunar Magic's layout: the build runs
  the project's Asar patches (`[patches] early` and `late`) and AddmusicK (`[music] dir`,
  laid over the user's AddmusicK folder, `tools.addmusick` or `KOBO_ADDMUSICK`), each
  checked with `rats::Snapshot`, each stage keyed by every file it can read. A vanilla
  build with AddmusicK's default music is deterministic, and Lunar Magic saves it keeping
  AddmusicK's code and data. UberASM Tool runs too (`[uberasm] dir`, `tools.uberasm`),
  as an x64 build on Linux ([toolchain.md](toolchain.md)); Lunar Magic keeps its hooks.
- 2a: the pipeline with vanilla formats. Its builds leave `$06F600` at `$FF` and write
  nothing in Lunar Magic's layout, so Lunar Magic's first save installs itself and keeps
  Kobo's data, as the spike showed for a relocated level. Manifest and level table, the
  level reader and writer, ROM writing and the allocator, the staged build and its cache, BPS output, Asar
  for Kobo's own patches, and import from MWL files and ROMs. Milestone: every vanilla
  level imported as text, rebuilt into expanded space, and rendering as vanilla does.
  The MWL reader and writer are done (`kobo_core::mwl`, `kobo mwl info`), checked on
  21,504 files Lunar Magic 3.70 exported from vanilla and 41 hacks
  ([lunar-magic.md](lunar-magic.md#mwl-files)); importing one into a project waits for
  the source format. An MWL is Lunar Magic's view of a level, not the ROM's: it rewrites
  screen exits, objects `3C`-`3F` in tileset 4, and pre-3.00 header bits on export, so
  import from an MWL and from the ROM can differ in those.
- 2b progress: `kobo_core::install` holds Kobo's clean-room patches (`asm/lunar-magic/`),
  applied through Asar and not yet used by builds. `map16.asm`: the Map16 routine behind
  `$06F540`, `$06F5D0`, and `$06F5E4` (each a `JML` to Kobo's code; the page tables are
  data at their fixed addresses), the seven hooks that call it, and tile generation that
  sets a page outright. Vanilla with it installed renders all 512 levels as vanilla does,
  and tiles on pages 2 to `7F`, and page 2 per tileset, resolve from tables where Lunar
  Magic's layout puts their pointers (`tests/install.rs`). The overworld entry keeps the
  game's behaviour; how Lunar Magic stores overworld pages past 0 is not known.
  `actslike.asm`: the gate, the acts-like chain, and the custom block actions, with
  GPS's entry blocks and slots where GPS expects them. Its behaviour was learned from a
  Lunar Magic-saved ROM by playing it against a logging GPS block
  (`examples/contact_probe.rs`), and matches it in every scenario tried; vanilla with both
  pieces renders all 512 levels as vanilla does.
- 2b starts with the one-time set. The first Lunar Magic-layout table Kobo writes needs the
  gate set, or Lunar Magic's install wipes it, and with the gate set Lunar Magic never
  installs the 15 one-time hooks and 95 one-time ranges itself. So Kobo provides all of
  them, clean-room, before any feature: the Map16 routine at `$06F540` and its four call
  sites, the BG Map16 and per-level flag hooks, and the rest of that list. The acts-like
  table pointer at `$06F624` is part of it, and GPS also patches the code around it (the
  entry slots from `$06F690`, the compare chain at `$06F67B` and `$06F717`, the exit at
  `$06F602`), so Kobo's code there has to have the shape GPS expects. This is the
  largest piece of 2b. The 32 hooks a save reinstalls need Kobo's own code only for the
  features Kobo supports.
- 2b then takes Lunar Magic-layout features one at a time, each through its source format,
  import from MWL and ROM, build, the Lunar Magic check, and the corpus check together, so
  neither direction anchors the format: Map16 pages 2 and up and background Map16, custom
  palettes, ExGFX, expanded level sizes, the sprite data formats (new sprite system, 255
  sprites, PIXI extension bytes), secondary entrances and exits.
- 2c: running the tools (user Asar patches, PIXI, GPS, UberASM Tool, AddmusicK), the
  companion build repository, and SA-1 builds with SA-1 Pack. PIXI and GPS need the
  one-time set, and PIXI Lunar Magic's VRAM patch at `$00F6E4`, so 2c follows 2b's start.

## Risks

- The one-time set's behaviour has to be worked out without reading Lunar Magic's code:
  from documentation (the help file documents the GFX decompression routine, the screen
  exit routine, and the Map16 acts-like code's contract and `JSL` slots; see
  [lunar-magic.md](lunar-magic.md)), tool sources, and the memory effects of running
  Lunar Magic-saved ROMs.
- Lunar Magic operations the spike did not try, the GUI's options especially, may write
  inside what Lunar Magic takes to be its own code at the fixed addresses, which in a Kobo
  build is Kobo's.
- Clean-room contamination, from an agent or contributor working from Lunar Magic's code.
- Lunar Magic's layout changing between versions: read many, write one.
- Import losing data silently. The import report and raw fields cover it.
- What a render does not show: exits, entrances, midway points, and secondary headers need
  the Lunar Magic check or emulator entry tests.
- The code GPS patches has to have a shape GPS's source describes, which pulls Kobo's
  bank `$06` code towards Lunar Magic's. It is written from GPS's source, the vanilla
  disassembly, and observed behaviour only, and reviewed with that in mind.
- AddmusicK overwrites `$0FF035`-`$0FF050`, which Lunar Magic's install fills. Lunar
  Magic's first save of a build with music writes its bytes back over AddmusicK's unused
  `$55` filler there and leaves AddmusicK's code and data alone
  ([lunar-magic.md](lunar-magic.md)). Every save rewrites `$0FF035`, and bytes below it
  that are not `$FF`, from the state of the one-time code; what they record is unknown
  ([lunar-magic-install.md](lunar-magic-install.md)).
- Lunar Magic's layout fixes operands inside code: the Map16 page table pointers in the
  `$06F540` routine, the secondary entrance table pointers at `$05DC81`-`$05DC8D` and
  `$0DE191`-`$0DE1A1`, which every save rewrites, so Kobo's code has to put them there.
- The tools: licences (three have none), directory-order dependence, and UberASM Tool on
  .NET outside Windows.
- The corpus is ROMs, not projects; every test project is made by exporting from Lunar
  Magic under Wine, and only hashes are committed.
- Drifting towards Lunar Magic parity.
