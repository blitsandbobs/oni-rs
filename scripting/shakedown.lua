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
for t = 60, TICKS, 60 do
  local ok2, e = pcall(step, 60)
  if not ok2 then step_err = string.format("tick %d: %s", t, tostring(e)) break end
  local m = mission_info()
  if m.outcome and not first_outcome then first_outcome, outcome_at = tostring(m.outcome), t end
end
local m = mission_info()
local x, y, z = pos()
print(string.format("START %.0f %.0f %.0f  END %.0f %.0f %.0f  (moved %.0f, fell %.0f)", x0, y0, z0, x, y, z, math.sqrt((x-x0)^2+(z-z0)^2), y0 - y))
print(string.format("PLAYER hp %s -> %s dead=%s", tostring(hp0), tostring(hp(0)), tostring(dead(0))))
print(string.format("MISSION tasks=%s spawned=%s objective=%s outcome=%s%s messages=%d characters=%s",
  tostring(m.tasks), tostring(m.spawned), tostring(m.objective), tostring(m.outcome),
  first_outcome and (" (first " .. first_outcome .. " at tick " .. outcome_at .. ")") or "", m.messages and #m.messages or 0, tostring(count and count() or "?")))
if step_err then print("STEP-ERROR " .. step_err) end
local seen = {}
for _, e in ipairs(m.errors or {}) do
  local k = tostring(e):gsub("%d+", "#")
  if not seen[k] then seen[k] = true print("SCRIPT-ERROR " .. tostring(e)) end
end
print("STUBBED " .. table.concat(m.stubbed or {}, " "))
local okh, h = pcall(hud) if okh then print("HUD " .. shallow(h)) end
local okg, g = pcall(game) if okg then print("GAME " .. shallow(g)) end
print("SHAKE-DONE")
