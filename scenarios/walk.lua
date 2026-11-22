-- Walk modifier: holding Walk (Caps Lock) selects the Walk clips, which cover
-- much less ground than Run over the same window. Both modes are measured from
-- standing (run and walk states don't interconnect mid-stride — faithful to
-- Oni: the modifier takes effect from a standing start).
--
--   cargo run --release -p oni -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/walk.lua

facing(0)

-- Run from standing; measure a 30-tick window once in the loop.
input(true, false)
step(60)
local _, _, zr0 = pos()
local run_anim = anim()
step(30)
local _, _, zr1 = pos()
local run_dist = zr1 - zr0

-- Stop and settle back to standing.
input(false, false)
step(70)

-- Walk from standing; measure the same window.
walk(true)
input(true, false)
step(50)
local walk_anim = anim()
local _, _, zw0 = pos()
step(30)
local _, _, zw1 = pos()
local walk_dist = zw1 - zw0

print(string.format("run: %.1f/30t (%s)   walk: %.1f/30t (%s)",
    run_dist, run_anim, walk_dist, walk_anim))

assert(run_anim:find("run") ~= nil, "run phase is a run clip, got " .. run_anim)
assert(walk_anim:find("walk") ~= nil, "walk modifier reaches a walk clip, got " .. walk_anim)
assert(walk_dist > 2.0, "still moving while walking, got " .. walk_dist)
assert(walk_dist < run_dist * 0.6, "walk is clearly slower than run: walk=" ..
    walk_dist .. " run=" .. run_dist)
