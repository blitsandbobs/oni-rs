-- Lens flares (docs/PARTICLES.md, *Lens flares*): the doors' lock lights,
-- emergency lights and lamps are drawn over the scene where their point is
-- seen from the camera, fading in and out; the lock lights take their colour
-- from their class's blend (red until a script unlocks them).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/lens.lua

step(30)
local total, seen = lens_flares()
print("flares:", total, "seen at the start:", seen)
assert(total > 60, "the level's flares are tracked")
assert(seen == 0, "none in sight from the start")

-- The lock light beside the warehouse door at (-201, 13, -672), seen from
-- off to its side (not over her shoulder: her body would hide it): it fades
-- in.
teleport(0, -251, 2, -623)
set_facing(0, 150)
step(120)
local near = lens_flare_near(-201, 13, -672)
print(string.format("by the door: %.2f", near))
assert(near > 0, "the lock light is seen")
local light = particles_of("locklight")[1]
print(string.format("its colour: %08x", light[7]))
assert(light[7] == 0xffff0000, "red: locked")

-- Turned away, the light is off the screen: it fades out (the original's
-- octree leaf isn't seen); turned back, it fades in again.
set_facing(0, 330)
step(120)
print(string.format("turned away: %.2f", lens_flare_near(-201, 13, -672)))
assert(lens_flare_near(-201, 13, -672) == 0, "off the screen, faded out")
set_facing(0, 150)
step(120)
print(string.format("turned back: %.2f", lens_flare_near(-201, 13, -672)))
assert(lens_flare_near(-201, 13, -672) > 0, "back on the screen, faded in")

-- Standing between the camera and the light, Konoko hides it.
teleport(0, -201, 2, -652)
set_facing(0, 180)
step(90)
print(string.format("behind her: %.2f", lens_flare_near(-201, 13, -672)))
assert(lens_flare_near(-201, 13, -672) == 0, "her body hides it")

-- Back at the start, facing as there: out of sight, it fades out.
teleport(0, 200, 8, 300)
set_facing(0, 0)
step(120)
local _, gone = lens_flares()
assert(gone == 0, "out of sight, gone")
print("ok")
