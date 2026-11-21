-- Death (#12, #57): a body stays where it fell — unless its class deletes
-- its dead after a delay (the mad bomber: 190 ticks), as the original does.
-- Settled and out of view, a body becomes a corpse (the character gone, its
-- pose kept in the level's corpse ring); past the ring's size the oldest is
-- recycled (20 bodies as shipped — the level's own among them —, 50 fixed:
-- quirk Q21).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/death.lua

step(10)
local px, py, pz = pos()
local thug = spawn(px + 20, py + 5, pz, 0, "thug_wh_1")
local bomber = spawn(px - 20, py + 5, pz, 0, "mad_bomber")
step(30)
damage(thug, 10000)
damage(bomber, 10000)
assert(dead(thug) and dead(bomber), "both die")
step(150)
assert(present(bomber), "the bomber's body lies a while")
step(60)
print("after 210 ticks: thug", present(thug), "bomber", present(bomber))
assert(not present(bomber), "then its class deletes it")
local function dynamic()
    local n = 0
    for _, c in ipairs(corpses()) do
        if c.dynamic and c.drawn then n = n + 1 end
    end
    return n
end
step(600)
assert(present(thug) or dynamic() == 1, "a thug's body stays")
-- Out of view (turned away from it), it is a corpse now.
facing(math.deg(math.atan(-1, 0)))
step(60)
print("thug present", present(thug), "corpses of play", dynamic())
assert(not present(thug) and dynamic() == 1, "a corpse in its place")
local cx = corpses()[#corpses()].x
assert(math.abs(cx - (px + 20)) < 15, "where it lay: " .. cx)

-- The ring: 20 in all as shipped (level 1 has one of its own), else 50.
local fixed = #corpses() - 1
for k = 1, 25 do
    local t = spawn(px + 20, py + 5, pz + k * 3, 0, "thug_wh_1")
    step(2)
    damage(t, 10000)
end
step(200)
-- (Konoko off past them, looking away: they are behind the camera.)
teleport(0, px, py + 5, pz + 200)
facing(0)
step(200)
local cap = faithful() and 20 or 50
print("corpses", #corpses(), "of play", dynamic(), "cap", cap)
assert(#corpses() == math.min(cap, fixed + 26), "the ring holds " .. cap)
print("death OK")
