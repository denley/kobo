# Lunar Magic ROM facts

What Lunar Magic changes in a ROM, as far as the library has to know: found from the formats
the community documents and from inspecting ROMs Lunar Magic produced, never from its
executable. Vanilla behaviour is in [smw.md](smw.md).

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
  pointer tables do not hold addresses. The files themselves are ordinary LC_LZ2. Six ROMs
  of the corpus are locked (Invictus, both Super Dram Worlds, Smb2dx, two of the `SMW_2021`
  set). `gfx::is_locked` recognises the routine and the GFX tooling refuses with
  `GfxError::Locked`; levels load regardless, since the ROM's code runs. No ROM in the
  corpus stores graphics as LC_LZ3, and nothing reads it.
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
