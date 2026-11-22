-- AI turning (#89): the body turns at most 2π/100 a tick times the class's
-- turning nimbleness (ninjas 2), half that walking about its everyday
-- business (`AI2rExecutor_GetMaxDeltaFacingPerFrame`); a standing AI
-- turning plays the step-turn clip of the way it turns (Q30: as shipped,
-- the other way).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ai_turning.lua

local STANDING_TURN_LEFT, STANDING_TURN_RIGHT = 7, 8
step(20)
local px, py, pz = pos()

-- How far fighter `id` turns over `ticks`, standing and told to face `deg`;
-- and the step-turn clip types it played.
local function turn(id, deg, ticks)
    set_facing(id, 0)
    ai_order(id, "stop")
    step(30)
    local before = facing_of(id)
    ai_order(id, "stop", "run", deg)
    local seen = {}
    for _ = 1, ticks do
        step(1)
        seen[type_of(id)] = true
    end
    return (facing_of(id) - before + 540) % 360 - 180, seen
end

-- Konoko's class (nimbleness 1): 3.6° a tick; turning left (facing up,
-- toward +X) plays the left step-turn.
local a = spawn(px + 30, py + 5, pz, 0)
step(30)
local turned, seen = turn(a, 90, 10)
print(string.format("nimbleness 1: %.1f° in 10 ticks", turned))
assert(math.abs(turned - 36) < 1, "3.6° a tick")
if faithful() then
    assert(seen[STANDING_TURN_RIGHT] and not seen[STANDING_TURN_LEFT],
        "as shipped, the right step-turn for a left turn")
else
    assert(seen[STANDING_TURN_LEFT] and not seen[STANDING_TURN_RIGHT],
        "the left step-turn for a left turn")
end
turned, seen = turn(a, -90, 10)
assert(math.abs(turned + 36) < 1, "the other way as fast")

-- A ninja (nimbleness 2): twice the rate.
local n = spawn(px - 30, py + 5, pz, 0, "ninja_med_1")
step(30)
turned = turn(n, 90, 10)
print(string.format("nimbleness 2: %.1f° in 10 ticks", turned))
assert(math.abs(turned - 72) < 1, "7.2° a tick")
print("ai turning ok")
