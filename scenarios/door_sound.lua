-- Doors and sound: a door that isn't open lets only what
-- its class allows through (the shipped doors: gunfire), and muffles it —
-- every step beyond counts twice. Open, it passes anything.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/door_sound.lua

-- Door 18 (closed) between Konoko (z -545) and a Striker (z -650), facing
-- away from the door.
step(5)
teleport(0, -82, 2, -545)
step(10)
local s = spawn(-82, 7, -650, 180, "striker_easy_1")
step(10)
watch(s)
step(30)
assert(door_state(18) == "closed")
local function heard(kind, volume)
    noise(-82, 2, -545, kind, volume, 0)
    step(3)
    return contact_of(s, 0) ~= nil
end
local through = heard("interesting", 150)
print("closed door, an interesting noise:", through)
assert(not through, "a closed door keeps it out")
local shot = heard("gunshot", 400)
print("closed door, a gunshot:", shot)
assert(shot, "gunfire gets through")

-- Opened: anything gets through.
local s2 = spawn(-75, 7, -650, 180, "striker_easy_1")
step(10)
watch(s2)
door(18, "open")
step(10)
noise(-82, 2, -545, "interesting", 150, 0)
step(3)
print("open door, an interesting noise:", contact_of(s2, 0) ~= nil)
assert(contact_of(s2, 0) ~= nil, "an open door lets it through")
print("door sound ok")
