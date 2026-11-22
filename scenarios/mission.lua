-- The level's own scripts (#20, docs/lua.md): level 1 begins as in
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
-- (As in the game, the training only watches for what it asked once
-- Shinatama has finished speaking: act after her line.)
local function quiet()
    for _ = 1, 1200 do
        if not dialog() then return end
        step(1)
    end
end
-- Waits (up to four times `max`: the lines play at their full length) for
-- `cond`, then for Shinatama to finish speaking.
local function wait_for(cond, max)
    for t = 1, max * 4 do
        step(1)
        if cond() then
            quiet()
            return t
        end
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
local took = wait_for(function() return showing("xmouse") end, 3000)
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
-- Its yellow floor target (an environmental particle the script creates).
step(40)
assert(env_particles("obj_control") > 0, "the console's floor target is lit")
teleport(0, -1062, -338, -288)
took = wait_for(function() return not showing("xconsole1") end, 90)
print("walked into the trigger volume:", took)
assert(took, "the trigger volume's entry script ran")

-- The lesson's console: Action in front of it walks Konoko onto it, plays
-- the console animation, and the console runs its script — the training
-- goes on.
assert(console_state(10) == "active", "the lesson turned the console on")
assert(console_screen(10) == "_CON_WH1", "its screen shows it is on: " .. tostring(console_screen(10)))
teleport(0, -1063.5, -338, -291)
facing(180)
step(10)
action()
took = wait_for(function() return console_state(10) == "triggered" end, 400)
print("used the console after", took, anim())
assert(took, "Action at the console uses it")
assert(console_screen(10) == "_CON_USED", "its screen shows it used")
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
-- The course's markers light up (an arrow and a flare each) and go out,
-- beeping, as she passes them.
assert(env_particles("t1") == 2, "the first marker is lit")
local beeped = false
for _, c in ipairs({
    {-1040, -201}, {-1040, -172}, {-1040, -142}, {-1040, -111}, {-1044, -84},
    {-1079, -67}, {-1126, -67}, {-1153, -78}, {-1163, -113}, {-1163, -143},
    {-1163, -173}, {-1163, -202}, {-1163, -232}, {-1163, -260}, {-1150, -282},
    {-1118, -290}, {-1091, -291},
}) do
    visit(c[1], c[2])
    for _ = 1, 4 do
        step(1)
        for _, ch in ipairs(channels()) do
            if ch.clip and ch.clip:match("^countdown_beep") then beeped = true end
        end
    end
end
assert(env_particles("t1") == 0 and beeped, "passed, the markers go out with a beep")
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
-- Its text comes up (the game standing still until it is closed).
took = wait_for(function() return text_console() ~= nil end, 300)
assert(took, "the review console shows its text")
print("review text:", text_console().name, text_console().pages, "page(s)")
text_console_key("close")
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
took = wait_for(function() return showing("c01_50_09") end, 8000)
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
quiet()
away_from_walls()
jump() step(8) crouch_tap() step(60)
took = wait_for(function() return showing("c01_50_23") end, 2500)
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
    -- (The lesson wants a forward throw: the dummy faces her.)
    set_facing(d2, 180)
    facing(0)
    step(5)
    input(true, false) step(2) punch() step(30) input(false, false) step(30)
    if showing("xthrow1") or not showing("c01_50_41") then thrown = true break end
end
print("throw:", thrown)
assert(thrown, "a throw moves the lesson on")

-- The block room: its drone comes at Konoko a while, then the training
-- sends her on to shooting.
took = wait_for(function() return showing("goto_block") end, 2400)
assert(took, "on to blocking")
visit(-700, -632, -296)
took = wait_for(function() return showing("c01_50_33") end, 300)
assert(took, "the block lesson")
visit(-629, -610, -296)
took = wait_for(function() return showing("goto_shoot") end, 2400)
print("block lesson done after", took, "hp", hp(0))
assert(took, "on to shooting")

