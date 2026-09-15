-- Kobo level oracle: boots SMW in Mesen 2, forces the title screen to load
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
--
-- How it works: the script presses Start on the title screen and A twice
-- on the file/player select, which makes the game load the intro level
-- through $0109 (the "overworld override"). An exec callback at the level
-- loader entry replaces that value with the requested level. Everything
-- is dumped on the first frame of game mode $14 (level running), before
-- the player has moved.

local mem = emu.memType.snesMemory
local outdir = os.getenv("KOBO_ORACLE_OUT") or "."
local list = os.getenv("KOBO_ORACLE_LEVELS") or "105"
local levels = {}
for s in list:gmatch("[^,%s]+") do
  levels[#levels + 1] = tonumber(s, 16)
end

local LOADER_ENTRY = 0x0096D5 -- GM11LoadLevel, right after $0109/$1F11 are written
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

local function override_for(level)
  local lo, hi = level & 0xFF, level >> 8
  local v = lo < 0x25 and lo or lo + 0x24
  if v > 0xFF then
    return nil
  end
  return v, hi
end

local function on_loader_entry()
  if current == nil then
    return
  end
  local v, hi = override_for(current)
  if v == nil then
    fail(string.format("level %03X cannot be expressed through $0109", current))
    return
  end
  emu.write(0x7E0109, v, mem)
  emu.write(0x7E1F11, hi, mem)
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
      stage, stage_frames = "level", 0
    end
  elseif stage == "level" then
    if mode == 0x14 then
      dump_ram(current)
      dump_video(current)
      logf("level %03X: dumped at frame %d", current, stage_frames)
      next_level()
    end
  end
end

local function on_input()
  emu.setInput(buttons, 0)
end

emu.addMemoryCallback(on_loader_entry, emu.callbackType.exec, LOADER_ENTRY, LOADER_ENTRY, mem)
emu.addMemoryCallback(on_loader_entry, emu.callbackType.exec, LOADER_ENTRY | 0x800000, LOADER_ENTRY | 0x800000, mem)
emu.addEventCallback(on_frame, emu.eventType.endFrame)
emu.addEventCallback(on_input, emu.eventType.inputPolled)
logf("oracle started: %d levels, out=%s", #levels, outdir)
