-- A patrol's shoot-at-flag (#6; docs/GOALS.md): on level 4 a thug's patrol
-- creeps to its spot and shoots at a flag for the waypoint's time — rounds
-- spent, the extra spread the waypoint asks for — then goes on (to a scan).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/04/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/patrol_shoot.lua

local d = place("alamo_thug_1")
local class, full = weapon(d)
assert(class == "w1_tap", "placed with its pistol")
local shot = nil
for t = 1, 900 do
    step(1)
    local _, ammo = weapon(d)
    if ammo < full and not shot then shot = t end
end
local _, left = weapon(d)
print("first shot at", shot, "rounds left", left, goal_of(d))
assert(shot, "it shoots at the flag")
assert(goal_of(d):find("@3"), "then goes on to its scan")
print("patrol shoot ok")
