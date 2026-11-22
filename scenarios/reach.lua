-- How high a jump reaches: in the air
-- the body's collision starts at its lowest point, so tucking the legs
-- lifts it — the original's rule. On level 1's training: a running held
-- jump from a small crate lands on the tall one (19 higher); the ledge
-- after the laser room (23) is out of reach of a jump but not of a flip
-- (with the training's barrier lowered, as the flip lesson does).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/reach.lua

local function try(x, y, z, yaw, run, flip)
    teleport(0, x, y, z)
    facing(yaw)
    step(20)
    input(true, false)
    step(run)
    hold_jump(true)
    for t = 1, 60 do
        if flip and t == 9 then crouch_tap() end
        step(1)
    end
    hold_jump(false)
    input(false, false)
    step(40)
    local _, py = pos()
    return py
end

step(5)
env_show(2012, false) -- (the barrier the flip lesson lowers)

local y = try(-1040, -321, -500, 90, 10, false)
print("ledge, jump: ends at", y)
assert(math.abs(y + 321) < 0.5, "a jump doesn't reach the 23-unit ledge")
y = try(-1040, -321, -500, 90, 10, true)
print("ledge, flip: ends at", y)
assert(math.abs(y + 298) < 0.5, "a flip does")
y = try(-917.5, -329, -269.5, 140.5, 6, false)
print("crate to tall crate: ends at", y)
assert(math.abs(y + 310.1) < 0.5, "a running jump gets from the small crate onto the tall one")
print("reach ok")
