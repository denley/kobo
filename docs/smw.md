# Super Mario World facts

What the library knows about the vanilla game: ROM tables, how a level loads, how `expand`
runs the game's own code to get at it, and how the picture is put together. Facts specific to
Lunar Magic's ROM changes are in [lunar-magic.md](lunar-magic.md); what the renderer does not
reproduce is in [known-gaps.md](known-gaps.md). Labels such as `CODE_05D796` and `LoadLevel`
are SMWDisX's.

## ROM tables

- Level pointer tables: layer 1 at `$05E000` and layer 2 at `$05E600` hold 3-byte pointers,
  0x200 levels each. Sprite pointers at `$05EC00` are 2 bytes each, implicitly bank `$07`.
- A layer 2 pointer with bank `$FF` marks a background tilemap; the game substitutes bank `$0C`.
- Level 105 (Yoshi's Island 1) layer 1 data starts at `$0688DD` in vanilla.
- GFX file pointers for `GFX00`-`GFX31`: low bytes at `$00B992`, high at `$00B9C4`, bank at
  `$00B9F6`. Lunar Magic keeps these tables and rewrites the entries (often into FastROM
  banks `$80+`). Data is LC_LZ2. Vanilla files are 3bpp (2bpp for `27`-`2B`, `2F`); Lunar
  Magic re-inserts files as 4bpp, so bit depth is inferred from decompressed size and the
  file's fixed tile count (128, except `2F`-`31` = 64). `GFX27` is the Mode 7 tiles of Iggy's
  platform and the Reznor sign: 128 tiles of 3-bit pixels packed most significant bit first,
  three bytes to a row of eight, which `CODE_00AB42` unpacks a pixel to a byte into the high
  bytes of VRAM (`gfx::GfxFormat::Packed3`).
- `GFX32` (Mario, 744 tiles, stored 4bpp) and `GFX33` (animated tiles, 384 tiles, stored 3bpp)
  are LC_LZ2 like the rest but have no pointer table entry: `CODE_00B888` loads them from
  immediates, `LDY #GFX33` at `$00B88A`, the bank of both in the `LDA #` at `$00B88F`, and
  `LDA #GFX32` at `$00B8D7`. It decompresses `GFX33` to `$7E2000`, widens it to 4bpp at
  `$7E7D00`, then decompresses `GFX32` to `$7E2000`. Lunar Magic rewrites the three operands
  when it moves the files, stores `GFX33` as 4bpp, and replaces the widening loop with a
  direct decompression to `$7E7D00`.
- The game's LC_LZ2 routine is `CODE_00B8DE`, reading through `ReadByte` (`$00B983`) from
  `$8A` into `[$00],Y`; `PrepareGraphicsFile` (`$00BA28`, output `$7EAD00`) and
  `CODE_00B888` call it. It agrees with `compress::lz2` on commands 0-4, short and long
  headers, word fills of odd length (they end on the first byte), incrementing fills
  (8-bit, so they wrap), and back-references copied a byte at a time from an offset into
  the output, so one overlapping its own output repeats. It is more lenient in two ways,
  which `compress::lz2::decompress` rejects and `compress::lz2::compress` never writes:
  any command with bit 2 set is a back-reference (5, 6, and a long header's 7, `$FC`-`$FE`),
  and a back-reference to bytes not yet written reads whatever the buffer holds. The
  offset is big-endian in the US version (`TAX` at `$00B96D`); SMWDisX assembles an extra
  `XBA` before that `TAX` for the Japanese and E1 versions, which makes it little-endian.
  `ReadByte` goes on at `$8000` of the next bank when the address wraps, so a stream may
  cross a LoROM bank boundary. The vanilla files are not optimally packed: `compress::lz2`
  stores the 52 in 121,663 bytes against the ROM's 130,317, and the game reads them back
  (`tests/lz2_compression.rs`).
- The game's `UploadGFXFile` sets the fourth plane to the tile silhouette for the first 16x16
  block of `GFX01`/`17`/`31` (the berry, drawn with colours 9-F) and for all of `GFX1E` (and
  `GFX08` in tilesets `$11+`). Lunar Magic's export mirrors this except it skips `17` and flags a
  fixed subset of `08`; see `gfx::upper_palette_tiles` vs `gfx::vram_upper_palette_tiles`.
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

## Entering and loading a level

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
  reloading. The screen-exit entry skips it; the oracle script sees the game choose it (an
  exec callback at `$05DA65`, past every check that skips it) and dumps the second level
  frame.
- `expand::expand_level` seeds the RAM-resident OAM routine by running the reset code, sets
  the frame counter `$13` to `$40`, and then runs game mode `$11` in the game's order:
  `CODE_05D796` (header pointers and entrance),
  `$1A`-`$21` copied to `$1462`-`$1469`, `CODE_00A635`, `$5E = $20`, `CODE_00A796`, one
  `UpdateScreenPosition` with vertical scrolling at will on (`$1404`), and only then
  `CODE_05801E` (clear buffers, `LoadLevel`), which ends by spawning the sprites around the
  camera (`CODE_02A751`) and running them once. The order matters to both: the scroll at
  will lets that first camera update jump to the player where the header's layer 1
  position would leave him off the screen (vertical levels `0DB` and `12A` start 192 pixels
  higher for it, and layer 2 with them), and sprites spawned before `CODE_00A635` has placed
  the camera are those of screen 0. Then `CODE_00B888` (GFX32/33 to RAM) and all of
  game mode `$12` (`GM12PrepLevel`, `$00A59C`), which draws boss floors, sets up layer 3 (tides
  zero rows 16-26 of the layer 2 screens), and uploads GFX, palettes, and initial tilemaps.
  `$0100` is set to `$11` and then `$12` on the way: Lunar Magic's replacement for the initial
  tilemap upload (`CODE_05809E` jumps to `$1FB1E8`) checks the game mode and uploads nothing
  under a stale value. The console's vertical blanks between those frames run too, each the
  ROM's whole NMI handler with the lag flag `$10` clear: once with the mode at `$11` (game
  mode `$10` ends by setting it, and `GM11LoadLevel` keeps NMIs off for its own frame),
  once after `LoadLevel` with the mode at `$12` (`GM11LoadLevel` increments it before
  `Mode04Finish` turns NMIs back on), and once after `GM12PrepLevel` with it at `$13`. Code
  a hack hooks into the handler sets up and consumes its queues there (QLDC 2021 `70_DPBOX`
  empties a DMA queue under mode `$11` and runs it under `$12`; without the first, a stale
  count DMAs 64 KiB over bank 0), and Lunar Magic's tilemap uploads for some levels only
  reach VRAM through the third (Akogare2 `0F8`, Luminescent `0F8`). The level's screen
  designation (`$212C`/`$212D`) is read before that third blank, as `GM12PrepLevel`'s
  `ScreenSettings` left it: vanilla's handlers never write it, a Mode 7 arena's write a
  band's, and Super Hark Bros 2's NMI writes one from a flag that shares a byte with a queue
  it is also filling, which the frame the emulator shows has clear again.
  The bus captures VRAM/CGRAM port and DMA writes, so rendering uses what the game uploaded:
  ExGFX, custom palettes, and animated tiles come for free. VRAM matches the emulator except
  animated slots (frame-dependent) and tilemap areas filled on later frames; animated
  colours likewise depend on the captured frame. CGRAM `$64` (row 6, column 4), used
  by the dragon coin's Map16 tiles `$002D`/`$002E`, loads as `$7C3F` (RGB `#FF08FF`)
  in vanilla level `105`. The regular level NMI calls `CODE_00A390`, whose tail at
  `$00A418` replaces it with a flashing yellow from `FlashingColors` (`$00B60C`), at
  byte offset `($14 & $1C) >> 1`. The player pass runs the whole NMI after each frame
  and retains its character and palette uploads, so the level's palette includes this update even
  when rendering with the player and sprites hidden. Calling only OAM and player
  graphics uploads left the dragon coin magenta; sprite-pass NMIs alone cannot fix
  it because those passes use separate video memory.
- Capture the screen count (`$005D`) immediately after `LoadLevel`: boss preparation
  overwrites it (level `$1C7` ends with `$FF`). `LevelTiles::size()` also bounds dimensions
  to complete screens in the captured grid planes.
- The bus models the CPU multiply/divide registers (`$4202`-`$4206`, `$4214`-`$4217`);
  sprite code uses them constantly. The DMA channel registers read back (`$43x0`-`$43x6`; a
  transfer leaves its size at zero and its source past the last byte). A LoROM cartridge's
  save RAM is at `$70`-`$7D` and `$F0`-`$FF` below `$8000`, as much of it as the header
  declares, mirrored across the window, and starts zeroed like the oracle's; QLDC 2021
  `77_NerDose` builds a routine there and calls it (`$70210E`).
- A routine that stops to poll memory for something the console's vertical blank would
  bring (`LDA $10 : BEQ -`, a frame wait inside sprite code) gets the ROM's NMI handler
  then and there, as the console gives it, up to 16 times in one run (`Cpu::run`,
  `Bus::vblank`); a wait the handler does not end is given up as one nothing will.

## Level data

- Object data (`kobo_core::level::objects`): a five-byte header (layer 1's is the primary
  header; `LoadLevel` skips layer 2's), then objects in drawing order until a first byte
  of `$FF`. `NBBYYYYY bbbbXXXX` plus a settings byte: `BBbbbb` is the object, `N` moves
  the current screen (`$1928`) on by one before the object is placed. Object `00` is an
  extended object numbered by its third byte: `00` a screen exit, four bytes, `000ppppp
  0000wush 00000000 dddddddd` (the exit's own screen, flags, destination low byte;
  `ExtOBJScreenExit`), and `01` a screen jump, which sets the screen to the first byte's
  low five bits (`ExtOBJScreenJump`). Extended objects `02`-`0F` have null handler
  pointers.
- On a vertical layer (`VerticalTable` at `$058417`: bit 0 layer 1, bit 1 layer 2; layer 1
  is vertical in modes `03`, `04`, `07`, `08`, `0A`, `0D`) `CODE_0585D8` swaps the two
  place nibbles of every object but extended `00` and `01`: the first byte's low five bits
  are the column across the 32-tile screen, the second byte's low nibble the row.
- Nintendo's data has redundant screen jumps: a jump to the screen already current, or to
  one a new-screen bit would reach. Eighteen of the 538 object lists have one, so Kobo's
  encoder, which uses the bit where it can, writes those shorter; the other 520 come out
  byte for byte.
- Background tilemaps are LC_RLE1 (`compress::rle1`), decompressed by `CODE_058126` into
  `$7EB900`: the left half's 16 columns by 27 rows, then the right half's, as low bytes.
  The routine stops when the two bytes after a chunk are `$FF $FF`, so a first chunk is
  always read and no chunk can begin with them. `CODE_05801E` takes the Map16 page for all
  of it from the data's address: 1 at or past `$0CE8FE`, else 0. Some of the 17 vanilla
  backgrounds decode one byte past their 864. Nintendo's encoding is not the shortest.
- The secondary header is one byte from each of the tables at `$05F000`, `$05F200`,
  `$05F400`, `$05F600`: `hhhhyyyy 33AAAxxx MMMMffbb NUVEEEEE` (`level::SecondaryHeader`).

## The tile grid and Map16

- Tile grid layout: horizontal levels are 16x27 per screen, screen after screen. Vertical
  levels are 32 wide; each screen is 16 rows stored as a left and a right 16x16 half.
- Layer 2 objects live in the upper part of the same grid planes, with a layout chosen by
  level mode independently of layer 1 (`CODE_058883` dispatch, screen tables at `$00BB08`
  and `$00BC16`): modes `01`-`04`, `0F`, and `1F` use 16 horizontal screens from plane offset
  `$1B00`; modes `05`-`08` use 14 vertical screens from `$1C00`. Modes `03`/`04` pair a
  vertical layer 1 with a horizontal layer 2. The upload resolves the tile numbers through
  the layer 1 Map16 pointer table (`$0FBE`), not the BG table, and ORs `$1000` (palette
  row + 4) into every word when the object tileset is 3. `expand::Layer2Objects` and
  `LevelTiles::layer2_object_tile` encode this. The layer 2 background tilemap is decoded into
  `$7EB900`/`$7EBD00` (two screens); its tile numbers index the BG Map16 (`200+`). The buffer
  is captured right after `LoadLevel`: game mode `$12` decompresses GFX into `$7EAD00`, and a
  4bpp file (Lunar Magic) overruns the 3bpp-sized buffer into `$7EB900`. The game has
  uploaded the tilemap to VRAM by then and does not notice.
- Vertical pipe tiles `133`-`13A` have four definitions each: the initial tilemap upload
  (`CODE_0580BD`) and the scroll setup (`CODE_05877E`) re-point them in the `$0FBE` table
  from `MAP16AppTable` (`$058776`: `$8AB0`, `$84E0`, `$8AF0`, `$8B30` in bank `$0D`) for
  every column they upload, variant `(column / 8) % 4` (rows in the initial upload of a
  vertical level), so a pipe's colour depends on where it stands (the ghost ship's pipes
  are grey, Yoshi's Island's green). `LevelTiles::map16_at` applies this; `map16` alone
  holds the table's final state. Lunar Magic ROMs replace the pointer-table lookup in the
  column upload (`$058A65`: `TAY : LDA $0FBE,Y` becomes `JSL $06F540`) with the pointer
  routine for every tile number, so the re-pointing has no effect there and `expand`
  resolves all tiles through `$06F540` in those ROMs.
- Vertical levels with a background (mode `$0A`) keep layer 2 horizontal (`$5B` bit 1
  clear): `CODE_058955` dispatches to the same column upload as mode `$00`, so the two
  screens sit side by side across the level's 32-tile width, 27 rows tall, in a 64x64
  tilemap (`BG2SC = $33`) whose last five Map16 rows are never written. The game scrolls
  layer 2 slowly so those rows never show; the renderer tiles the background down the
  level instead. `Ptrs00BDE8`/`$00BE68` route mode `$0A` to the vertical object tables,
  but only `CODE_058883` (object modes `$05`-`$08`) uses those.
- Tilemaps: `BG1SC`-`BG4SC` are captured in `VideoMemory::bg_sc`. Vanilla puts layer 1 at VRAM
  word `$2000` and layer 2 at `$3000`, both 64x64 tiles, and uploads the whole two-screen
  background. Lunar Magic uses `$3000`/`$3800`, 64x32, and uploads only the 15 or 16 rows
  around the initial scroll position. `VideoMemory::vram_written` says which bytes were touched.


## Layers, screen designation, and colour math

- A level mode (`$1925`, five bits of the primary header) decides what layer 2 is, and
  `level::LevelMode::layer2` is the one place that says so: a background tilemap the loader
  decodes (`$00`, `$0A`, `$0C`, `$0D`, `$0E`, `$11`, `$1E`), horizontal objects (`$01`-`$04`,
  `$0F`, `$1F`), vertical objects (`$05`-`$08`), or nothing the loader builds (the boss arenas
  `$09`, `$0B`, `$10`, and the modes the game does not define). `expand` captures the
  background buffer only in background modes, and the renderer asks
  `LevelTiles::shows_background`, so a buffer held under any other mode is never drawn (the
  arenas and the dark rooms sharing their tilemap, `$0F`, never upload one). Orientation,
  screen designation, and colour math also follow the mode, but through the ROM's own tables,
  which the game's code reads; they are taken from RAM after loading, not restated.
- Screen designation and colour math come from three per-level-mode tables in `LoadLevel`
  (`LevMainScrnTbl`, `LevSubScrnTbl`, `LevCGADSUBtable` at `$0581E0`, `$058200`, `$058220`;
  mirrors `$0D9D`, `$0D9E`, `$40`), with `CGWSEL` (`$44`) at `$02` (add the subscreen, fixed
  colour where it is transparent) and `COLDATA` fed from the back area colour `$0701`.
  CGRAM colour 0 stays black: the "back area colour" is the fixed colour, added to the
  backdrop. Most modes put layers 1 and 3 and objects on the main screen and layer 2 on
  the subscreen with `$24` (backdrop and layer 3 add the subscreen), so layer 2 shows only
  through transparent pixels whatever its priority bits, and a tide is translucent over
  it. Mode `$02` (and `$06`, `$08`, `$11`) puts layer 2 on the main screen (`$17`/`$00`),
  where priorities interleave. Modes `$0C`/`$0D` use `$70`: objects and the backdrop
  half-add layer 2 (dark background, translucent Boos), except objects on sprite palettes
  0-3, which never take part in colour math, and pixels over a transparent subscreen,
  which take the fixed colour unhalved. Mode `$0E` shows only layer 3 on the main screen
  and adds everything else from the subscreen (`$04`/`$13`/`$24`). Mode `$11` is the
  spotlight room: `$FF` subtracts and halves everything against the fixed colour, and the
  spotlight sprite (`C6`) sets `$44 = $20` (prevent math inside the colour window) and
  drives the window by HDMA; no window is modelled in a scrolling level, so the room renders
  dark throughout.
  Modes `$1E`/`$1F` put only layer 1 or only layer 2 on the main screen and add the rest.
  `render::LevelLayers` keeps BG1, BG2, BG3, and objects as separate colour-index layers
  and composes them per pixel from `LevelScene::screen` (`video::Screen`), which is also
  how sprites end up in front of or behind layers. Windows are modelled for boss arenas
  only (see below); the spotlight, keyhole, and message box windows are not.
- Layer 2 position: the entry camera is `$1A`/`$1C` and layer 2 sits at `$1E`/`$20`, which
  `UpdateScreenPosition` (`$00F6DB`) derives every frame from layer 1 and the layer 2 scroll
  settings `$1413`/`$1414` (same, half, or a fraction plus the offset `CODE_00A796`
  computes at load). Game mode `$11` copies `$1A`-`$21` to `$1462`-`$1469` right after
  `CODE_05D796` and runs that update once before loading; `expand` does the same (without
  enabling vertical scroll-at-will, which would start the camera drifting towards the
  player), and once more after game mode `$12`, as the level loop does before the first
  frame is shown: what game mode `$12` leaves in `$1E`/`$20` is not always what that
  update derives (vanilla `0D0`, `0D1`, `0F5`, `0F6`, and `122` move by a few pixels, and a
  hack's level-init code can leave anything there). Ghost houses (`02`/`03`) end up with layer 2 18 pixels up and at half speed.
  The renderer draws layer 2 and the scrolling axes of layer 3 where the entry camera sees
  them and continues them unstretched across the level, so parallax layers keep the entry
  screen's phase; an axis layer 3 does not scroll on repeats per screen horizontally and
  stays in the entry band vertically.
- Layer 3: the secondary header byte at `$05F200` (bits 7-6, `$1BE3` after loading) picks
  one of three settings per object tileset from `Layer3TilemapSettings` (`$009F88`, applied
  by `CODE_009FB8` in game mode `$12`): `1`/`2` are tides (up-and-down at `$24 = $70`,
  stationary at `$40`), `$80`/`$C0` fixed backgrounds at `$D0` (`$13D5` non-zero stops all
  scrolling; `$80` also loads the crusher colours and its sprite moves the layer), and `$81` a
  scrolling one: half-speed horizontal parallax at `$C0` in tilesets 1 and 3 (castle windows,
  underground rocks), otherwise autoscrolling with the level (`$24 = $1C`; fish, clouds).
  The stripe images (`Layer3Ptr`, `$059000`) go to the 64x64 tilemap at word `$5000`
  (`BG3SC = $53`) with 2bpp tiles from word `$4000`; the status bar occupies rows 0-4 with
  the scroll at zero, and the IRQ at scanline 36 switches to `$22`/`$24` for the rest of the
  frame. `ProcScreenScrollCmds` (`$05BC00`) moves layers 2 and 3 each frame from the
  camera delta in `$17BC`/`$17BD`; `expand::capture_layer3` runs it with the camera moved 16
  pixels per axis and records the layer's movement as `Layer3::scroll_per_16`, so custom scroll
  code hooked there measures like vanilla. Layer 2 sits on the subscreen (`$0D9E`) and shows
  through the transparent main screen (`$0D9D`) by colour math; `CGADSUB` (`$40`) bit 2
  blends layer 3 with it in the fish and fog levels (mode `$0E` puts only layer 3 on the main
  screen). Level modes `$1E`/`$1F` have no layer 3 on the main screen. The renderer draws
  layer 3 at its entry position, continued unstretched along the axes it scrolls on: a
  non-scrolling axis repeats the entry screen every 256 pixels horizontally and stays in the
  entry 224-pixel band vertically.

## The player and sprites

- The player (`expand::player`, `LevelScene::player`): from the prepared state, with
  every sprite slot cleared, every load flag set (whichever table the loader reads, as in
  the sprite passes), and the generator `$18B9` zeroed,
  `GM14Level` frames run until the entrance action `$71` is zero (a cannon pipe takes a few
  dozen; pipes and doors in vanilla start at zero), with the whole NMI after every frame.
  Its `MarioGFXDMA` (`$00A300`) uploads his tiles (VRAM words `$6000`, `$6100`, `$67F0`)
  and palette (CGRAM `$86`-`$8F`), which
  stay in `vram`/`cgram`. Only OAM slots 64-71 (`$0300`-`$031F`, what `DrawMarioAndYoshi`
  writes) are kept: they are picked out as the frame reaches `ConsolidateOAM` and found
  again in the uploaded image (see [sa1.md](sa1.md)), in level coordinates from the camera
  the pass ended with. The other
  objects in that pass are cluster sprites the entrance screen's level sprites spawned
  (castle candle flames, ghost house Boo ceilings); the sprite passes clear the cluster
  tables and capture the ones their entries respawn, so keeping them here doubled the Boos. Boss
  arenas leave the field empty; their drawing pass already includes him. The CLI draws him
  behind the sprites (his slots follow most of theirs).
- Sprite graphics (`expand::capture_sprites`): for each camera position that puts a sprite
  entry's column at the screen's left edge (horizontal: `$1A = column*16`; vertical: `$1C =
  row*16`), restore the loaded level with the sprite tables (`$14C8`), cluster sprites
  (`$1892`), and load flags cleared, set `$55` to 1 so the loader's column offset is zero,
  and call `CODE_02A802` (the body of `LoadSprFromLevel`) with the data bank at 2, since it
  reads its slot tables through the bank its callers set. The loader returns after a scroll
  sprite and skips entries it has no free slot for (the game calls it every other frame), so
  it is called again with the slots freed until a call loads nothing new. Each spot the
  loader filled (slot statuses 1 and 8-B; sprites sharing a spot stay together, since the
  flying platform `9C` draws the Hammer Bro `9B` placed on it) then gets its own pass: the
  other slots and the generator `$18B9` are cleared, every load flag is set so the level
  loop loads nothing else, the camera is centred on the sprite with `$1411`/`$1412` zero,
  and `GM14Level` (`$00A1DA`) frames run until the slots have left status 1 (init) and at
  least two frames have passed, at most 8, reading OAM as the PPU gets it (below). A
  sprite that has drawn nothing by then gets up to 160 frames while it lives (a Podoboo
  waits under the lava), and the camera follows a sprite that leaves the screen, up to
  three times (a line-guided chainsaw's init moves it 320 pixels left). A pass without any
  sprite at the same camera and frame count gives the objects to subtract; its load flags
  are all set, because the level loop calls the loader too and a baseline that spawns the
  sprite subtracts it (shells, which skip the init frame, vanished that way). Y `$F0` is
  the hidden marker; objects past the bottom wrap negative. Mario is parked 64 pixels left
  of the screen before every frame with `$185C` set (no tile interaction): a Banzai Bill
  erases itself in `InitBanzai` when he is to its right, and parked inside a wall he is
  crushed, which locks every sprite (`$9D = $30`). The scroll commands `$143E`/`$143F` are
  zeroed before and after the loader call: an autoscroll command (`E8` in level `009`)
  copies the layer 2 position over `$1462` every frame and drags the camera off the sprite.
  The spawn position is matched to its entry by low nibbles and screen number only: the
  vanilla loader leaves the entry's extra bits in the other axis's high byte for the init
  routine (a goal tape starts 1280 pixels down). Entries that filled no slot (shooters
  `C9`-`CA`, generators `CB`-`D9`, scroll commands `E7`-`F5`, cluster spawners) share a
  two-frame pass from the loader's camera. The castle candle flames (`E6`: cluster sprite 5
  in slots 0-3, OAM objects 124-127) are positioned in the low eight bits of layer 2
  (`CODE_02FA16` subtracts `$1E`/`$20`), so they go in `SpriteScene::layer2_objects` and the
  renderer repeats them every 256 pixels along layer 2. A pass the CPU core gives up on
  (a crashing custom sprite, or broken per-level code: Invictus level `136` ends its
  UberASM routine with `RTS` under a `JSL`) counts as drawing nothing, and the player pass
  leaves the level without a player for the same reason. Sprites that leave OAM empty become
  ID markers; in vanilla those are `19`, `1F`, `54` (tiles until it turns), `6D`, `82`,
  `89`, `8C`, `8E`, `C7`, and the slotless ones. Every such pass is listed in
  `SpriteScene::diagnostics` with the CPU error. Rendering stacks by Mode 1 priority:
  layer 2 low (5), layer 1 low (6), layer 2 high (8), layer 1 high (9), objects 2/4/7/10.

- Slots. The loader searches down from a maximum the sprite memory setting (`$1692`) picks
  (`SpriteSlotMax`, with reserved ranges for one or two sprite numbers per setting) to the
  first free slot, so a sprite's slot depends on which sprites are alive when its column loads,
  and some sprites look at theirs: the Yoshi's House birds take their colour from it, the
  Blurps of level `011` and others their animation phase, and the line-guided rope its length
  (nine segments from slot 6 up under a setting other than zero, else five; `CODE_01DC54`). The
  loader works on the column `$120` pixels ahead of a camera moving right or down and `$30`
  behind one moving left or up (`DATA_02A7F6`), most sprites erase themselves `$40` beyond the
  screen, and on entry `CODE_02AC5C` loads 32 columns from `$60` before the camera, left to
  right (top to bottom in a vertical level), calling the loader twice for each. The capture
  therefore loads a column twice. First with the slots taken that the sprites of the columns
  before it get: those before it in the entry load, or past that the columns within `$160`
  pixels on the entrance's side, in the order the camera passes them. Only the slot statuses
  are carried over, not the cluster sprites, generator, or scroll command those columns set up,
  and the slots are emptied again before any frame runs. Then, as before, with every slot free,
  which picks up whatever found no slot the first time. With that the birds of level `104` have
  Mesen's colours.
- Every frame of a sprite pass is followed by the ROM's whole NMI handler with the lag flag
  `$10` clear, as the console would run it: some sprites have no tiles until it has run.
  `CODE_01E198` (the Podoboo) and `CODE_02EA25` (a baby Yoshi) rewrite the object's tile to
  `06` and point `$0D8B`/`$0D95` (`DynGfxTilePtr+6`/`+$10`) at that frame in the animated
  tile buffer (`$7E8500` on); Yoshi does the same with two tiles, `06` and `08`, and the
  next pair of pointers. `MarioGFXDMA` then copies them to VRAM words `$6060` and `$6160` along with the
  player's tiles, since the player sets the tile count `$0D84` to `$0A` every frame. Custom
  dynamic sprites upload from code their patch hooks into the handler. The capture therefore
  runs on video memory of its own, reset to the level's before every spot, and keeps the
  characters of a pass's objects that differ from the level's (`SpriteScene::dynamic`,
  `video::DynamicObjects`): two sprites can put different pictures in the same tile, so the
  scene cannot share one VRAM. The player pass also runs the whole NMI, retaining
  its character and palette uploads in the level's video memory, including animated
  background palettes, but not its tilemaps (`player::Tilemaps`): the level's own
  per-frame code can move a layer and have Lunar Magic's row updater (`$1FA6xx`) upload
  rows for the new position (Akogare2 `111` pans layer 2 upward from its second dozen
  frames), and the picture is drawn at the positions the loader's RAM holds.
- The frame counter `$13` counts every vertical blank since power-on, so a player enters a
  level with any value in it. Straight from reset it is zero, which is the one value that
  fires the game's longest periodic event on the level's first frame: Lakitu's cloud
  throws a Spiny when the counter's low seven bits are clear (`$01E98D`), and test level
  `132` gained two Spinies no emulator entry showed. The loader starts it at `$40`: bit 6
  alone puts that event 192 frames off, further than any pass runs, and leaves every
  shorter period, animation phases among them, as it was with zero. What the game decides
  by the counter's parity is decided by an even first frame: the offscreen check looks at
  the right side then, and a sprite a hack has placed beyond `$130` pixels right of the
  camera is erased before a frame has passed (QLDC 2022 `43_gui` `105`, [testing.md](testing.md)).

- OAM is read as the PPU gets it. After every frame the ROM's own upload runs
  (`DoSomeSpriteDMA`, `$008449`: a DMA of `$0200`-`$041F` to `$2104`), and the bus keeps
  what arrives at `$2102`-`$2104`. The upload ends by writing `$80` to `$2103` and `$3F` to
  `$2102`: priority rotation on, so the object `$3F` points into (`$3F / 2`) is drawn in
  front and the rest follow in order, wrapping round. Levels leave `$3F` zero; Roy's,
  Morton's, and Ludwig's rooms set it to 200 (object 100 first).
  A ROM that changes the upload (SA-1 Pack returns before the two writes) gets its own
  order (`SmwBus::first_object`).

## Boss arenas

- Mode 7 boss arenas render a 256x224 scene from captured video registers. One game drawing
  pass supplies the objects (`BossScene::objects`, including arena walls and Bowser's
  floor), and then the interrupts of the frame run, each whole, from its vector to its
  `RTI` (`Machine::interrupt`): the NMI with the lag flag `$10` clear, which uploads the
  player's and the boss's tiles and OAM and leaves the registers of the band at the top of
  the screen, then an IRQ for as long as the last handler armed one (`$4200` bit 5, at the
  line in `$4209`; the handler finds `TIMEUP` set), each leaving the next band's
  registers: the ceiling from the NMI, the floor from the ceiling's IRQ, none in Bowser's
  room. Entering a handler part-way or stopping it at an address does not survive patches:
  SA-1 Pack replaces both handlers' prologue and epilogue (another stack frame, `$4200`
  written at the very end), and a patch in three QLDC 2021 entries reroutes `$00835C`
  past `$008294`, where the pass used to stop. The pass's RAM changes are restored so collision grids remain
  at the loader state. This is an initial arena view, not a cycle-timed gameplay
  screenshot.
- The arena is drawn through the same `render::LevelLayers` and `compose` as any level, as one
  fixed screen: each band's layer 1 (Mode 7's single layer stacks between object priorities
  0 and 1) and the objects. Arena preparation sets `TM = $15`, `CGADSUB = $20` (the backdrop
  only), `CGWSEL = $20` (prevent math inside the colour window), `TMW = $11`, `TSW = 0`,
  `W12SEL = $02`, and `WOBJSEL = $32`: window 1, driven by HDMA from the table at `$04A0`,
  masks layer 1 and the objects, and the colour window is its inverse. Inside window 1 the
  backdrop (CGRAM colour 0, black) therefore takes the back area colour, and outside it
  stays as it is under the arena.
- `video::Window` is the PPU's window model in full: window 1 per row from the HDMA table,
  window 2 from `WH2`/`WH3` (the game's HDMA drives only `WH0`/`WH1`; the arenas leave
  window 2 off), the
  per-layer enable and invert bits from the `$41`-`$43` mirrors, the combining logic
  (`WBGLOG`/`WOBJLOG`), and the main and subscreen masks (`TMW`/`TSW`). The last three
  pairs have no RAM mirror (each game mode writes the registers directly), so the bus
  captures `$2128`-`$212B` and `$212E`-`$212F`. `compose` applies a window only to a fixed
  screen, because window positions are screen positions.
