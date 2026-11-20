-- The countdown timer (docs/HUD.md, *Countdown timer*): timer_start counts
-- down and shows m:ss, under ten seconds m:ss.t, blinking faster as it runs
-- out; at zero its script runs once and 0:00.0 blinks five seconds;
-- timer_stop ends it before then (and its script never runs).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/timer.lua

mission_begin(false)
mission_exec([[
F.t_start = function() C.timer_start(12.5, "t_out") end
F.t_short = function() C.timer_start(5, "t_out") end
F.t_stop = function() C.timer_stop() end
F.t_out = function() C.chr_set_health(0, 77) end
]])
step(5)
local hp0 = hp(0)

assert(mission_call("t_start"))
step(2)
local shown, ticks, mode = timer()
print("started:", shown, ticks, mode)
assert(mode == "on" and shown == "0:12", "12.5 s shows 0:12")

-- Down to 11 s (under twenty it blinks: the next time it shows), then
-- under 10 (tenths).
local function next_shown()
  for _ = 1, 40 do local s = timer() if s then return s end step(1) end
end
step(ticks - 11 * 60 - 20)
local t11 = next_shown()
print("at eleven seconds:", tostring(t11))
assert(t11 == "0:11", "0:11")
step(2 * 60)
local t9 = select(1, timer())
print("under ten seconds:", tostring(t9))
assert(t9 == nil or t9:match("^0:0%d%.%d$"), "m:ss.t under ten seconds (or blinked off)")

-- It blinks: over half a second some ticks show nothing.
local off = 0
for _ = 1, 30 do step(1) if select(1, timer()) == nil then off = off + 1 end end
print("blinked off", off, "of 30 ticks")
assert(off > 0 and off < 30, "blinks under twenty seconds")

-- Run out: the script runs once, 0:00.0 blinks five seconds, then nothing.
local _, left = timer()
step(left + 1)
local s, _, m = timer()
print("ran out:", tostring(s), m, "hp", hp(0))
assert(m == "fail" and (s == nil or s == "0:00.0"), "0:00.0 after running out")
assert(hp(0) == 77, "its script ran")
step(5 * 60 + 2)
assert(timer() == nil, "gone after five seconds")

-- Stopped in time: no script.
set_hp(0, hp0)
assert(mission_call("t_short"))
step(2 * 60)
assert(mission_call("t_stop"))
step(5 * 60)
print("stopped: timer", tostring(timer()), "hp", hp(0))
assert(timer() == nil and hp(0) == hp0, "stopped: gone, its script never ran")
print("ok")
