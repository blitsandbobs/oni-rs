-- Goals and jobs (issue #4, docs/GOALS.md): a level's character patrols its
-- path (guarding two flags in turn), fights an enemy it sees, checks the
-- body and goes back to its patrol; a noise sends it to investigate; a
-- character too calm for its job stands idle; a non-combatant panics.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/goals.lua

local function flat(ax, az, bx, bz)
    return math.sqrt((ax - bx) ^ 2 + (az - bz) ^ 2)
end
local function near(id, x, z)
    local px, _, pz = pos_of(id)
    return flat(px, pz, x, z)
end
local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

step(10)

-- Bay2_Thug_1 patrols path 62: guard flag CL-7055 (177, -548) for 300
-- ticks, then CL-7056 (86, -568), and loop.
local t = place("Bay2_Thug_1")
step(1)
print("placed:", goal_of(t), alert_of(t))
assert(goal_of(t):find("job patrol patrol_62 @0"), "it takes up its patrol")
local took = wait_for(function() return near(t, 177, -548) < 6 end, 900)
print("reached its first post after", took)
assert(took, "it walks to its first post")
step(200)
assert(goal_of(t):find("@0") and near(t, 177, -548) < 6, "it stands guard there")
local f = facing_of(t)
print(string.format("guarding, facing %.0f° (the flag's 250°)", f))
assert(math.abs(f - 250) < 20, "facing the flag's way")
took = wait_for(function() return near(t, 86, -568) < 6 end, 1200)
print("reached its second post after", took)
assert(took and goal_of(t):find("@1"), "then it guards the second flag")
took = wait_for(function() return goal_of(t):find("@0") ~= nil end, 900)
assert(took, "and loops back to the first")

-- An enemy in plain view: a startle, combat, melee — it fights.
took = wait_for(function() return near(t, 177, -548) < 6 end, 900)
assert(took, "back at the first post")
local d = spawn(131, 8, -567, 250, "konoko_generic") -- in front, back turned
team(d, "konoko")
set_hp(d, 15)
took = wait_for(function() return goal_of(t):find("combat") ~= nil end, 120)
print("sees the enemy:", goal_of(t), alert_of(t))
assert(took and alert_of(t) == "combat", "it sees the enemy and fights")
took = wait_for(function() return dead(d) end, 900)
print("enemy down after", took)
assert(took, "it beats the enemy")
-- It checks the body, then (alerted) goes back to its patrol.
took = wait_for(function() return goal_of(t):find("job patrol") ~= nil end, 900)
print("back on patrol:", goal_of(t), alert_of(t))
assert(took, "it returns to its job")
assert(alert_of(t) == "high", "leaving combat, the alert drops to high")

-- A danger noise off its route: it goes to look (running, at high alert).
step(60)
local nx, nz = 117, -548
noise(nx, 2, nz, "danger", 300)
step(2)
print("after the noise:", goal_of(t))
assert(goal_of(t) == "pursuit goTo", "it goes to investigate")
took = wait_for(function() return goal_of(t) == "pursuit look" end, 600)
-- (It stops once it has seen the spot clearly from within 60 — at once in
-- the shipped game, Q10.)
print(string.format("looking around %.0f from the noise", near(t, nx, nz)))
assert(took and near(t, nx, nz) < 60, "it gets there and looks around")
local seen = {}
for _ = 1, 20 do
    step(30)
    seen[math.floor(facing_of(t) / 30)] = true
end
local turns = 0
for _ in pairs(seen) do turns = turns + 1 end
print("faced", turns, "different ways while looking")
assert(turns >= 3, "it looks about")

-- Too calm for its job: a patroller below its job alert stands idle.
local idle = place("dum_train_block")
step(2)
print("dum_train_block:", goal_of(idle), alert_of(idle))
assert(goal_of(idle) == "idle", "below its job alert it stands idle")

-- A non-combatant panics at the sight of an enemy (cowering — the panic
-- animations — not fighting), and calms down once the danger is gone.
local n = place("Bay6_Neutral_1")
took = wait_for(function() return goal_of(n):find("@6") ~= nil end, 600)
assert(took, "the non-combatant takes its post")
step(120)
local nx2, _, nz2 = pos_of(n)
local e = spawn(nx2 + 28, 58, nz2 - 28, 315, "thug_wh_1") -- 40 in front of it
team(e, "syndicate")
took = wait_for(function() return goal_of(n):find("panic") ~= nil end, 120)
print("sees the enemy:", goal_of(n), anim_of(n))
assert(took, "it panics")
step(30)
assert(anim_of(n):find("PAN"), "and cowers")
damage(e, 1000)
took = wait_for(function() return goal_of(n):find("job patrol") ~= nil end, 600)
print("danger gone, calmed down after", took)
assert(took and took > 100, "it calms down a while after the danger is gone")

print("goals OK")
