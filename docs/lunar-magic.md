# Lunar Magic ROM facts

What Lunar Magic changes in a ROM, as far as the library has to know: found from the formats
the community documents and from inspecting ROMs Lunar Magic produced, never from its
executable. Vanilla behaviour is in [smw.md](smw.md).

- Marker: `Lunar Magic Version X.YZ ...` as ASCII at `$0FF0A0` (`Rom::lunar_magic_version`).
- Map16 pages 0-1 stay in the vanilla tables (rewritten in place). Higher pages live in
  RATS-tagged blocks whose layout differs by Lunar Magic version; the routine at `$06F540`
  (called with A = tile*2, 16-bit; returns the pointer's low word in A and bank in `$0C`)
  resolves any layer 1 tile number. Call it on the core instead of parsing the blocks to
  draw a level. From 2.52 on (the corpus's 2.52 and 2.53 ROMs and every 3.x one) the
  blocks are one per group of 16 pages, found through the pointers at `$06F553` and on
  ([lunar-magic-install.md](lunar-magic-install.md)), and a group's block holds its pages
  up to the last one used, so the next group's may start inside the space a whole group
  would take, and a block ends at the last tile used, so the last page may stop part way;
  `map16::pages` and `import::read_map16` read them so. Super Dram World 2 (2.43) has other
  values there, which point at no block. The BG Map16 tables at `$0EFD50` are allocated
  the same way.
- The BG Map16 pages (`200`-`3FF`, Lunar Magic's file index `8000+`) are a separate block from
  layer 1 pages 2-3 and `$06F540` does not find them. Lunar Magic 2.3+ replaces `STA $0A` at
  `$058DA4` in the layer 2 tilemap upload with a `JSL` (to `$0EFD00`) that leaves the level's BG
  table pointer in `$0A`-`$0C`, chosen from a 3-byte pointer table at `$0EFD50` by the level's
  flags in `$7FC00B`. `expand` calls whatever the hook targets; 1.6x ROMs keep `$0D9100`.
  `LevelTiles::map16` holds foreground definitions (all of pages 0-3, resolved through
  `$06F540`, plus whatever higher numbers the grid uses); `bg_map16` holds background
  definitions. Keep these separate despite their overlapping tile numbers. The `palette png`
  and `map16 png` commands with `--level` show these and the loaded CGRAM and VRAM, which
  is what Lunar Magic's palette and Map16 editors show for a level.
- Per-level flags at `$0EF310` (copied to `$7FC00B` by the hook at `$05803B`): bit 1 marks a
  Lunar Magic background stored at the level's own layer 2 pointer, bit 2 a 32-row background
  whose buffer uses `$200` bytes per screen. The hook leaves that stride in `$05`;
  `LevelTiles::layer2_screen_len` carries it. Background indices can exceed `$1FF`; retain
  the raw index and load enough BG definitions instead of OR-ing in `$200`. Background
  presence follows the loaded level mode, since object levels can retain an `$FF` pointer
  bank. Upload rows wrap with a five-bit mask, not modulo 27. Grand Poo World 2's 32-row
  backgrounds pass the tilemap check; its unused level `$09F` has a null BG table pointer
  and is explicitly rejected with `MissingBackgroundTable`.
- Expanded level heights (Lunar Magic 3.00+, Vitor Vilela's dynamic level patch): a
  per-level byte `TB0MMMMM` (T: uses layer 2 or 3, B: show the bottom row, MMMMM: horizontal
  level mode) selects one of 32 sizes trading height for screens, from `$1B0` px x `$20`
  screens (mode 0, vanilla) through `$280` x `$16` (mode 7) to `$3800` x 1 (mode `$1C`);
  the full table is on SNESLab under "Lunar Magic/Custom Level Sizes". The loader hook
  (`JSL` at `$05D9A1`) stores the height in pixels in `$13D7` (vanilla leaves it zero), so
  `expand` derives rows per screen from it and lays screens out with a stride of
  `rows * 16` bytes; layer 1 objects are otherwise the vanilla format plus screen jumps
  (extended object `01`, and `03` for mode `$1C`). `tests/sprite_lists.rs` checks that
  sprites in expanded levels stay inside the level, tying the sprite Y jumps to this.
  A level with layer 2 objects splits the screens its height allows (`expand::LEVEL_SIZES`)
  between the layers: layer 1 takes the first half rounded up, and layer 2 starts right
  after it with the rest (47 rows: 19 screens, layer 2 from `0x1D60`; 298 rows: 3, from
  `0x2540`). Vanilla's 27 rows and 32 screens give the same `0x1B00`. Lunar Magic's
  dynamic tilemap upload (`$1F8000`, 20 unrolled column slots per layer, reading from the
  row above the camera) was traced to find this, and its BG2 tilemap cells agree with
  `LevelTiles::layer2_object_tile` on every corpus level checked.
- Lunar Magic 3's loader sets the screen count `$5D` independently of the header byte:
  Grand Poo World 2's never-saved levels all point at `$068000` (3 screens) yet load with
  2 to 17 screens, and level `109` with `$FF`. `LevelTiles::size()` bounds the grid, and
  `tests/sprite_lists.rs` tolerates the `$FF`.
- Custom level palettes: 3-byte pointers at `$0EF600` per level to `$202` bytes (back area
  colour, then 256 colours); `$000000`/`$FFFFFF` = none. Game mode `$12` loads them itself.
- ExGFX and Lunar Magic's 4bpp re-inserted GFX are handled by the game's own upload code, so
  capturing VRAM during game mode `$12` covers them without knowing the tables.
- That upload code (`$0FF8xx`-`$0FFExx`) decompresses files larger than the game's buffer at
  `$7EAD00` can take: up to `$2000` bytes, which run over the background (`$7EB900`) and the
  first three screens of the tile grid (`$7EC800`-`$7ECCFF`). It parks `$7EBD00`-`$7ECCFF`
  in VRAM first (a DMA to `$2118`), and reads it back afterwards: `VMADD`, a 16-bit read of
  `$2139` to get past the port's read-ahead, and a DMA from `$2139`-`$213A` (control `$81`).
  The bus has to model the VRAM read port, its latch, and DMA towards the A bus for that;
  without them the grid's first three screens stay graphics data (Akogare2 level `008`).
- A hack's own code may write `TM`/`TS` (`$212C`-`$212D`) directly. The game copies them from
  their mirrors (`$0D9D`-`$0D9E`) once per level load and never again, so such a write stays
  in force: Akogare2 level `008` loads with `$15`/`$02` in the mirrors and then puts layer 2
  on the main screen (`$17`) from `$91CBF2`, in front of an opaque layer 3.
  `video::Screen` takes the two from the registers for that reason, and colour math from
  the mirrors, which go out every frame.
- A ROM locked by its author has `JSL` to a short routine at the start of the decompression
  routine (`$00B8DE`), which changes the pointer in `$8A` before the file is read, so the GFX
  pointer tables do not hold addresses. The files themselves are ordinary LC_LZ2. Seven
  `.smc` ROMs of the corpus are locked (Invictus, both Super Dram Worlds, Smb2dx, Baby Kaizo
  World 3, two of the `SMW_2021` set) and one QLDC entry. `gfx::is_locked` recognises the routine and the GFX tooling refuses with
  `GfxError::Locked`; levels load regardless, since the ROM's code runs.
- Lunar Magic can store a ROM's GFX and ExGFX as LC_LZ3 instead of LC_LZ2, all files at
  once. It then puts `JSL` to its own routine, in a RATS block, at `$00B8E3` inside the
  game's decompression routine, which it does for its faster LC_LZ2 routine as well (and
  SA-1 Pack for its own), so the hijack does not say which format. Lunar Magic 3.70
  records it at `$0FFFEB` (`$00` LC_LZ2, `$01` its faster LC_LZ2, `$02` LC_LZ3; vanilla
  `$FF`), which SA-1 Pack reads; whether older versions do is unchecked, so detection
  stays. `gfx::Compression::detect` decodes the 50 table files both ways and takes the
  format more of them come out whole in: 46-50 for the right one, at most 18 for the other
  (a file of copies and fills alone reads the same in both). LC_LZ3 is `compress::lz3`:
  command 3 is a zero fill with no operand, 4-6 copy from the output (as is, bits
  reversed, backwards), and their source is a 15-bit big-endian offset or, with bit 7 of
  its first byte set, seven bits counting back from the last byte written. One hack of the
  corpus uses it: QLDC 2021 `34_idol`.
- FastROM patches run the whole game from banks `$80` and up (Super Riff World 1.4 reaches
  its game loop at `$80806B`). `SmwBus::code_mirrors` gives both addresses of a routine for
  anything that waits for the program counter to get somewhere.
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
- Sprite load flags: vanilla keeps one per entry at `$1938` (128). ROMs where Lunar Magic 3
  installed its 255-sprites-per-level patch have a `JML` over the loader's flag check at
  `$02A856` into code that uses `$7FAF00` (256 entries) instead; `capture_sprites` detects
  that and clears or sets whichever table the loader reads. With only `$1938` cleared, the
  entrance screen's sprites never respawned in those hacks and came out as markers.
- SA-1 hacks run on a second CPU; see [sa1.md](sa1.md). On one, SA-1 Pack's own loader hook is
  at `$02A856`, not Lunar Magic's, and the flags are wherever the RAM map puts `$1938`.

- Level data it adds (layouts from the community's documentation of the format, checked by
  round trip on every level of the corpus, `tests/level_data.rs`): objects `22`/`23`
  (direct Map16, 4 bytes), `27`/`29` (Map16 objects, 5 to 8 bytes: 5 when the fourth
  byte's top two bits are `00` or `01`, 6 for `10`, 7 or 8 for `11` by bit 7 of the third
  byte), `2D` (user objects, 5 bytes), `24`-`26` and `28` (settings with no place),
  extended `02` (a 5-byte screen exit with a 13-bit destination), and extended `03`, a
  screen jump for level heights past 16 units. From 3.00 a screen jump's second byte
  carries a vertical part in units of 32 rows. Exactly the 3.x ROMs of the corpus have
  `JSL` at `$05D9A1` (`level::LevelFormat`).
- Every Lunar Magic ROM of the corpus, 1.62 to 3.51, has its install gate `$06F600` set
  (`$EA`, or `$68` in the 1.62 and 2.41 ROMs). All of them have the sprite pointer bank
  table at `$0EF100`: every level's sprite list parses from `$0EF100`'s bank and the
  low word at `$05EC00`, in bank `$07` for untouched levels and in a RATS block for the
  rest.
- Background layouts by the flags at `$0EF310` (`bbBBVFCT`), as the corpus has them: a
  layer 2 pointer with bank `$FF` is the game's format in bank `$0C`, flags aside; `V` is
  a vanilla background behind a full pointer, 864 bytes; `C` with `F` is Lunar Magic's
  own, one LC_RLE1 stream of 2048 bytes (32 rows to a half, low bytes then high bytes),
  in a RATS block; `C` without `F` decodes to 864 bytes. Lunar Magic 3.51 rewrites every
  vanilla background pointer to a full one with `V`; older versions leave bank `$FF`.
- Lunar Magic opens and saves a step 2a build without loss (`tools/lunar-magic/save-check`,
  2026-09-25): on the first save it installs itself (the gate, the 3.x hooks, the sprite
  bank table), and every level reads back as Kobo wrote it. The level it saves is
  re-encoded, and its screen exits rewritten in its own format: `u` set, and `h` from the
  level number, which is the destination's bit 8 the game's format leaves implicit
  (`ScreenExit::in_lunar_magic_format`). Its restore system will not change a ROM it
  does not recognise unless `sysLMRestore/smwOrig.smc` beside the ROM holds the original
  game with a copier header; the script puts one there.
- Lunar Magic's first save points the background of every level on the shared empty level
  (`$068000`) at `$FFDE54`, as its MWL export does: 276 of the 277 such levels of a vanilla
  ROM change, whoever built it. `save-check` with a project compares only its levels.
- Its first save sets bit 3 of `$05FE00` (its copy of the destination's bit 8) in every
  secondary entrance from `100` on, used or not, and zeroes the never-used entrances the
  game leaves pointing at level `000` with other bytes set (`0CE`, `0CF`, ...). Kobo does
  not keep bit 3 in the source, counts an entrance in use without it, and `save-check` of
  a project that defines level `000` reports that level's entrances.
- On a build with AddmusicK's music, the first save writes Lunar Magic's own bytes over
  `$0FEF9F`-`$0FF050`, which AddmusicK filled with `$55` and left unused; AddmusicK's code
  and data (`$0E8000`, its RATS blocks) are untouched (2026-09-25, AddmusicK 1.0.11).
- An MWL export records where the level's data was in the ROM, so exports of the same
  level from two ROMs differ there (see "MWL files" below). A level whose layer 1 is the
  shared empty level at `$068000` exports with the background at `$FFDE54`, so exports of
  such a level from a vanilla ROM and from a build that moved its layer 1 differ in the
  background too.
- ROMs locked by their authors (see below) add objects past the level's last screen:
  Baby Kaizo World 3's level `014` has 8 screens and 779 objects, running to screen 48 and
  back, which no screen jump can express. `tests/level_data.rs` skips them.

## MWL files

Lunar Magic's single-level file ("Save Level to File", `-ExportLevel`, `-ExportMultLevels`),
read and written by `kobo_core::mwl`. The layout is the community write-up "MWL File
Format" (kaizoman666's SMW-Data repository, accurate to 3.63) with SMW Speedruns' level
data format page, both saved in `~/.local/share/kobo/docs/`. What follows was checked on
every level of the vanilla ROM and of 41 corpus hacks (Lunar Magic 1.62 to 3.51, two of
them SA-1) exported by 3.70: 21,504 files, against the ROM each came from
(`tests/mwl_files.rs`, [testing.md](testing.md)). The help file says only that an MWL holds
the level, its background, sprites, palette, secondary entrances "and a few other things",
and no graphics, Map16, or shared palettes. Lunar Magic refuses to open the seven locked
ROMs of the corpus and Smb2dx ("Requested operation failed").

- Container, confirmed: `"LM"`, the version as a word (`70 03`), the section table at `$40`,
  `$40` long, flags `00 00 00 00`, and the comment `Lunar Magic 3.70  ©2026 FuSoYa  Defender
  of Relm` (Latin-1). Eight sections follow the table in order with no gaps, the last
  ending the file. Exporting again gives the same bytes.
- Level information, 64 bytes: the level; the game's four secondary header bytes, equal to
  the ROM's; `$05DE00`; two zero bytes; the four midway entrance bytes and a zero; then
  `$06FC00`, `$06FE00`, the level size byte, and `$06FA00`, equal to the ROM's where its
  version has them (3.00, 3.40); the rest zero.
- Layer 1 and layer 2, sprites, palette, secondary entrances, and ExAnimation start with 8
  bytes: byte 0 is the section's own, bytes 4-6 where the data was in the ROM (the layer 1
  pointer, always), the rest zero. Layer 1's byte 0 is 1 exactly when `$0EF600` has a
  custom palette for the level; layer 2's is the level's `$0EF310` flags as Lunar Magic 3.70
  writes them; ExAnimation's is `$03FE00`.
- Object data is the ROM's format and decodes to the ROM's objects, with screen jumps as
  Lunar Magic 3 reads them whatever the ROM's version. A background is 2048 bytes of
  16-bit tiles, the left half's 32 rows of 16 first, uncompressed, and equals
  `level::Background::tiles` of the ROM's (27-row formats pad with tile 0). Its flags are
  `V` and `F` (`$0C`) for the game's format, and `C` and `F` for Lunar Magic's: the ROM's
  top nibble (`BB`, the BG Map16 bank) is kept if it had `F`, and folded into the tiles'
  high bytes and cleared if not. Layer 2 objects or a background follow the flags, not the
  level mode: boss levels export their pointer's background.
- Sprite data is the ROM's list byte for byte, from the level's sprite pointer, and parses
  to the same sprites given the ROM's PIXI size table, which the file lacks.
- The palette section holds 256 colours and then the back area colour: a custom palette
  equals the ROM's (which has the back area colour first). Without one, it is the palette
  the header selects as the editor shows it, `palette::vanilla_level_palette` but for row 8,
  where the editor puts the player's colours, and rows 0-1 colours 8-15, which the title
  screen (level `0C7`, as the help file says) and some layer 3 settings replace.
- Secondary entrances: 8 bytes each, the number, `$05FA00`, `$05FC00`, `$05FE00`, two
  bytes of Lunar Magic 3's tables, and a zero. A level gets every entrance in use whose
  destination is the level, destination bit 8 being bit 3 of `$05FE00` in Lunar Magic ROMs
  and the entrance number's bit 8 in vanilla, and in use meaning any of its four bytes
  nonzero. That does not hold for levels `000` and `100`, where the table's never-used
  entries point, some with other bytes set; which of those Lunar Magic exports is not known.
- ExAnimation data is the ROM's bytes at the header's address; the address is the ROM's
  pointer as is, `$0000FF` (middle byte zero) meaning none.
- ExGFX: sixteen words in the write-up's slot order; the vanilla ROM's levels all have `7F`
  but SP4 `FFFF` and LG1-LG4 `28`-`2B`.

Lunar Magic rewrites some things on export, which an import from the ROM will not see:

- Screen exits come out in its own format (`u` set, `s` per exit, `h` the level's bit 8),
  and not in the ROM's order.
- In object tileset 4, objects `3C`-`3F` are rewritten: vanilla level `0BD`'s `3F` with
  settings `F1` becomes `F0` and three more `3C`/`3F` objects, and `3D` settings `2F` become
  `1F`. That is eleven vanilla levels (their copies in the hacks), and no other object
  of any level is changed.
- Level `0C5`'s vertical scroll setting 0 becomes 3, the only primary header change.
- From a ROM before 3.00: layer 2 scroll setting 8 becomes 3; the Y bit of `$05DE00`
  (`IWPYX---`, bit 4) and of `$05FE00` (`IPYXDAAA`, bit 5, when `P` is set) moves to bit 0
  of `$06FC00` and of the entrance's first Lunar Magic 3 byte, except in vertical levels,
  which keep it where it was; Lunar Magic 1.62's missing `$03FE00` (`$FF`) becomes 0; and
  Lunar Magic 2.41's ExAnimation comes out in the current format (Kaizo Mario World 3,
  seven levels).
- Bit 3 of `$05FE00` is set to bit 8 of the destination.
- A level whose layer 1 is vanilla's shared empty level (`$068000`) gets the background at
  `$FFDE54` whatever its own pointer is. A background stream shorter than its format
  (Kaizo Mario 2 level `1C7`, 743 bytes of 864) comes out with other tiles past its end.

Not confirmed:

- The midway entrance bytes, the level size byte, the entrance's two Lunar Magic 3 bytes,
  and the ExGFX files were not compared with the ROM: the write-up locates their tables
  through pointers in what Lunar Magic installs (`read3($05D9E4)+$0A`,
  `read3(read3($05D9A2)+70)`, `read3($05DC86)`, `read3($0FF7FF)`), some of them surely
  operands of its code, which Kobo does not read. Their layouts are the write-up's.
- A ROM whose secondary entrance tables Lunar Magic moved (entrances past `$1FF`, Super
  Riff World 2) is not compared; the tables are behind such pointers too.
- No flag bit but SMA2 is known, and no SMA2 file was seen; nor any file from another
  Lunar Magic version, whose layout may differ. Files before 1.60 were several files and
  are not read. What Lunar Magic does with each section on import was not tested.

## What Lunar Magic installs, and how it decides

Found with Lunar Magic 3.70's command line under Wine, by byte diffs of ROMs before and
after a save and by changing bytes and saving again (the hook spike of
[step-2.md](step-2.md)); none of it comes from reading Lunar Magic's code.
`tools/lunar-magic/` has the wrapper and the region diff used.

- The first save into a vanilla ROM (here `-ImportLevel` of level `105`'s own MWL) expands
  it to 1 MiB and writes `JSL`/`JML` at 52 sites in vanilla code, changes about 160 other
  ranges of vanilla code and data, fills fixed parts of vanilla's unused space (`$05DC50`-
  `$05DFFF`, `$06F540`-`$06FFFF`, `$0DE190`-`$0DE24F`, `$0EF100`-`$0EF56B`,
  `$0EFD00`-`$0EFD7F`, `$0FF035`-`$0FF13F`, among others), adds 13 RATS blocks from
  `$108000`, and writes a 64-byte marker at `$0FF0A0`. It does not update the internal
  checksum. The output is the same byte for byte on every run, and saving the same level
  again changes nothing.
- The marker is not what Lunar Magic reads to decide what is installed. With it removed, a
  save writes it back and changes nothing else, and a ROM with no hooks gets the full
  install whether or not it carries the marker.
- There is no single gate. A save decides piece by piece whether its install is there,
  each piece by a check of its own: `$06F600` other than `$FF` for the Map16 routine and
  the acts-like chain in bank `$06` (`$00`, `$42`, and `$5C` there all count), a `JSL` at
  one of its hook sites for most others, some of them hooks a save restores
  ([lunar-magic-install.md](lunar-magic-install.md#how-a-save-decides-what-to-install)
  has the map). The spike found only `$06F600` because it put a whole Lunar Magic ROM
  back to vanilla in halves, where every other check was still met.
- With the gate set, a save repairs only part of what is missing. It reinstalls 32 of the
  52 hooks as new copies of its code in fresh space, with the jumps retargeted, and puts 5
  back in place (`$00A6B8`, `$00A6CC`, `$0583C7`, `$05D8F5`, `$05D97D`); of the other
  ranges it restores 52 and 16 in part. The rest only the one-time install writes: 15
  hooks (`$00C17A`, `$00C25C`, `$02BA9E`, `$04DCFA`, `$04E5F1`, `$05803B`, `$058A65`,
  `$058B45`, `$058C33`, `$058D2A`, `$058DA4`, `$05D7CE`, `$05D8E2`, `$05DB5B`,
  `$05DBC2`) and 95 other ranges, `$695` bytes, in banks `$00`-`$06`, `$0D`, and `$0E`.
  [lunar-magic-install.md](lunar-magic-install.md) has what each of them replaces, what it
  is for, and what it leaves behind.
- It never checks the code behind a hook. Foreign bytes at all 46 hook targets survive a
  save, and all 52 sites retargeted to a foreign RATS block count as installed, with the
  block kept.
- A piece that a save installs reinitialises its tables over whatever is there: a custom
  palette imported for level `105` lost its pointer at `$0EF600` and its space was reused. Vanilla-format data elsewhere survives it: level `105`'s layer 1 moved
  into a RATS block at `$118000`, with its pointer at `$05E000` retargeted, kept both.
- Lunar Magic warns "The ROM may be Corrupt!" when the internal checksum is wrong, and
  "This isn't a fresh ROM!" when it is right but the image is not vanilla. The command
  line answers both on its own; the GUI shows them.
- Only command-line level and palette imports were tried. Whether other operations, the
  GUI's options in particular, write inside what Lunar Magic takes to be its own code is
  not known.
- Lunar Magic's help file (`Lunar Magic.chm`, "Technical Information") documents three
  entry points into its code, which other code calls or patches and so are interface:
  `JSL $0FF900` decompresses GFX or ExGFX file A (16-bit) to the 24-bit address in `$00`;
  `JSL $03BCDC` returns the screen Mario is on for screen exits in X; and the Map16
  "acts like" code has room for three 4-byte `JSL`s at each of `$06F890`-`$06F9F0`
  (file offsets `0x37890`-`0x379F0`, one per kind of contact), which block tools write
  into. There, A/X/Y are 8-bit, X and Y must be preserved, Y and `$1693` hold the tile
  number reported to the game after the acts-like chain (always below `$200`), and `$03`
  holds the last tile number of the chain (up to `$7FFF`).
