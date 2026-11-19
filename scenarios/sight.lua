-- The gun's sight (docs/WEAPONS.md, *Sight*): from the sight on the gun
-- along its direction to what it meets, stopping 2 short; a laser in the
-- class's colour for the guns that have one, a tunnel of sprites for
-- others; the player's only; none with an empty gun or one poking through a
-- wall — which doesn't fire either.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/sight.lua

local function sub(a, b) return {a[1]-b[1], a[2]-b[2], a[3]-b[3]} end
local function len(a) return math.sqrt(a[1]*a[1] + a[2]*a[2] + a[3]*a[3]) end

step(20)
facing(0)
local x, y, z = pos()

-- Run up to the low wall at the end of the hall: the pistol pokes over it.
assert(give_weapon(0, "w1_tap"))
step(12)
input("forward")
local through = false
for _ = 1, 400 do
    step(1)
    if gun_through_wall(0) then through = true break end
end
input()
step(30)
assert(through and gun_through_wall(0), "the gun through the wall")
assert(sight(0) == nil, "no sight through a wall")
local _, before = weapon(0)
fire(1)
step(20)
local _, after = weapon(0)
print("through the wall: ammo", before, "→", after)
assert(after == before, "a gun through a wall doesn't fire")
teleport(0, x, y + 8, z)
facing(0)
step(30)

assert(give_weapon(0, "w1_tap"), "a pistol")
step(12)
local s = sight(0)
assert(s, "the player's pistol has a sight")
-- From the class's sight marker on the gun (gun space → world).
local g = held_gun(0)
local m = {3.1507, 0.0050, 0.4108}
local want = {}
for k = 1, 3 do want[k] = g.at[k] + m[1]*g.x[k] + m[2]*g.y[k] + m[3]*g.z[k] end
assert(len(sub(s.from, want)) < 0.01, "from the sight on the gun")
assert(s.laser and s.laser[1] == 1 and s.laser[2] == 0 and s.laser[3] == 0, "a red laser")
assert(s.target == "environment" and s.tunnel == 0, "on the wall ahead")
print(string.format("pistol: laser to z %.1f, on the %s", s.to[3], s.target))

-- A character in the way: stops 2 short of its body.
local d = spawn(x, y + 5, z + 60, 180)
step(30)
s = sight(0)
print(string.format("with a dummy at z %.0f: to z %.1f, on a %s", z + 60, s.to[3], s.target))
assert(s.target == "character" and s.to[3] > z + 50 and s.to[3] < z + 60, "on the dummy, short of it")
assert(sight(d) == nil, "only the player's sight")

-- The phase rifle's laser is blue; the scram cannon has a tunnel, no laser.
assert(give_weapon(0, "w3_phr"))
step(12)
s = sight(0)
assert(s.laser and s.laser[3] == 1 and s.laser[1] == 0, "a blue laser")
assert(give_weapon(0, "w7_scc"))
step(12)
s = sight(0)
print("scram cannon: laser", s.laser, "tunnel sprites", s.tunnel)
assert(s.laser == nil and s.tunnel >= 1, "a tunnel, no laser")

-- An empty gun: no sight.
assert(give_weapon(0, "w1_tap"))
step(12)
for _ = 1, 30 do
    local _, ammo = weapon(0)
    if ammo == 0 then break end
    fire(1)
    step(15)
end
local _, ammo = weapon(0)
assert(ammo == 0, "emptied")
step(2)
assert(sight(0) == nil, "no sight with an empty gun")

print("sight ok")
