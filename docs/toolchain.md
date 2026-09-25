# The toolchain

What the tools a build runs require of the ROM they are given, how they find space, and what
makes their output vary. From their sources (paths relative to each tool's repository), read
2026-09-25; the versions are the ones [step-2.md](step-2.md) pins. `$xxxxxx` is a SNES
address, "PC" an offset in the headerless file.

## Asar 1.91, and every tool built on it

- Free space is runs of `$00`, skipping valid RATS blocks (`assembleblock.cpp:746`). LoROM
  code goes in banks `$10`-`$3F`; data tries banks `$40` and up first on images over 2 MiB,
  and never crosses a bank. A patch that needs more space expands the image, 512 KiB to
  1 MiB to 2 MiB (4 MiB for data), and rewrites `$00FFD7` and the checksum
  (`libsmw.cpp:244-352`).
- `autoclean` erases the whole RATS block it finds before the old target of a pointer or
  jump it replaces, and refills it with `$00` (`libsmw.cpp:135-150`,
  `assembleblock.cpp:2081-2158`). Anything a tool repoints, or a hook site a tool takes
  over, has to lead to a block of its own: the tables at `$06F624` and `$06F63A` (GPS) and
  `$0EF30C` (PIXI), and the targets of the jumps at every tool's hook sites.

## PIXI 1.43 (GPL-3.0)

- Refuses a ROM where the pointer at `$06F624` (the acts-like table) is `$FFFFFF`, "without
  having modified a level in Lunar Magic", and one where `$00F6E4` is not `$5C`, Lunar
  Magic's VRAM patch (`src/sprite.cpp:1875-1906`). It checks only that byte; what the
  patch has to do is still to be worked out.
- Writes the sprite size table pointer at `$0EF30C` and `$42` at `$0EF30F`
  (`asm/main.asm:209-217`), clears bit 0 of `$0FFFE0` (`:49-55`), and hooks `$05D8B9` to
  store the level number in `$010B` (`:61-67`). On a LoROM image it always installs its own
  255-sprites-per-level code, with jumps at `$02A856` (Lunar Magic 3's site for the same),
  `$02A936`, `$02A8BB`, `$02FAE9`, and `$02ABF2` (`:301-316`).
- Its tables: `STSD` and flags at `$02FFE2`-`$02FFFF`, shared routine pointers at
  `$03E05C`. A newer PIXI's marker at `$02FFE6` is refused.
- MeiMei, its sprite data remapper, uses addresses that assume a copier header
  (`src/MeiMei/MeiMei.cpp:96`) and reads the wrong bytes of a headerless image. Kobo runs
  PIXI with `-meimei-off`; its own level writer sizes sprite entries.
- The version digits at `$0FF0B4` (inside Lunar Magic's marker) turn on `!EXLEVEL` when
  above 2.53; vanilla `$FF` counts as above, `$00` would not (`asm/sa1def.asm:40-43`).
- Shared routines are listed in directory order (`src/sprite.cpp:1203`), which decides
  their slots, so output depends on the file system. `pixi_settings.json` and `plugins/`
  in the working directory change a run.
- Builds natively with CMake; configure downloads Asar and nlohmann/json by tag. The
  repository holds Nintendo data for its Windows CFG editor (SMW graphics, palettes, and a
  Map16 page under `src/CFG Editor/CFG Editor/Resources/`).

## GPS 1.4.4 (no licence)

- No public repository. The 1.4.4 release (the last) is on the Wayback Machine as
  `dl.smwcentral.net/31515/GPS (V1.4.4).zip`; its `src.zip` has the source.
- Refuses a ROM whose pointer at `$06F624` is `$FFFFFF`, custom blocks on Map16 pages
  `40`+ unless `$06F63C` is not `$FF`, and a ROM with `$8B` at `$06F690` but no
  `GPS_VeRsIoN` string, as "unidentified custom block code" (`gps_src/main.cpp:544-548`,
  `822-825`, `851-853`).
- Copies the acts-like table (`$8000` bytes, two per tile, from the pointer at `$06F624`;
  pages `40`-`7F` from `$06F63A`), applies its list, and writes it back to a new block.
- It depends on the shape of Lunar Magic's code in bank `$06`, not only on tables: it
  writes 16-byte entries at `$06F690`, `$06F6A0`, `$06F6B0`, `$06F6C0`, `$06F6D0`,
  `$06F6E0`, `$06F720`, `$06F730`, `$06F780`, `$06F7C0`, `$06F7D0`, and `$06F7E0`, each
  ending `JMP $F602`, and puts `JML`s over `$06F67B` and `$06F717`, whose replacements
  compare A with `#$39`, `#$EA`, and `#$82` and fall back to `$06F602` (`main.asm:7-77`).
  It does not use the documented `JSL` slots at `$06F890`-`$06F9F0`.
- Its shared routine table is at `$0CB66F`. Routine slots follow `readdir` order
  (`main.cpp:605`).
- Builds natively with `g++ -std=c++17 main.cpp asar/asardll.c -ldl`; it loads
  `./libasar.so`, and Asar 1.91 works (the same API as the 1.81 it ships).

## UberASM Tool 2.1 (GPL-3.0)

- Requires an image of 1 to 8 MiB whose internal title at PC `0x7FC0` is
  `SUPER MARIOWORLD     ` (`UberASMTool/ROM.cs:89-110`); no Lunar Magic check.
- Reads `$0FFFE0` bit 0 (clear: 255 sprites per level), so it runs after PIXI, and the
  version digits at `$0FF0B4`. Hooks `$05808C`, `$00A5EE`, `$00A242`, `$00A2EE`,
  `$00A1C3`, `$00A18F`, `$009322`, `$00804E`, `$008176`, and `$008E1A`, and rewrites
  `$05D8B7`-`$05D8DF` around PIXI's hook (`assets/asm/base/main.asm:15-101`).
- Library files are inserted in `Directory.GetFiles` order (`Library.cs:17`).
- Targets `net8.0` with `PlatformTarget` x86 and a 32-bit Windows Asar, so the published
  build does not run on Linux or macOS, where .NET has no x86 runtime. A rebuild as x64
  with a native `libasar` probably does; not tried.

## AddmusicK 1.0.11, AddMusicKFF (no licence)

- Requires an image over 512 KiB, and `$0E8000` either vanilla (`3E 0E`) or its own
  `@AMK` (`src/AddmusicK/AddmusicK.cpp:196-197`, `338-373`).
- Fills `$0E8000`-`$0EF0FF` and `$0F8000`-`$0FF050` with `$55` ("Lunar Magic install
  some hacks there") and puts its own code there (`asm/SNES/patch.asm:112-115`). The second
  range overlaps `$0FF035`-`$0FF050`, which Lunar Magic 3.70's first save fills. Kobo puts
  nothing in either range.
- Its own free-space scanner counts only `$00` as free, honours RATS, never crosses a bank,
  starts at PC `0x200000` and then `0x080000`, and handles only PC below `0x400000`
  (`globals.cpp:297-396`). Everything written before it must be RATS-tagged.
- Output is repeatable, including over its own output. It runs from its own folder, renames
  the ROM to `ROM~`, and reads `Addmusic_options.txt` in place of its arguments.
- Builds natively with `make`; loads `./libasar.so` or runs `asar`. The repository holds
  SMW samples, transcriptions of SMW's music and sound effects, and vanilla code bytes.

## SA-1 Pack 1.40 (no licence)

- Applies to a clean ROM only, before Lunar Magic or any tool; it detects its first run by
  `$0DA693` not being `$1E`, and on it moves every level's sprite header into bank `$07`.
  It sets `$00FFD5` to `$23`, which every other tool reads to detect SA-1, and marks
  itself at `$0084C0` (`$05A123`) and `$0084C3` (version, 140). `6mb.asm` and `8mb.asm`
  follow `sa1.asm` for larger images.
- Chooses its decompressor by `$0FFFEB` (`2`: LC_LZ3) and hooks `$00B8E3`, Lunar Magic's
  decompression site. It has to be reapplied when a ROM's compression changes.
- Contains code attributed to Lunar Magic (`asm/boost/lz3.asm`). Do not read it; the
  clean-room rule covers it.

## Across the tools

- Run twice on the same input, every tool tried gave the same bytes. Run again over its own
  output, AddmusicK repeats itself but PIXI, GPS, and SA-1 Pack move things (UberASM Tool
  was not run). A build therefore always runs a tool on the previous stage's snapshot,
  never on its own output.
- PIXI, GPS, and UberASM Tool order shared routines or library files by directory listing,
  which differs between operating systems and file systems. Byte-identical builds need
  that order fixed, by patching the tools to sort or by handing them one file at a time.
