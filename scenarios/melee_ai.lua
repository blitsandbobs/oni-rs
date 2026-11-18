-- The melee AI (issue #5): a character fights with its class's melee
-- profile — it picks techniques (run in and combo, kicks, throws, flying
-- kicks…), lands
-- hits on a training dummy, and after a knockdown lies dazed for its class's
-- daze time before getting up by itself.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/melee_ai.lua

local FALLEN_BACK, FALLEN_FRONT = 18, 50
local function down(id) local s = state_of(id); return s == FALLEN_BACK or s == FALLEN_FRONT end

step(20)
local px, py, pz = pos()

local function duel(class, x)
    -- A passive dummy, its back to an AI fighter starting 20 units behind it.
    local d = spawn(px + x, py + 5, pz + 4, 0, class)
    local a = spawn(px + x, py + 5, pz - 16, 0, class)
    step(30)
    spar(a, d)
    local hp0 = hp(d)
    for _ = 1, 600 do
        step(1)
        if hp(d) <= hp0 - 10 then break end
    end
    print(string.format("%s AI vs dummy: dummy hp %d -> %d", class, hp0, hp(d)))
    assert(hp(d) <= hp0 - 10, class .. " AI landed several hits")
    return a, d
end

duel("konoko_generic", 40)
local a = duel("striker_easy_1", 15)

-- Knocked down, the Striker lies dazed (its class: 96-138 ticks) then gets up.
step(120)
knock(a, 79)
local t_down, t_up = nil, nil
for t = 1, 600 do
    step(1)
    if not t_down and down(a) then t_down = t end
    if t_down and not down(a) then t_up = t; break end
end
print(string.format("striker AI knocked down at t=%s, up at t=%s", tostring(t_down), tostring(t_up)))
assert(t_down and t_up, "the AI got back up by itself")
assert(t_up - t_down >= 96, "it lay dazed for at least its class's minimum")
-- A Ninja's flying kick: it runs in, jumps, and kicks in the air when the
-- kick will land (Lua `technique` asks for a profile technique by name).
-- (Along the room's long side: a run-jump flies ~58 units, and the AI won't
-- jump at a wall.)
local nd = spawn(px + 15, py + 5, pz + 10, 90, "ninja_med_1")
local na = spawn(px - 20, py + 5, pz + 10, 90, "ninja_med_1")
step(20)
spar(na, nd)
technique(na, "Jump Kick/Circle Rt")
local hp1, flew = hp(nd), false
for _ = 1, 200 do
    step(1)
    if type_of(na) == 12 and anim_of(na):find("jump") then flew = true end
    if flew and hp(nd) < hp1 then break end
end
print(string.format("ninja flying kick: kicked in the air=%s, dummy hp %d -> %d", tostring(flew), hp1, hp(nd)))
assert(flew, "the ninja kicked in mid-air")
assert(hp(nd) < hp1, "the flying kick landed")
-- Difficulty scales the AI's chances to notice, dodge and block (the game
-- settings' multipliers; normal is 1).
local n, d, b = difficulty("hard")
print(string.format("hard: notice x%.1f dodge x%.1f block x%.1f", n, d, b))
assert(n > 1 and d > 1 and b > 1, "hard sharpens the AI")
n, d, b = difficulty("easy")
assert(n < 1 and d < 1 and b < 1, "easy dulls it")
difficulty("normal")
print("melee AI ok: profile techniques land hits; flying kicks; daze then get up; difficulty")
