-- Consoles and alarms (#19, #4; scripting/BSL.md, *Consoles*): an AI sent
-- to raise the alarm runs to the alarm console, uses it (the console
-- animation; the console fires on its action frame) and goes back to its
-- job; when Konoko trips the same alarm, the AIs answering it learn where
-- she is and come after her.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/alarm.lua

local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

step(5)
-- Level 1's comguy and a striker answering alarm group 4 (which console
-- 6, the bay's alarm, raises).
local c = place("Bay4_Comguy_1")
local s = place("Alarm_Bay4_Striker_1")
step(30)
teleport(c, 426, 102, 480) -- on the console's floor (clear of the crates: no one is
                           -- pushed out of what they're put into)
step(30)
assert(console_state(6) == "active", "the alarm console is armed")
alarm(c, 6)
assert(goal_of(c):find("alarm"), "it sets off for the alarm")
local took = wait_for(function() return console_state(6) == "triggered" end, 600)
print("raised the alarm after", took, anim_of(c))
assert(took and anim_of(c):find("console"), "it runs there and uses the console")
took = wait_for(function() return goal_of(c):find("job") ~= nil end, 120)
assert(took, "then goes back to its job")
-- (A friend raising it: the striker doesn't come running — the level's
-- alarm script deals with that, as in the original.)

-- Konoko trips it: the striker learns where she is and comes for her.
console_reset(6)
teleport(0, 451, 102, 469)
facing(180)
step(20)
action()
took = wait_for(function() return console_state(6) == "triggered" end, 400)
print("Konoko raised it after", took)
assert(took, "Konoko uses the alarm console")
step(5)
print("striker:", goal_of(s), alert_of(s), contact_of(s, 0))
assert(contact_of(s, 0) ~= nil, "the striker knows where she is")
assert(goal_of(s):find("pursuit") or goal_of(s):find("combat"), "and comes for her")
print("alarm OK")
