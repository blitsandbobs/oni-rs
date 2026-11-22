-- Automatic blocking: a character standing still and facing its attacker
-- (within 20°) blocks ordinary strikes — no damage, it plays its block. Turned
-- further away, it can't block and the strike lands.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/block.lua

local BLOCK = 23

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 6, 180) -- facing the player head-on
step(60)
facing(0)

local function punch_and_watch()
    local hp0, blocked = hp(d), false
    punch()
    for _ = 1, 20 do
        step(1)
        if type_of(d) == BLOCK then blocked = true end
    end
    return hp0, blocked
end

-- Head-on: blocked, no damage.
local hp0, blocked = punch_and_watch()
print(string.format("head-on:   blocked=%s hp %d -> %d", tostring(blocked), hp0, hp(d)))
assert(blocked, "the dummy blocked the head-on punch")
assert(hp(d) == hp0, "a blocked punch does no damage")

-- Turned 60° away from the player (who is still in front of it): the punch
-- lands.
step(90)
set_facing(d, 180 + 60)
step(10)
facing(0)
hp0, blocked = punch_and_watch()
print(string.format("turned 60°: blocked=%s hp %d -> %d", tostring(blocked), hp0, hp(d)))
assert(not blocked, "a dummy facing 60° away doesn't block")
assert(hp(d) < hp0, "the punch lands")
print("block ok: head-on strikes are blocked, off-angle ones land")
