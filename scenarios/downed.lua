-- A downed character doesn't turn (ONrCharacter_IsUnableToTurn): knocked
-- down, a Striker fighting Konoko lies as it fell while she walks round it,
-- whatever its AI wants to face.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/downed.lua

local FALLEN_BACK, FALLEN_FRONT = 18, 50
local function down(id) local s = state_of(id); return s == FALLEN_BACK or s == FALLEN_FRONT end
local function wrap(d) return (d + 180) % 360 - 180 end

local px, py, pz = pos()
local s = spawn(px, py + 5, pz + 25, 180, "striker_easy_1")
step(20)
spar(s, 0)
step(30)
knock(s, 79)
local lying
for t = 1, 60 do
    step(1)
    if down(s) then lying = t break end
end
assert(lying, "knocked down")
local f0 = facing_of(s)

-- Konoko walks round it (teleported a quarter-turn at a time) while it lies.
local most = 0
local _, _, sz = pos_of(s)
local sx = select(1, pos_of(s))
for k = 1, 4 do
    local a = math.rad(90 * k)
    teleport(0, sx + 20 * math.sin(a), py, sz + 20 * math.cos(a))
    for t = 1, 15 do
        step(1)
        if down(s) then most = math.max(most, math.abs(wrap(facing_of(s) - f0))) end
    end
end
print(string.format("lying, fighting: turned at most %.1f degrees", most))
assert(most < 1, "a downed character doesn't turn")

-- Ordered to face elsewhere (the AI's movement orders: what turns a
-- shooting AI toward its target) while it still lies: it doesn't.
if down(s) then
    local before = facing_of(s)
    ai_order(s, "stop", nil, before + 120)
    step(10)
    local turned = math.abs(wrap(facing_of(s) - before))
    print(string.format("lying, ordered to turn: turned %.1f degrees", turned))
    assert(turned < 1, "orders don't turn a downed body")
end
print("ok")
