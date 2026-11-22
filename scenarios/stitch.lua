-- Clip-to-clip stitching: the idle→run interrupt blends instead of snapping.
--
--   cargo run --release -p oni -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/stitch.lua

assert(anim() == "KONOKOidle1", "starts idling")
assert(stitch() == 0, "not blending while idle")

-- Press forward: the first move clip is selected and a blend begins.
input(true, false)
step(4)
print(string.format("tick 4: clip=%s  stitch=%.2f", anim(), stitch()))
assert(stitch() > 0, "stitch blend still active a few frames into the interrupt")

-- Mid-blend shot (the pose is between idle and the run clip, not a snap).
local x, y, z = pos()
cam(x + 9, y + 9, z + 8, x, y + 5, z)
screenshot("/tmp/oni_stitch_mid.png", 720, 900)

-- The 8-frame blend decays to zero.
step(10)
print(string.format("after 11 ticks: clip=%s  stitch=%.2f", anim(), stitch()))
assert(stitch() == 0, "blend finished (<=8 frames)")
