-- Kobo level oracle: boots SMW in Mesen 2, uses the file select to load
-- each requested level, and dumps what the game computed.
--
-- Usage (see dump.sh):
--   KOBO_ORACLE_LEVELS=105,106 KOBO_ORACLE_OUT=/some/dir \
--     Mesen --testRunner dump_levels.lua rom.sfc --timeout=600
--
-- Per level it writes:
--   level_XXX.l1lo.bin  $7EC800..$7EFFFF  Map16 tile numbers, low bytes
--   level_XXX.l1hi.bin  $7FC800..$7FFFFF  Map16 tile numbers, high bytes
--   level_XXX.cgram.bin 512 bytes of CGRAM on the first level frame
--   level_XXX.vram.bin  64 KiB of VRAM on the first level frame
--   level_XXX.txt       header-derived RAM values, one "key value" per line
-- With KOBO_ORACLE_VIDEO=1, capture after the fade-in instead, also writing
-- a PPM image, full WRAM, and PPU register state for visual diagnostics.
-- Use a separate directory: moving actors and grids can differ by then.
--
-- How it works: the script presses Start on the title screen and A twice
-- on the file/player select, which makes the game load the intro level
-- through $0109 (the "overworld override"). An exec callback at the level
-- loader entry keeps that override set, and a second one at the header
-- pointer lookup (CODE_05D8B7) replaces the resolved level number with the
-- requested one, since $0109 cannot encode levels 000, 100, or low bytes
-- $DC and above. Everything is dumped on the first frame of game mode $14
-- (level running), before the player has moved.
--
-- Castle and ghost house tilesets first play the "No Yoshi" entrance intro
-- (a separate one-screen room) and then reload the level. The script
-- predicts that from the ROM's header and entrance tables, the same way
-- CODE_05DA24 decides, and dumps the second level frame in that case.

