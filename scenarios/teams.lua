-- Teams (#11): a friend's blows pass through (no damage, no
-- reaction); auto-facing turns an attack toward an enemy, never a friend.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/teams.lua

step(20)
local px, py, pz = pos()
-- A dummy just ahead, its back to Konoko (it would block otherwise).
local d = spawn(px, py + 5, pz + 6, 0)
step(60)

-- On Konoko's side (TCTF): the punch passes through.
team(d, "tctf")
facing(0)
local hp0 = hp(d)
punch()
step(40)
print(string.format("friend: hp %d -> %d", hp0, hp(d)))
assert(hp(d) == hp0, "a friend takes no hit")

-- An enemy (Syndicate): it lands.
team(d, "syndicate")
facing(0)
punch()
step(40)
print(string.format("enemy: hp %d -> %d", hp0, hp(d)))
assert(hp(d) < hp0, "an enemy is hit")

-- Auto-facing: punching while turned 30° away swings round to an enemy…
step(60)
local function bearing()
    local ax, _, az = pos()
    local bx, _, bz = pos_of(d)
    return math.deg(math.atan(bx - ax, bz - az))
end
local function off(a, b)
    return math.abs((a - b + 180) % 360 - 180)
end
local want = bearing()
facing(want + 30)
punch()
step(3)
local f = facing_deg()
print(string.format("toward an enemy 30° off: facing %.0f (it is at %.0f)", f, want))
assert(off(f, want) < 8, "turned toward the enemy")

-- …but not to a friend.
team(d, "tctf")
step(60)
want = bearing()
facing(want + 30)
punch()
step(3)
f = facing_deg()
print(string.format("toward a friend 30° off: facing %.0f (it is at %.0f)", f, want))
assert(off(f, want + 30) < 8, "no turn toward a friend")
print("teams OK")
