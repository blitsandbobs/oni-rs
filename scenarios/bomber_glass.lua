-- Script-fired blasts break glass (#95): level 3's bomber cutscene. The
-- window he's thrown through is swapped from its unbreakable pane (778) to
-- a breakable twin (777, same texture), then his harness's first charge
-- (BomberCharge01, `charge_medium`) blows it out; the second
-- (BomberCharge02, `charge_small`) breaks the foyer's glass he falls
-- through. Both are `particle(tag, "do", "explode")` from the script, run
-- out of the particle update.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/bomber_glass.lua

mission_begin(false)
env_show(777, false) -- (level_start)
step(5)

-- The swap, as the cutscene does it.
env_show(777, true)
env_show(778, false)
step(2)
assert(env_broken(777) == 0, "the breakable twin intact")

local function blast(tag)
    local before = #particles_of("glass_shard")
    particle(tag, "do", "explode")
    step(1)
    return #particles_of("glass_shard") - before
end

local window = blast("BomberCharge01")
print("window shards", window)
assert(env_broken(777) == 1, "the window blown out")
assert(window > 0, "in shards")

local foyer = blast("BomberCharge02")
print("foyer shards", foyer)
assert(foyer > 0, "the foyer's glass breaks under him")
print("bomber glass ok")
