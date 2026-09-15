-- Debug helper: like dump_levels.lua's navigation, but instead of dumping
-- it logs the first writes to a RAM address after the level loader runs.
--   KOBO_ORACLE_LEVELS=102 KOBO_TRACE_ADDR=7EE400 KOBO_ORACLE_OUT=dir
local mem = emu.memType.snesMemory
local outdir = os.getenv("KOBO_ORACLE_OUT") or "."
local level = tonumber(os.getenv("KOBO_ORACLE_LEVELS") or "105", 16)
local addr = tonumber(os.getenv("KOBO_TRACE_ADDR") or "7EE400", 16)
local log = assert(io.open(outdir .. "/trace.log", "w"))
local function logf(fmt, ...) log:write(string.format(fmt, ...), "\n"); log:flush() end
local LOADER_ENTRY = 0x0096D5
local stage, stage_frames, buttons, armed, count = "title", 0, {}, false, 0

local function override_for(l)
  local lo, hi = l & 0xFF, l >> 8
  local v = lo < 0x25 and lo or lo + 0x24
  return v, hi
end
local function on_loader_entry()
  local v, hi = override_for(level)
  emu.write(0x7E0109, v, mem); emu.write(0x7E1F11, hi, mem)
  armed = true
  logf("loader entry hit; tracing writes to %06X", addr)
end
local function on_write(a, value)
  if not armed then return end
  count = count + 1
  if count <= 40 then
    local st = emu.getState()
    local cpu = st.cpu or {}
    local keys = {}
    if count == 1 then for k, _ in pairs(cpu) do keys[#keys + 1] = k end; table.sort(keys); logf("cpu keys: %s", table.concat(keys, " ")) end
    logf("write #%d: addr %06X value %02X pc %02X:%04X mode %02X", count, a, value, cpu.k or -1, cpu.pc or -1, emu.read(0x7E0100, mem))
  end
end
local function tap(name) if stage_frames % 8 == 0 then buttons[name] = true end end
local function on_frame()
  stage_frames = stage_frames + 1
  if stage_frames > 1800 then logf("stuck in %s", stage); log:close(); emu.stop(2); return end
  local mode = emu.read(0x7E0100, mem)
  buttons = {}
  if stage == "title" then
    if mode == 0x07 then tap("start") elseif mode == 0x08 then stage, stage_frames = "file", 0 end
  elseif stage == "file" then
    if mode == 0x08 then tap("a") elseif mode == 0x0A then stage, stage_frames = "player", 0 end
  elseif stage == "player" then
    if mode == 0x0A then tap("a") elseif mode > 0x0A then stage, stage_frames = "level", 0 end
  elseif stage == "level" then
    if mode == 0x14 then
      logf("level running; %d writes seen; $5B=%02X", count, emu.read(0x7E005B, mem))
      log:close(); emu.stop(0)
    end
  end
end
emu.addMemoryCallback(on_loader_entry, emu.callbackType.exec, LOADER_ENTRY, LOADER_ENTRY, mem)
emu.addMemoryCallback(on_write, emu.callbackType.write, addr, addr, mem)
emu.addEventCallback(on_frame, emu.eventType.endFrame)
emu.addEventCallback(function() emu.setInput(buttons, 0) end, emu.eventType.inputPolled)
