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
  apart from levels whose loader already fails, but nothing compares custom sprites to an
  emulator. Boss arenas show the OAM of the first drawing pass instead.
- Sprites whose tiles are uploaded per frame into the player's dynamic tile area (the
  Podoboo; VRAM bytes `$C0C0`-`$C305` were seen to matter) are drawn from whatever the
  player pass left there: the sprite passes never run the NMI upload (`MarioGFXDMA`), and the
  scene is drawn from the level's VRAM, not the pass's.
- SA-1 ROMs ([sa1.md](sa1.md)): nothing has been compared to an emulator yet, since the
  oracle script reads `$7E` addresses. Against the vanilla ROM, vanilla with SA-1 Pack
  renders 476 of 512 levels identically and 23 with the same objects overlapping in another
  order (MaxTile's priorities). Ten have other objects (`00F`, `0DD`, `0FC`, `104`, `11D`,
  `12A`, `12C`, `1DD`, `1E8`, `1E9`: an extra object, another animation frame, another
  palette in `104`), which SA-1 Pack's changed sprite memory settings and random number
  use could explain but nothing has shown; a fault in the SA-1 model would look the same.
  Boss arenas (`098`, `0D9`, `198`) differ in their flames. OAM is still read starting from
  `$3F`, which SA-1 Pack no longer applies, so overlapping objects in an arena may be in
  the wrong order. Images over 4 MiB fail on every level, in code running from banks
  `$C0`-`$FF` (QLDC 2021 `24_HD_DankBaron`, `70_DPBOX`, `79_Hwailaluta`): the Super MMC is
  not modelled. Three QLDC 2021 entries fail in the Mode 7 boss rooms and nowhere else
  (`62_Rykon-V73` and `84_TickTockClock` in the same 21 levels with a jump to `$000000`,
  `76_Bench-kun` in 18 with a `COP`); the same rooms load in the other SA-1 hacks, and
  whether these use something unmodelled or are broken in the hacks is not known. Code
  using SA-1 DMA or character conversion is not handled.
- `GFX27`'s layout is unknown; `GFX32`/`GFX33` are not handled by the GFX tooling.
