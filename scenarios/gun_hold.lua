-- The gun in the hand (docs/WEAPONS.md, *Holding*): as the original builds
-- its matrix — riding the hand while the clip has no gun aiming screen (a
-- pickup: reaching down for the gun, it moves with the hand), swinging to
-- the aim when it has one (standing: the barrel straight down the aim), the
-- aim held to the screen's reach; a left-handed class holds it in its left
-- hand.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/gun_hold.lua

local function dot(a, b) return a[1]*b[1] + a[2]*b[2] + a[3]*b[3] end
local function len(a) return math.sqrt(dot(a, a)) end
local function unit(a) local l = len(a) return {a[1]/l, a[2]/l, a[3]/l} end

step(10)
facing(0)
local x, y, z = pos()
assert(spawn_weapon("w1_tap", x, y + 1, z + 3))
swap()
local rode = 0
for _ = 1, 30 do
    step(1)
    local g = held_gun(0)
    if g and anim():find("pickup") then
        local hand = bone(0, 18)
        assert(dot(unit(g.x), unit(hand.x)) > 0.999, "picking up, the gun rides the hand")
        rode = rode + 1
    end
end
print("ticks the gun rode the hand while picking up:", rode)
assert(rode > 0, "seen in the hand during the pickup")
step(60)
local g = held_gun(0)
print("standing:", anim(), string.format("barrel (%.2f %.2f %.2f)", g.x[1], g.x[2], g.x[3]))
assert(math.abs(g.x[3] - 1) < 0.01, "standing, the barrel points down the aim (facing +z)")
-- Aiming far up: held to the pistol screen's reach, short of straight up.
aim(0, 1.5)
step(20)
g = held_gun(0)
print(string.format("aimed 1.5 up: barrel rises %.2f", g.x[2]))
assert(g.x[2] > 0.3 and g.x[2] < 0.99, "the aim is held to the screen's reach")
aim(0, 0)

-- A left-handed ninja's gun is in its left hand.
local n = spawn(x + 20, y + 5, z + 20, 180, "ninja_med_1")
step(40)
assert(give_weapon(n, "w1_tap"), "a pistol for the ninja")
step(30)
g = held_gun(n)
local left, right = bone(n, 14), bone(n, 18)
local dl = len({g.at[1] - left.at[1], g.at[2] - left.at[2], g.at[3] - left.at[3]})
local dr = len({g.at[1] - right.at[1], g.at[2] - right.at[2], g.at[3] - right.at[3]})
print(string.format("ninja's gun: %.2f from its left hand, %.2f from its right", dl, dr))
assert(dl < 0.01 and dr > 1, "held in the left hand")
print("gun hold ok")
