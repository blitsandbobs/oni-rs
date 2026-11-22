-- The level's sound objects: level 1's
-- placed ambience. Walking into a box starts its ambient with a 2-second
-- ease-in and walking out eases it out and halts it; standing by a sphere
-- plays its ambient placed there, which plays out once we leave.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ambience.lua

step(5)
local objs = sound_objects()
assert(#objs > 100, "level 1 places its ambience: " .. #objs)
local function heard(name)
    for _, s in ipairs(sounds()) do
        if s.name == name then return s end
    end
end
local far = { 5000, 5000, 5000 }
local function away() teleport(0, far[1], far[2], far[3]) step(1) end

-- A box: stand at its middle (the first that takes us in).
away()
step(200)
local b
for i, o in ipairs(objs) do
    if o.box then
        teleport(0, o.x, o.y, o.z)
        step(1)
        if sound_objects()[i].state == "Starting" then b = i break end
        away()
        step(130)
    end
end
assert(b, "a box to stand in")
local o = sound_objects()[b]
print("box", b, o.ambient, o.state)
step(59)
o = sound_objects()[b]
print("after a second:", o.state, o.volume, heard(o.ambient) and heard(o.ambient).stage)
assert(o.volume > 0.6 and o.volume < 0.9, "an ease-in, about three quarters up after a second")
assert(heard(o.ambient), "its ambient plays")
step(70)
assert(sound_objects()[b].state == "Playing" and sound_objects()[b].volume == 1, "fully in after two seconds")
away()
step(60)
o = sound_objects()[b]
assert(o.state == "Stopping" and o.volume < 1 and o.volume > 0, "easing out")
step(70)
assert(sound_objects()[b].state == "Off" and not sound_objects()[b].playing, "out and halted")

-- A sphere: stand by it.
local s
for i, o in ipairs(objs) do
    if not o.box then
        teleport(0, o.x + 3, o.y, o.z)
        step(2)
        if sound_objects()[i].playing then s = i break end
        away()
        step(5)
    end
end
assert(s, "a sphere to stand by")
step(30)
o = sound_objects()[s]
print("sphere", s, o.ambient, "volume", o.volume, heard(o.ambient) and heard(o.ambient).stage)
assert(o.volume > 0, "heard, faded up")
away()
local h = heard(o.ambient)
assert(not h or h.stage == "stopping" or h.stage == "out", "left: it plays out")
print("ambience ok")
