-- The HUD (docs/HUD.md): messages print, fade and go; a new objective is
-- prompted until read; the compass takes a target; a blow lights the hit ring
-- on its side and fades; picking up a new kind of gun says so, once; standing
-- by a gun prompts to pick it up; a cutscene letterboxes the screen and hides
-- the rings; fades go out and back; subtitles show while a line plays.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/hud.lua

local function has(lines, text)
    for _, l in ipairs(lines) do
        if string.find(l, text, 1, true) then return true end
    end
    return false
end

mission_begin(false)
step(2)
mission_exec([[
F.t_msg = function() C.message("autoprompt_usedoor", 120) end
F.t_msg_hold = function() C.message("autoprompt_useconsole") end
F.t_msg_off = function() C.message_remove("autoprompt_useconsole") end
F.t_obj = function() C.objective_set(1) end
F.t_target = function() C.target_set(1, 50) end
F.t_target_off = function() C.target_set(1, 0) end
F.t_letterbox = function() C.begin_cutscene() end
F.t_letterbox_off = function() C.end_cutscene() end
F.t_fade_out = function() C.fade_out(0, 0, 0, 30) end
F.t_fade_in = function() C.fade_in(30) end
F.t_fill = function() C.ui_fill_element("all", 1) end
F.t_hide = function() C.ui_show_element("left", 0) end
F.t_say = function() C.sound_dialog_play("c00_01_29Shinatama") end
F.t_brain = function() C.sound_dialog_play("brain07final") end
]])

-- A message with a time: up, then gone (after its time; the last 64 ticks
-- fading). One without: up until removed, then fades out.
mission_call("t_msg")
step(2)
assert(has(hud().messages, "to open this door"), "the message is up")
step(125)
assert(not has(hud().messages, "to open this door"), "and gone after its time")
mission_call("t_msg_hold")
step(300)
assert(has(hud().messages, "to use this CONSOLE"), "an untimed message stays")
mission_call("t_msg_off")
step(70)
assert(not has(hud().messages, "to use this CONSOLE"), "removed, it fades out")

-- A new objective: prompted (message, compass ring flashing) until read.
mission_call("t_obj")
step(2)
local o = hud()
print("objective:", o.objective, o.new_objective, o.messages[#o.messages])
assert(o.objective == 1 and o.new_objective and has(o.messages, "New objective"), "a new objective is prompted")
read_objective()
step(70)
assert(not hud().new_objective and not has(hud().messages, "New objective"), "read, the prompt goes")

-- The compass's target.
mission_call("t_target")
step(1)
assert(hud().target ~= nil, "the compass has a target")
mission_call("t_target_off")
step(1)
assert(hud().target == nil, "and drops it")

-- A blow from in front (shoving her back, -z, while she faces +z) lights the
-- ring's quarter the original calls "back" (drawn at the top), fading 3 a tick.
set_facing(0, 0)
damage(0, 5, 0, -1)
local hits = hud().hits
print("hit ring:", hits[1], hits[2], hits[3], hits[4])
assert(hits[4] == 255 and hits[1] == 0 and hits[2] == 0 and hits[3] == 0, "the front quarter lights")
step(100)
assert(hud().hits[4] == 0, "and fades")

-- A new kind of gun: its message, once.
give_weapon(0, "w1_tap")
step(2)
assert(has(hud().messages, "EQUALIZER PISTOL"), "a new gun is announced")
drop_weapon(0)
step(300)
give_weapon(0, "w1_tap")
step(2)
assert(not has(hud().messages, "EQUALIZER PISTOL"), "but only the first time")
drop_weapon(0)

-- Standing by a gun lying there: the autoprompt.
local x, y, z = pos()
spawn_weapon("w2_sap", x + 3, y + 1, z + 3)
step(30)
print("by a gun:", hud().messages[#hud().messages])
assert(has(hud().messages, "pick up weapon"), "prompted to pick it up")
teleport(0, x + 80, y, z)
step(90)
assert(not has(hud().messages, "pick up weapon"), "walking away, the prompt goes")

-- A cutscene: the letterbox slides in (75 at 480 high, 1.25 a tick) and the
-- rings go; it slides out after.
local before = hud_triangles()
mission_call("t_letterbox")
step(70)
local lb = hud()
print(string.format("letterbox %.1f; triangles %d → %d", lb.letterbox, before, hud_triangles()))
assert(lb.letterbox == 75 and hud_triangles() < before, "letterboxed, the rings hidden")
mission_call("t_letterbox_off")
step(70)
assert(hud().letterbox == 0, "and back")

-- A fade out to black over 30 ticks, and back in.
mission_call("t_fade_out")
step(31)
assert(hud().fade > 0.99, "faded out")
mission_call("t_fade_in")
step(31)
assert(hud().fade == 0, "and back in")

-- A dialogue line, subtitles on: its subtitle stays for its length and a
-- second (`OSrInGameDialog_Play`). Named with a capital its sound doesn't
-- have — as level 2's scripts name Shinatama's lines — it is still found (the
-- original's lookup ignores case where its hash agrees).
subtitles(true)
mission_call("t_say")
step(2)
print("subtitle:", hud().subtitles[1])
assert(has(hud().subtitles, "Shinatama"), "the subtitle is up")
local gone
for t = 1, 900 do
    step(1)
    if #hud().subtitles == 0 then gone = t break end
end
print("the subtitle went after", gone)
assert(gone and gone > 60, "and goes once the line is over")

-- A line keyed by name rather than number (the Deadly Brain's, level 2):
-- found by its whole name.
mission_call("t_brain")
step(2)
print("brain subtitle:", hud().subtitles[1])
assert(has(hud().subtitles, "Deadly Brain"), "the brain's subtitle is up")
-- Its line runs 123 ticks; the subtitle a second more (then fades).
local brain_gone
for t = 1, 2400 do step(1) if #hud().subtitles == 0 then brain_gone = t break end end
print("the brain's subtitle went after", brain_gone)
assert(brain_gone and brain_gone >= 123 + 60 - 2, "up for the line and a second more")

-- Scripts fill and hide parts of it.
mission_call("t_fill")
mission_call("t_hide")
step(1)
local f = hud()
assert(f.filled == 4095 and not f.show[1] and f.show[2], "all filled, the left ring hidden")
print("ok")
