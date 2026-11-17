-- Example oni-view scenario (headless Lua test driver).
--
-- Run with:
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/run.lua
--
-- API: input(fwd,back) · facing(deg) · turn(deg) · step(n) ·
--      cam(ex,ey,ez, tx,ty,tz) · pos() -> x,y,z · anim() -> name ·
--      screenshot(path [,w,h])

assert(anim() == "KONOKOidle1", "starts idling, got " .. anim())

-- Hold forward for a second: the state machine runs its start → loop chain.
input(true, false)
step(60)
local x, y, z = pos()
print(string.format("after 60 ticks: feet %.0f,%.0f,%.0f  clip=%s", x, y, z, anim()))
assert(z > 330, "root motion carried her forward")

-- A side-on shot of her mid-run.
cam(250, 15, 338,  207, 6, 332)
screenshot("/tmp/oni_run.png", 720, 900)

-- Release: she settles back to a resting pose.
input(false, false)
step(90)
print("settled on: " .. anim())
