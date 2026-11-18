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

-- The dash lesson: a double-tapped forward dashes (the training waits for
-- the dash animation).
took = wait_for(function() return showing("xdash1") end, 600)
assert(took, "the dash lesson")
facing(0)
input(true, false) step(4) input(false, false) step(4) input(true, false)
step(40)
print("dashing:", anim())
input(false, false)
took = wait_for(function() return showing("xdash2") end, 120)
assert(took, "the dash is recognised")

-- The course: into its start, then past the 17 markers in order and into
-- the finish within the time — a great run.
took = wait_for(function() return showing("xgotrack") end, 900)
assert(took, "on to the course")
local function visit(x, z, y)
    teleport(0, x, y or -338, z)
    step(4)
end
visit(-1090, -290) -- the start
took = wait_for(function() return showing("xrun") end, 200)
assert(took, "the course is on")
for _, c in ipairs({
    {-1040, -201}, {-1040, -172}, {-1040, -142}, {-1040, -111}, {-1044, -84},
    {-1079, -67}, {-1126, -67}, {-1153, -78}, {-1163, -113}, {-1163, -143},
    {-1163, -173}, {-1163, -202}, {-1163, -232}, {-1163, -260}, {-1150, -282},
    {-1118, -290}, {-1091, -291},
}) do
    visit(c[1], c[2])
end
visit(-1039, -236) -- the finish
took = wait_for(function() return showing("xgreat") end, 300)
print("course run; verdict after", took)
assert(took, "a clean, quick run is great")

-- The review: its floor target, then its console.
took = wait_for(function() return showing("xreview") end, 700)
assert(took, "the review")
visit(-1100, -95)
teleport(0, -1100.3, -338, -94)
facing(0)
step(10)
action()
-- (Its script resets it at once, and sends Konoko on to the door.)
took = wait_for(function() return showing("xgodoor") end, 600)
assert(took, "the review console moves the training on")

-- The door lesson: at its target the training unlocks the door (a manual
-- one); Action opens it and the jump lesson starts beyond it.
local _, locked = door_state(98)
assert(locked, "the door starts locked")
visit(-1042, -278)
took = wait_for(function() _, locked = door_state(98) return not locked end, 120)
assert(took, "the training unlocks it")
teleport(0, -1036, -338, -278)
facing(90)
step(10)
action()
step(5)
print("door 98 after Action:", door_state(98))
assert(door_state(98) ~= "closed", "Action opens it")
input(true, false)
took = wait_for(function() local x = pos() return x > -1010 end, 200)
input(false, false)
assert(took, "through the door")
took = wait_for(function() return showing("jump_basic") end, 300)
assert(took, "the jump lesson")

-- Jump, then a jump on the move: the training goes on and lowers the
-- barrier (env_show) to the next part.
took = wait_for(function() return showing("c01_50_06") end, 400)
assert(took, "asked to jump")
jump()
took = wait_for(function() return not showing("c01_50_06") end, 200)
assert(took, "a jump")
took = wait_for(function() return showing("c01_50_07") end, 200)
assert(took, "asked to jump on the move")
input(true, false)
step(10)
jump()
step(30)
input(false, false)
took = wait_for(function() return showing("jump1") end, 700)
print("jump lessons done after", took)
assert(took, "on to the jumping course")

-- The crouch lessons (in the laser room beyond): crouch; jump up to the
-- next target to watch the combat demo (two trainees turn Syndicate and
-- beat the third — AI against AI); then creep, a somersault, a backward
-- cartwheel and a slide — after which the training switches the lasers on.
teleport(0, -1042, -319, -356)
took = wait_for(function() return showing("c01_50_08") end, 500)
assert(took, "asked to crouch")
crouch(true) step(30) crouch(false)
took = wait_for(function() return showing("xfite") end, 700)
assert(took, "the crouch lesson goes on")
teleport(0, -1061, -300, -313)
took = wait_for(function() return showing("c01_50_09") end, 3000)
print("the combat demo ended after", took)
assert(took, "the demo ends (one trainee beaten) and the lessons resume")
local function away_from_walls()
    teleport(0, -1080, -319, -400)
    facing(0)
    step(20)