-- The shooting range (#10): the training puts out a pistol; Konoko picks it
-- up (the pickup clip), fires (the lesson waits for the recoil), breaks the
-- 18 glass targets — picking up the clips the training puts out when she
-- runs dry, and reloading — and is handed the rifle.
visit(-692, -541, -297)
took = wait_for(function() return weapons_lying() > 0 end, 300)
assert(took, "a pistol to pick up")
visit(-660, -487, -297)
step(40)
took = wait_for(function()
    if weapon(0) == nil then swap() end
    return weapon(0) == "w1_tap"
end, 400)
print("pistol picked up after", took)
assert(took, "picked up the pistol")

-- Each target column, from 30 units on its -X side; the rows are 8, 17
-- and 26 above the floor. (The pistol's shot leaves 3° off the barrel —
-- aim a touch right; pitches found for a muzzle 23 units from the pane.)
local columns = {
    {-510.0, -517.9}, {-478.8, -506.0}, {-447.6, -493.4},
    {-447.6, -420.5}, {-478.8, -406.3}, {-510.0, -390.4},
}
local rows = {-0.15, 0.21, 0.45} -- (the muzzle rises as the gun tips up)
local function refill()
    -- Out of rounds: the training puts out two clips; pick them up, reload.
    if select(2, weapon(0)) > 0 then return end
    -- A spare clip (both picked up last time): just reload.
    if inventory(0) > 0 then
        assert(reload(0), "reload")
        step(80)
        assert(select(2, weapon(0)) == 10, "full again")
        return
    end
    local before = powerups_lying()
    local spawned = wait_for(function() return powerups_lying() >= before + 2 end, 600)
    assert(spawned, "clips put out when the pistol runs dry")
    for _, at in ipairs({{-660, -487}, {-651, -482}}) do
        visit(at[1], at[2], -297)
        step(10)
        local had = inventory(0)
        wait_for(function()
            if inventory(0) > had then return true end
            swap()
            return false
        end, 200)
    end
    step(60)
    print("refill: clips", inventory(0))
    assert(reload(0), "reload")
    step(80)
    assert(select(2, weapon(0)) == 10, "full again")
end
local shots = 0
for _, c in ipairs(columns) do
    for _, pitch in ipairs(rows) do
        -- (The gun follows the hand through the pistol idle, so a shot can
        -- stray by a hair: a miss is tried again, nudged.)
        for try = 0, 3 do
            refill()
            local before = env_broken(3001, 3018)
            teleport(0, c[1] - 30, -297, c[2])
            step(10)
            facing(93)
            aim(0, pitch + ({0, 0.03, -0.03, 0.06})[try + 1])
            step(5)
            fire(1)
            shots = shots + 1
            step(20)
            if env_broken(3001, 3018) > before then break end
        end
    end
end
local broken = env_broken(3001, 3018)
print("targets broken:", broken, "of 18 with", shots, "shots")
assert(broken == 18, "every target broken")
took = wait_for(function() return weapons_lying() > 0 and weapon(0) == "w1_tap" end, 600)
assert(took, "the rifle is put out")
visit(-660, -452, -297)
step(30)
took = wait_for(function()
    if weapon(0) ~= "w3_phr" then swap() end
    return weapon(0) == "w3_phr"
end, 400)
print("rifle picked up after", took)
assert(took, "picked up the rifle")

-- The two training droids, one after the other: face each, aim at its
-- chest and fire the rifle until it falls (cells come out, and she
-- reloads, when it runs dry); the second's fall ends the lesson.
local function refill_cells()
    -- Dry: the training puts out cells at two spots; collect, reload.
    if select(2, weapon(0)) > 0 then return end
    for _ = 1, 4 do
        for _, at in ipairs({{-652, -458}, {-660, -452}}) do
            visit(at[1], at[2], -297)
            step(10)
            local c0 = select(2, inventory(0))
            wait_for(function()
                if select(2, inventory(0)) > c0 then return true end
                swap()
                return false
            end, 120)
        end
        if select(2, inventory(0)) > 0 then break end
        step(120)
    end
    step(60)
    reload(0)
    step(140)
end
local function shoot_down(name)
    local id = wait_for(function() return named(name) ~= nil end, 600) and named(name)
    assert(id, name .. " is there")
    for _ = 1, 40 do
        if hp(id) <= 0 then return true end
        refill_cells()
        -- Close in (40 units, on its -X side), and lead it: the bolt starts
        -- slow and speeds up, reaching it in about 12 ticks.
        local x1, _, z1 = pos_of(id)
        step(2)
        local x, y, z = pos_of(id)
        local vx, vz = (x - x1) / 2, (z - z1) / 2
        teleport(0, x - 40, -297, z)
        step(4)
        x, y, z = pos_of(id)
        local tx, tz = x + vx * 16, z + vz * 16
        local px, py, pz = pos()
        facing(math.deg(math.atan(tx - px, tz - pz)))
        local flat = math.sqrt((tx - px) ^ 2 + (tz - pz) ^ 2)
        aim(0, math.atan((y + 10) - (py + 11), math.max(flat, 1)))
        step(2)
        fire(1)
        step(60)
    end
    return hp(id) <= 0
end
local killed1 = shoot_down("dum_train_shoot")
print("first droid down:", killed1)
assert(killed1, "shot the first droid")
local killed2 = shoot_down("dum_train_shoot2")
print("second droid down:", killed2)
assert(killed2, "shot the second droid")
took = wait_for(function() return showing("xlastreview") end, 2400)
print("on to the last review after", took)
assert(took, "the shooting lesson ends")

-- The last review console: Action at it ends the training.
assert(console_state(12) == "active", "the last review console is on")
teleport(0, -659.7, -297, -470.1)
facing(90)
step(10)
action()
took = wait_for(function() return console_state(12) == "triggered" end, 400)
print("last review used after", took)
assert(took, "Action at the last review console uses it")
step(120)

local info = mission_info()
print("tasks", info.tasks, "errors", #info.errors)
for _, e in ipairs(info.errors) do print("  " .. e) end
assert(#info.errors == 0, "no script errors")
print("mission OK")