local mem = emu.memType.snesMemory
local outdir = os.getenv("KOBO_ORACLE_OUT") or "."
local list = os.getenv("KOBO_ORACLE_LEVELS") or "105"
local levels = {}
for s in list:gmatch("[^,%s]+") do
  levels[#levels + 1] = tonumber(s, 16)
end

local LOADER_ENTRY = 0x0096D5 -- GM11LoadLevel, right after $0109/$1F11 are written
local POINTER_LOOKUP = 0x05D8B7 -- CODE_05D8B7: level number in $0E-$0F becomes pointers
local rom = emu.memType.snesPrgRom
local MAX_FRAMES_PER_STATE = 1800

local idx = 1
local current = levels[idx]
local stage = "title" -- title -> file -> player -> level -> next
local stage_frames = 0
local buttons = {}
local log = assert(io.open(outdir .. "/oracle.log", "a"))

local function logf(fmt, ...)
  log:write(string.format(fmt, ...), "\n")
  log:flush()
end

local function fail(msg)
  logf("FAIL: %s", msg)
  log:close()
  emu.stop(2)
end

-- LoROM file offset of a bank $00-$3F address, for reading ROM tables.
local function pc(addr)
  return ((addr >> 16) & 0x7F) * 0x8000 + (addr & 0x7FFF)
end

-- Whether the game shows the "No Yoshi" entrance intro before this level
-- when entering from the overworld: castle, rope, ghost house, and
-- similar tilesets, unless the level's entrance settings disable it.
local function has_intro(level)
  local ptr = pc(0x05E000 + 3 * level)
  local data = emu.read(ptr, rom) | (emu.read(ptr + 1, rom) << 8) | (emu.read(ptr + 2, rom) << 16)
  local tileset = emu.read(pc(data) + 4, rom) & 0x0F
  local intro_tilesets = { [1] = true, [2] = true, [5] = true, [6] = true, [8] = true }
  local disabled = emu.read(pc(0x05F600 + level), rom) & 0x80 ~= 0
  return intro_tilesets[tileset] == true and not disabled
end

local function on_loader_entry()
  -- The title screen also calls this entry in game mode $03. Replacing
  -- it there changes the graphics cache before the actual level load.
  if current == nil or emu.read(0x7E0100, mem) ~= 0x11 then
    return
  end
  -- Any non-zero value takes the forced-level path; the number itself is
  -- replaced at the pointer lookup.
  emu.write(0x7E0109, 1, mem)
  emu.write(0x7E1F11, 0, mem)
end

local function on_pointer_lookup()
  if current == nil or emu.read(0x7E0100, mem) ~= 0x11 then
    return
  end
  emu.write(0x7E000E, current & 0xFF, mem)
  emu.write(0x7E000F, current >> 8, mem)
  emu.write(0x7E17BB, current & 0xFF, mem)
end

local function read_range(base, len, memtype)
  local parts = {}
  for i = 0, len - 1 do
    parts[#parts + 1] = string.char(emu.read(base + i, memtype))
  end
  return table.concat(parts)
end

local function write_file(name, data)
  local f = assert(io.open(outdir .. "/" .. name, "wb"))
  f:write(data)
  f:close()
end

local function dump_ram(level)
  local tag = string.format("level_%03X", level)
  write_file(tag .. ".l1lo.bin", read_range(0x7EC800, 0x3800, mem))
  write_file(tag .. ".l1hi.bin", read_range(0x7FC800, 0x3800, mem))
  local info = {
    { "level", string.format("%03X", level) },
    { "loading_level_number", string.format("%02X", emu.read(0x7E17BB, mem)) },
    { "level_mode", string.format("%02X", emu.read(0x7E1925, mem)) },
    { "screen_mode", string.format("%02X", emu.read(0x7E005B, mem)) },
    { "screens", string.format("%02X", emu.read(0x7E005D, mem)) },
    { "last_screen_horiz", string.format("%02X", emu.read(0x7E005E, mem)) },
    { "last_screen_vert", string.format("%02X", emu.read(0x7E005F, mem)) },
    { "fg_palette", string.format("%02X", emu.read(0x7E192D, mem)) },
    { "sprite_palette", string.format("%02X", emu.read(0x7E192E, mem)) },
    { "back_area", string.format("%02X", emu.read(0x7E192F, mem)) },
    { "bg_palette", string.format("%02X", emu.read(0x7E1930, mem)) },
    { "object_tileset", string.format("%02X", emu.read(0x7E1931, mem)) },
    { "sprite_tileset", string.format("%02X", emu.read(0x7E192B, mem)) },
  }
  local lines = {}
  for _, kv in ipairs(info) do
    lines[#lines + 1] = kv[1] .. " " .. kv[2]
  end
  write_file(tag .. ".txt", table.concat(lines, "\n") .. "\n")
end

local function dump_video(level)
  local tag = string.format("level_%03X", level)
  write_file(tag .. ".cgram.bin", read_range(0, 512, emu.memType.snesCgRam))
  write_file(tag .. ".vram.bin", read_range(0, 0x10000, emu.memType.snesVideoRam))
  if os.getenv("KOBO_ORACLE_VIDEO") then
    -- Render the current PPU frame synchronously. takeScreenshot() can
    -- lag behind in the test runner's asynchronous video decoder.
    local pixels = {}
    for _, rgb in ipairs(emu.getScreenBuffer()) do
      pixels[#pixels + 1] = string.char((rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255)
    end
    local size = emu.getScreenSize()
    assert(#pixels == size.width * size.height, "unexpected screen buffer size")
    write_file(tag .. ".ppm", string.format("P6\n%d %d\n255\n", size.width, size.height) .. table.concat(pixels))
    write_file(tag .. ".wram.bin", read_range(0x7E0000, 0x20000, mem))
    write_file(tag .. ".oam.bin", read_range(0, 544, emu.memType.snesSpriteRam))
    local state = emu.getState()
    local lines = {}
    for key, value in pairs(state) do
      if key:find("ppu") then
        lines[#lines + 1] = key .. " " .. tostring(value)
      end
    end
    table.sort(lines)
    write_file(tag .. ".ppu.txt", table.concat(lines, "\n") .. "\n")
  end
end

-- Press a button for one frame every 8 frames while in a stage, so the
-- game sees clean presses rather than a held button.
local function tap(name)
  if stage_frames % 8 == 0 then
    buttons[name] = true
  end
end

local function next_level()
  idx = idx + 1
  current = levels[idx]
  if current == nil then
    logf("done: %d levels", #levels)
    log:close()
    emu.stop(0)
    return
  end
  stage, stage_frames = "title", 0
  emu.reset()
end

local function on_frame()
  stage_frames = stage_frames + 1
  if stage_frames > MAX_FRAMES_PER_STATE then
    fail(string.format("stuck in stage %s for level %03X", stage, current or -1))
    return
  end
  local mode = emu.read(0x7E0100, mem)
  buttons = {}
  if stage == "title" then
    if mode == 0x07 then
      tap("start")
    elseif mode == 0x08 then
      stage, stage_frames = "file", 0
    end
  elseif stage == "file" then
    if mode == 0x08 then
      tap("a")
    elseif mode == 0x0A then
      stage, stage_frames = "player", 0
    end
  elseif stage == "player" then
    if mode == 0x0A then
      tap("a")
    elseif mode > 0x0A then
      stage, stage_frames = has_intro(current) and "intro" or "level", 0
    end
  elseif stage == "intro" then
    -- The intro room runs in mode $14 and reloads through mode $0F.
    if mode == 0x14 then
      stage, stage_frames = "intro_running", 0
    end
  elseif stage == "intro_running" then
    if mode ~= 0x14 then
      logf("level %03X: intro ended at frame %d", current, stage_frames)
      stage, stage_frames = "level", 0
    end
  elseif stage == "level" then
    if mode == 0x14 then
      if os.getenv("KOBO_ORACLE_VIDEO") then
        stage, stage_frames = "video", 0
      else
        dump_ram(current)
        dump_video(current)
        logf("level %03X: dumped at frame %d", current, stage_frames)
        next_level()
      end
    end
  elseif stage == "video" then
    local state = emu.getState()
    if stage_frames >= 3 and not state["ppu.forcedBlank"] and state["ppu.screenBrightness"] == 15 then
      dump_ram(current)
      dump_video(current)
      logf("level %03X: visible video dumped at frame %d", current, stage_frames)
      next_level()
    end
  end
end

local function on_input()
  emu.setInput(buttons, 0)
end

emu.addMemoryCallback(on_loader_entry, emu.callbackType.exec, LOADER_ENTRY, LOADER_ENTRY, mem)
emu.addMemoryCallback(on_loader_entry, emu.callbackType.exec, LOADER_ENTRY | 0x800000, LOADER_ENTRY | 0x800000, mem)
emu.addMemoryCallback(on_pointer_lookup, emu.callbackType.exec, POINTER_LOOKUP, POINTER_LOOKUP, mem)
emu.addMemoryCallback(on_pointer_lookup, emu.callbackType.exec, POINTER_LOOKUP | 0x800000, POINTER_LOOKUP | 0x800000, mem)
emu.addEventCallback(on_frame, emu.eventType.endFrame)
emu.addEventCallback(on_input, emu.eventType.inputPolled)
logf("oracle started: %d levels, out=%s", #levels, outdir)