end
away_from_walls()
crouch(true) step(15) input(true, false) step(40) input(false, false) crouch(false) step(20)
took = wait_for(function() return showing("c01_50_11") end, 600)
assert(took, "asked for a somersault")
away_from_walls()
input(true, false) step(30) crouch_tap() step(40) input(false, false)
took = wait_for(function() return showing("back") end, 400)
assert(took, "asked for a cartwheel")
away_from_walls()
crouch(true) input(false, true) step(25) input(false, false) crouch(false)
took = wait_for(function() return showing("c01_50_16") end, 400)
assert(took, "asked for a slide")
away_from_walls()
input(true, false) step(4) input(false, false) step(4) input(true, false)
step(40) crouch_tap() step(40) input(false, false)
took = wait_for(function() return showing("slide") end, 400)
assert(took, "on to the lasers")
took = wait_for(function() return (laser_state(1)) == true end, 200)
print("lasers on after", took, "beams", #beams())
assert(took and (laser_state(3)) and (laser_state(2)), "the training switches the three lasers on")

-- A flip (jump, then crouch in the air); the training lowers the next
-- barrier and the fighting lessons begin in the next room — the lasers go
-- off, and it asks for a punch, then a kick.
took = wait_for(function() return showing("c01_50_19") end, 1500)
assert(took, "asked to flip")
away_from_walls()
jump() step(8) crouch_tap() step(60)
took = wait_for(function() return showing("c01_50_23") end, 600)
assert(took, "the flip is recognised")
teleport(0, -1011, -296, -505)
took = wait_for(function() return showing("xpunch") end, 600)
assert(took, "the fighting lessons")
assert(laser_state(1) == false and laser_state(2) == false and laser_state(3) == false, "the lasers are off")
punch()
took = wait_for(function() return showing("xkick") end, 300)
assert(took, "a punch")
kick()
took = wait_for(function() return not showing("xkick") end, 300)
print("punched and kicked; the fighting lessons go on")
assert(took, "a kick")

-- Combos: two punches; then punch, punch, kick.
took = wait_for(function() return showing("punch") end, 400)
assert(took, "asked for two punches")
punch() step(10) punch() step(40)
took = wait_for(function() return showing("c01_50_29") end, 600)
assert(took, "asked for punch-punch-kick")
step(30)
punch() step(10) punch() step(22) kick() step(60)
took = wait_for(function() return showing("xspar2") end, 900)
print("combos done after", took)
assert(took, "on to sparring")

-- The hit-flash room: hitting its dummy (it can't be killed for good —
-- death-locked) runs its hurt script and the lesson.
visit(-855, -482, -296)
took = wait_for(function() return showing("start_hit") end, 400)
assert(took, "the hit lesson")
local dummy = named("dum_hit_flash")
assert(dummy, "its dummy is there")
local function hit(target)
    local x, y, z = pos_of(target)
    teleport(0, x, y + 2, z - 9)
    facing(0)
    step(5)
    punch()
    step(40)
end
for _ = 1, 4 do
    if showing("c01_50_35") then break end
    hit(dummy)
end
took = wait_for(function() return showing("c01_50_35") end, 200)
print("hit the dummy; lesson after", took)
assert(took, "hurting the dummy moves the lesson on")

-- The throw room: a forward punch close to its dummy throws it.
took = wait_for(function() return showing("goto_throw") end, 2400)
assert(took, "on to throwing")
visit(-746, -575, -296)
took = wait_for(function() return showing("c01_50_41") end, 400)
assert(took, "asked to throw")
local thrown
local d2 = named("dum_train_throw")
assert(d2, "the throw dummy is there")
for _ = 1, 6 do
    local x, y, z = pos_of(d2)
    teleport(0, x, y + 2, z - 6)
    facing(0)
    step(5)
    input(true, false) step(2) punch() step(30) input(false, false) step(30)
    if showing("xthrow1") or not showing("c01_50_41") then thrown = true break end
end
print("throw:", thrown)
assert(thrown, "a throw moves the lesson on")

local info = mission_info()
print("tasks", info.tasks, "errors", #info.errors)
for _, e in ipairs(info.errors) do print("  " .. e) end
assert(#info.errors == 0, "no script errors")
print("mission OK")
