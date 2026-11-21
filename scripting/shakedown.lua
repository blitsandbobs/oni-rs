-- Shakedown (scripting/shakedown.sh): begin a level's mission — from a save
-- point if SHAKE_SAVE is set — and let it run SHAKE_TICKS ticks with the
-- player idle; print what happened (where the player went, health, the
-- scripts' errors and the commands they called that the port skips).
local TICKS = tonumber(os.getenv("SHAKE_TICKS") or "7200")
local function shallow(t)
  if type(t) ~= "table" then return tostring(t) end
  local ks = {}
  for k, v in pairs(t) do if type(v) ~= "table" and type(v) ~= "function" then ks[#ks + 1] = k .. "=" .. tostring(v) end end
  table.sort(ks)
  return table.concat(ks, " ")
end
local SAVE = tonumber(os.getenv("SHAKE_SAVE") or "")
local ok, err = pcall(mission_begin, true, SAVE)
if not ok then print("BEGIN-ERROR " .. tostring(err)) return end
local x0, y0, z0 = pos()
local hp0 = hp(0)
local first_outcome, outcome_at
local step_err
local TOUR = os.getenv("SHAKE_TOUR")
if TOUR == "" then TOUR = nil end
-- Characters lost out of the world (NaN, or far below everything).
local function lost_characters(where)
  for i = 0, count() - 1 do
    if present(i) then
      local x, y, z = pos_of(i)
      if x ~= x or y ~= y or z ~= z or y < -5000 then
        print(string.format("LOST %s: fighter %d %s hp %s dead %s at (%s, %s, %s)", where, i, tostring(class_of(i)), tostring(hp(i)), tostring(dead(i)), x, y, z))
      end
    end
  end
end
if TOUR then
  -- Tour (shakedown.sh --tour): the player, kept alive, is put in every
  -- trigger volume and at every console in turn (using it), twice over, so
  -- most of the level's scripts run; errors are reported with the stop.
  local stops = dofile(TOUR)
  local seen_errors = 0
  local t = 0
  for pass = 1, 2 do
    for _, s in ipairs(stops) do
      local label, x, y, z, face = s[1], s[2], s[3], s[4], s[5]
      local clock0 = os.time()
      pcall(skip_cutscene)
      teleport(0, x, y, z)
      if face then facing(math.deg(face)) end
      for k = 1, 9 do
        set_hp(0, 100000)
        if face and k == 2 then action() end
        local ok2, e = pcall(step, 10)
        t = t + 10
        if not ok2 then step_err = string.format("%s (pass %d): %s", label, pass, tostring(e)) break end
      end
      if step_err then break end
      local m = mission_info()
      local errs = m.errors or {}
      for i = seen_errors + 1, #errs do print(string.format("TOUR-ERROR at %s (pass %d): %s", label, pass, tostring(errs[i]))) end
      seen_errors = #errs
      if m.outcome ~= nil and not first_outcome then first_outcome, outcome_at = tostring(m.outcome), t end
      lost_characters(label)
      -- A slow stop (3 s or more of wall time for its 90 ticks): what the world holds.
      local took = os.time() - clock0
      if took >= 3 then
        local w = world_stats()
        print(string.format("SLOW %s (pass %d): %d s; particles %d (%s), weapons %d, fighters %d, corpses %d",
          label, pass, took, w.particles, w.top, w.weapons, w.fighters, w.corpses))
      end
    end
    if step_err then break end
  end
  print(string.format("TOUR %d stops x 2, %d ticks", #stops, t))
else
  for t = 60, TICKS, 60 do
    local ok2, e = pcall(step, 60)
    if not ok2 then step_err = string.format("tick %d: %s", t, tostring(e)) break end
    local m = mission_info()
    if m.outcome and not first_outcome then first_outcome, outcome_at = tostring(m.outcome), t end
  end
  lost_characters("the end")
end
local m = mission_info()
local x, y, z = pos()
print(string.format("START %.0f %.0f %.0f  END %.0f %.0f %.0f  (moved %.0f, fell %.0f)", x0, y0, z0, x, y, z, math.sqrt((x-x0)^2+(z-z0)^2), y0 - y))
print(string.format("PLAYER hp %s -> %s dead=%s", tostring(hp0), tostring(hp(0)), tostring(dead(0))))
print(string.format("MISSION tasks=%s spawned=%s objective=%s outcome=%s%s messages=%d characters=%s",
  tostring(m.tasks), tostring(m.spawned), tostring(m.objective), tostring(m.outcome),
  first_outcome and (" (first " .. first_outcome .. " at tick " .. outcome_at .. ")") or "", m.messages and #m.messages or 0, tostring(count and count() or "?")))
if step_err then print("STEP-ERROR " .. step_err) end
for _, w in ipairs(m.waiting or {}) do print("WAITING " .. w) end
local seen = {}
for _, e in ipairs(m.errors or {}) do
  local k = tostring(e):gsub("%d+", "#")
  if not seen[k] then seen[k] = true print("SCRIPT-ERROR " .. tostring(e)) end
end
print("STUBBED " .. table.concat(m.stubbed or {}, " "))
local okh, h = pcall(hud) if okh then print("HUD " .. shallow(h)) end
local okg, g = pcall(game) if okg then print("GAME " .. shallow(g)) end
print("SHAKE-DONE")
