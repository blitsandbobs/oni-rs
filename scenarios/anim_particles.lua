-- Animation particles (#70): a
-- clip puts its class's particles on the body, each on a body part, on over
-- a run of its frames — Konoko's running kick draws a trail from her foot
-- (frames 15–28), and it goes when the clip ends.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/anim_particles.lua

local TRAIL = "h2h_trail_e01"
step(20)
assert(#particles_of(TRAIL) == 0)
input(true, false)
step(40)
kick()
local kicked = false
for _ = 1, 20 do
    step(1)
    if anim() == "KONOKOrun_kick" then kicked = true break end
end
input(false, false)
assert(kicked, "a running kick")
assert(#particles_of(TRAIL) == 1, "the trail made with the clip")
local x0 = particles_of(TRAIL)[1]
for _ = 1, 10 do step(1) end
local x1 = particles_of(TRAIL)[1]
assert(x1 and (x0[1] ~= x1[1] or x0[3] ~= x1[3]), "riding her foot")
while anim() == "KONOKOrun_kick" do step(1) end
step(1)
assert(#particles_of(TRAIL) == 0, "gone with the clip")
print("ok")
