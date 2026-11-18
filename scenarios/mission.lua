-- The level's own scripts (#20, scripting/PORT.md): level 1 begins as in
-- the game — the player at its placement, `main` runs the training intro —
-- the training waits for the player to do what it asks (turn, run), and a
-- trigger volume fires its script when the player walks in.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/mission.lua

local function showing(m)
    for _, x in ipairs(mission_info().messages) do
        if x == m then return true end
    end
    return false
end
local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

step(5)
mission_begin()
step(2)
local x, y, z = pos()
print(string.format("player starts at %.0f %.0f %.0f", x, y, z))
assert(math.abs(x + 1101) < 3 and math.abs(z + 126) < 3, "the player is at its placement (char_0)")

-- The intro plays, then asks for a mouse turn — and waits for one.
local took = wait_for(function() return showing("xmouse") end, 1200)
print("asks to turn after", took)
assert(took, "the training asks the player to turn")
step(300)
assert(showing("xmouse"), "it waits until the player turns")
turn(120)
took = wait_for(function() return showing("xmove") end, 120)
print("turned; asks to move after", took)
assert(took, "a turn moves the training on")

-- Asked to run: it waits for a run.
step(120)
assert(showing("xmove"), "it waits until the player runs")
input(true, false)
took = wait_for(function() return not showing("xmove") end, 180)
input(false, false)
print("ran; next lesson after", took)
assert(took, "running moves the training on")

-- The next lesson enables a trigger volume (half a second after it is
-- shown); standing in it then runs its entry script, which clears the
-- lesson's messages.
took = wait_for(function() return showing("xconsole1") end, 900)
assert(took, "the console lesson")
teleport(0, -1062, -338, -288)
took = wait_for(function() return not showing("xconsole1") end, 90)
print("walked into the trigger volume:", took)
assert(took, "the trigger volume's entry script ran")

-- The lesson's console: Action in front of it walks Konoko onto it, plays
-- the console animation, and the console runs its script — the training
-- goes on.
assert(console_state(10) == "active", "the lesson turned the console on")
teleport(0, -1063.5, -338, -291)
facing(180)
step(10)
action()
took = wait_for(function() return console_state(10) == "triggered" end, 400)
print("used the console after", took, anim())
assert(took, "Action at the console uses it")
took = wait_for(function() return showing("xactivate2") end, 200)
assert(took, "its script moves the training on")

local info = mission_info()
print("tasks", info.tasks, "errors", #info.errors)
for _, e in ipairs(info.errors) do print("  " .. e) end
assert(#info.errors == 0, "no script errors")
print("mission OK")
