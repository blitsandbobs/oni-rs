-- Hit reactions on a training dummy with its back to the player: a behind
-- hit, a combo that knocks it down, getting back up, and death.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/reactions.lua

local HIT_HEAD_BEHIND, HIT_BODY_BEHIND = 85, 86
local STANDING, FALLEN_BACK, FALLEN_FRONT = 7, 18, 50

local function show(label, d)
    print(string.format("%-18s dummy type=%3d state=%2d hp=%3d dead=%s",
        label, type_of(d), state_of(d), hp(d), tostring(dead(d))))
end
local function down(d) return state_of(d) == FALLEN_BACK or state_of(d) == FALLEN_FRONT end

step(20)
local px, py, pz = pos()
local d = spawn(px, py + 5, pz + 6, 0) -- its back to the player
step(60)
facing(0)

-- A punch from behind lands (no block from behind) with the behind reaction.
local hp0 = hp(d)
punch()
local reacted = false
for _ = 1, 20 do
    step(1)
    local t = type_of(d)
    if t == HIT_HEAD_BEHIND or t == HIT_BODY_BEHIND then reacted = true end
end
show("behind punch", d)
assert(hp(d) < hp0, "the punch from behind hurt (" .. hp0 .. " -> " .. hp(d) .. ")")
assert(reacted, "the dummy played a from-behind hit reaction")

-- Close in on the dummy, then a plain punch. (Punching with forward held
-- would be a forward punch, which turns into a throw within reach.)
local function gap()
    local x, _, z = pos()
    local dx, _, dz = pos_of(d)
    return math.sqrt((dx - x)^2 + (dz - z)^2)
end
local function close_in_and_punch()
    local x, _, z = pos()
    local dx, _, dz = pos_of(d)
    set_facing(0, math.deg(math.atan(dx - x, dz - z)))
    if gap() > 8.5 then
        input(true, false); step(2)
    else
        input(false, false); punch(); step(2)
    end
end

-- Chase it with a punch combo until it goes down (the third punch knocks down).
step(60)
local floored = false
for _ = 1, 120 do
    close_in_and_punch()
    if down(d) then floored = true; break end
end
input(false, false)
show("after combo", d)
assert(floored, "the combo knocked the dummy down")

-- A knocked-down character gets up on input. The dummy stands in for the AI's
-- daze timer: after lying still briefly it presses forward and rolls up.
local up_after = nil
for i = 1, 400 do
    step(1)
    if state_of(d) == STANDING then up_after = i; break end
end
show("recovered", d)
assert(up_after, "the dummy got back up")
print("stood up after " .. up_after .. " ticks")

-- The player gets up the same way: knocked down, then forward pressed.
knock(0, 79) -- a frontal head knockdown
for _ = 1, 90 do step(1) end
print(string.format("player down: type=%d state=%d", anim_type(), state()))
assert(state() == FALLEN_BACK or state() == FALLEN_FRONT, "the player is down")
input(true, false); step(4); input(false, false)
local stood = false
for _ = 1, 200 do step(1); if state() == STANDING then stood = true; break end end
print(string.format("player after forward: type=%d state=%d", anim_type(), state()))
assert(stood, "pressing forward gets the player up")
facing(0)

-- A killing blow: it dies, goes down, and stays down.
step(60)
set_hp(d, 1)
local died = false
for _ = 1, 120 do
    close_in_and_punch()
    if dead(d) then died = true; break end
end
input(false, false)
step(400)
show("dead", d)
assert(died and hp(d) == 0, "the killing blow killed it")
assert(down(d), "the body lies on the ground")
print("reactions ok: behind hit, knockdown, get up, death")
