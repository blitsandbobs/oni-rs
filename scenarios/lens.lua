-- Lens flares: the doors' lock lights,
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

-- The option (#58): characters hide the lens flares behind them. A dummy's
-- head over part of the lock light's glow, its centre still seen (the
-- flare shown): as shipped the glow is drawn over the head; with the option
-- the head covers it — the glow's region darker. Clear of the glow, the
-- same either way.
teleport(0, -251, 2, -623)
set_facing(0, 150)
local function glow(on)
    flares_behind_characters(on)
    local p = temp_path("lens_glow.png")
    screenshot(p, 480, 360)
    local r, g, b = image_mean(p, 0.6, 0.25, 0.73, 0.42)
    os.remove(p)
    flares_behind_characters(false)
    return r + g + b
end
local dummy = spawn(-251 + 30 + 5.5, 2, -623 - 29.4, 0)
step(40)
teleport(0, -251, 2, -623)
set_facing(0, 150)
step(150) -- (the flare fades in)
assert(lens_flare_near(-201, 13, -672) > 0.9, "the light's centre still seen")
local shipped, hidden = glow(false), glow(true)
print(string.format("glow over the head: %.5f, behind it: %.5f", shipped, hidden))
assert(hidden < shipped - 0.0005, "the head covers the glow with the option")
teleport(dummy, -251 + 30 + 8, 2, -623 - 29.4)
step(40)
teleport(0, -251, 2, -623)
set_facing(0, 150)
step(150)
assert(math.abs(glow(false) - glow(true)) < 1e-6, "clear of the glow: the same")
print("lens flares behind characters ok")
