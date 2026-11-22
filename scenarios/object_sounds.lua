-- Objects' and the game's sounds:
-- level 1's door 18 opening plays its class's sound; picking up ammo plays
-- the receive sound; low health starts the heartbeat loop, healing ends it.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/object_sounds.lua

local clips = {}
local function listen(n)
    for _ = 1, n do
        step(1)
        for _, c in ipairs(channels()) do
            if c.clip then clips[c.clip] = true end
        end
    end
end
local function any(pattern)
    for clip in pairs(clips) do
        if clip:match(pattern) then return clip end
    end
end
local function playing(name)
    for _, s in ipairs(sounds()) do
        if s.name == name and (s.stage == "in" or s.stage == "body") then return true end
    end
end

-- The door: walk up to it.
step(5)
teleport(0, -82, 2, -545)
facing(180)
step(30)
input(true, false)
for _ = 1, 120 do
    listen(1)
    if door_state(18) ~= "closed" then break end
end
input(false, false)
listen(10)
print("door:", door_state(18), any("^door") or "-")
assert(any("^door"), "the door sounds as it opens")

-- A clip of ammo, the game's way.
clips = {}
assert(powerup(0, "ammo", 1) > 0, "she takes it")
listen(5)
print("receive:", any("^receive_ammo") or "-")
assert(any("^receive_ammo"), "the pickup sounds")

-- Low health: the condition loop; healed, it ends.
set_hp(0, 20)
listen(5)
assert(playing("health_low"), "low health loops")
set_hp(0, 200)
listen(5)
assert(not playing("health_low"), "healed, it stops")
print("object sounds ok")
