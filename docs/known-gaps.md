# Known gaps

What a rendered level does not reproduce, and what the tooling does not handle.

- Layers 2 and 3 are drawn as the entry screen shows them and continued unstretched
  (see the layer facts in [smw.md](smw.md)); parallax is not reproduced away from the entry
  screen, the status bar is left out, and an axis layer 3 does not scroll along cannot be
  followed once the camera moves.
- Windows are applied only on a boss arena's fixed screen: they are screen positions, which
  a picture of a whole scrolling level has no place for. The spotlight rooms (mode `$11`)
  render uniformly dark, which is what the game shows until the light switch is hit (the
  spotlight sprite writes an empty window to `$04A0` while its `$C2` is zero), and the
  keyhole and message box effects do not appear.
- Sprites show their first drawn frame, each alone on a camera centred on it, with Mario
  off the left screen edge and no scrolling, so anything that waits for Mario or spawns
  over time (Bullet Bill shooters, generators, Lakitu's Spinies, a Magikoopa, Monty Moles
  in some hacks) is a marker or not what a player sees, and sprites that interact with one
  another only do so when they share a spot. A sprite that stays hidden at first is drawn
  where it first appears (a Podoboo at the lava's surface). Cluster sprites are captured
  from their spawner's camera only, except the candle flames. Custom sprite loaders run as
  ROM code; an undrawn-sprite census over every level of the corpus runs without errors
  apart from levels whose loader already fails. The emulator oracle compares which sprite
  is in which slot on a sample of every hack's levels ([testing.md](testing.md)), and whole
  frames on a few; nothing compares each custom sprite's own picture. Boss arenas show the
  OAM of the first drawing pass instead.
- A sprite is captured alone, and some sprites look at their slot: the Yoshi's House birds
  take their colour from it, several animations their phase, and the line-guided rope its
  length. It gets the slot the ROM's loader gives it with the columns before it loaded in
  order from the entrance and every sprite of them still in place, which is what a player
  sees who walks there and leaves everything alone. One who kills or outruns sprites, or
  arrives through another entrance, may see another colour, phase, or length.
- SA-1 ROMs ([sa1.md](sa1.md)): the SA-1's timers, the second type of character conversion,
  the variable-length bit reader, and write protection are not modelled; no hack in the
  corpus uses the first three, and the last only matters to a game that relies on a write
  being refused. Of the 39 SA-1 hacks in the corpus, 38 render all 512 levels; QLDC 2021 `76_Bench-kun` fails in
  the 18 Mode 7 boss rooms with a `COP` at `$00E296`, which is the hack's own doing (the
  patch at `$10E288` calls `$00987D` with `JSL` and the routine returns with `RTS`, into
  data). The emulator oracle compares tile grids, layer 3 tilemaps, and sprite slots on a
  sample of each hack's levels ([testing.md](testing.md)), not their pictures.
- Code a hack runs every frame of the level loop (a custom status bar, a power-up handed
  to the player, UberASM `main` code) has not run: a level is loaded and prepared, and its
  sprites and player are drawn, but no frame of game mode `$14` is played.
- HDMA is not run, so whatever a hack changes by scanline is missing: a gradient sky
  (Luminescent level `148` writes the fixed colour per line) comes out as the one colour the
  level's back area has.
- The GFX tooling refuses ROMs their authors locked (`GfxError::Locked`, see
  [lunar-magic.md](lunar-magic.md)): their pointer tables are not addresses. Levels are
  unaffected, since the ROM's own decompression runs for them. There is no LC_LZ2 or
  LC_LZ3 encoder yet, which the build will need.
