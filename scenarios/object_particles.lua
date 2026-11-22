-- A placed object's own particles (#45;, *Attached
-- particles*): level 8's opening makes the black van, and its headlights
-- come with it — tagged after it (the script starts them by that name) and
-- riding it as it drives in. And a gun's sight stops on it (an object met
-- by its sphere, `AMrRayToObject`).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/08/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/object_particles.lua

mission_begin(true)
local function lights()
    return particles_of("headlightStart")
end
local t = 0
repeat
    step(1)
    t = t + 1
until #lights() > 0 or t > 3000
assert(#lights() == 2, "the van's two headlights, got " .. #lights())
assert(env_particles("object_Blackvan_FB_VanHeadlight") == 2, "tagged after the van")
local a = lights()[1]
print("headlight at", a[1], a[2], a[3])
-- Not at the van's own origin's offset in the world (near 0, 0, 0).
assert(math.abs(a[1]) + math.abs(a[3]) > 100, "placed on the van")
-- The van drives off once the opening's cameras have run.
local function van_frame()
    for _, o in ipairs(objects()) do
        if o.name == "object_Blackvan_FB" and o.animating then return o.frame end
    end
    return 0
end
t = 0
repeat
    step(1)
    t = t + 1
until van_frame() > 150 or t > 3000
local b = lights()[1]
local moved = math.sqrt((b[1] - a[1]) ^ 2 + (b[3] - a[3]) ^ 2)
print("van driving", b[1], b[2], b[3], "moved", moved)
assert(moved > 20, "riding the van as it drives")

-- Once it has stopped: Konoko's sight, 60 off and aimed at it, meets it.
local function van_at()
    for _, o in ipairs(objects()) do
        if o.name == "object_Blackvan_FB" then return o.at, o.animating end
    end
end
t = 0
repeat
    step(1)
    t = t + 1
    local _, moving = van_at()
until not moving or t > 3000
local at = van_at()
local _, ky = pos()
teleport(0, at[1] - 60, ky + 5, at[3])
facing(90)
assert(give_weapon(0, "w1_tap"))
step(40)
local s = sight(0)
print("sight", s and s.target, s and s.to[1])
assert(s and s.target == "object", "the sight meets the van")
print("ok")
