-- Laser triggers: level 1's three training lasers start
-- off; switched on they sweep back and forth along their rails; a beam that
-- meets Konoko trips its trigger once, until it misses her again; switched
-- off or hidden, no beams.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/lasers.lua

local function z_of(id)
    for _, b in ipairs(beams()) do
        if math.abs(b[2] - ({-310, -305, -300})[id]) < 0.5 then return b[3] end
    end
end

step(5)
-- Out of the way first: on the training floor, north of the rails.
teleport(0, -1060, -295, -470)
step(60)
local active = laser_state(1)
assert(active == false and #beams() == 0, "they start off")

for id = 1, 3 do laser(id, "activate") end
step(1)
assert(#beams() == 3, "three beams")
-- Active and showing, each hums (its class's ambient; at most three copies).
local function hums()
    local n = 0
    for _, s in ipairs(sounds()) do
        if s.name == "trigger_active" and (s.stage == "in" or s.stage == "body") then n = n + 1 end
    end
    return n
end
assert(hums() > 0, "the lasers hum")
local z0 = z_of(1)
step(100)
local z1 = z_of(1)
print(string.format("beam 1 swept from z %.1f to %.1f in 100 ticks", z0, z1))
assert(z1 - z0 > 12 and z1 - z0 < 18, "half a track frame a tick: ~15 units in 100 ticks")
step(150)
assert(z_of(1) < z1 + 15, "and back (ping-pong)")
local _, _, tripped = laser_state(1)
assert(not tripped, "nobody in the beam")

-- Konoko stands in the sweep: the low beams trip on her; the top one
-- passes over her head.
teleport(0, -1060, -315, -430)
local hit = {}
for _ = 1, 420 do
    step(1)
    for id = 1, 3 do
        local _, _, t = laser_state(id)
        if t then hit[id] = true end
    end
end
print("tripped:", hit[1], hit[2], hit[3])
assert(hit[1] and hit[2] and not hit[3], "the beams at her legs and chest, not over her head")

-- Off, hidden: no beams.
laser(-1, "hide")
step(1)
assert(#beams() == 0, "hidden")
assert(hums() == 0, "hidden, they are quiet")
laser(-1, "show")
laser(1, "deactivate")
step(1)
assert(#beams() == 2, "one switched off")
laser(-1, "deactivate") -- (-1 doesn't mean every laser here, as in the original)
step(1)
assert(#beams() == 2, "deactivate -1 does nothing")
print("lasers OK")
