-- Placement flags (#64): a character placed
-- with a random costume takes one of the classes of its class's variant; one
-- that upgrades with the difficulty takes, on hard, a class of the variant
-- its own upgrades to (and below hard a random one of its own) — its melee
-- profile the new class's default if it had its old class's. And the level
-- scripts' `difficulty` is the game's.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/costumes.lua

mission_begin(false)
mission_exec([[
F.thug = function() C.ai2_spawn("Bay2_Thug_2", "force") end
F.striker = function() C.ai2_spawn("Ambush_Striker_1", "force") end
F.what = function() if C.difficulty() == 2 then C.objective_set(7) end end
]])

-- Spawns `fn` n times; the classes they took.
local function classes(fn, n)
    local seen, all = {}, {}
    for _ = 1, n do
        local before = count()
        assert(mission_call(fn))
        step(1)
        local c = class_of(before)
        seen[c] = (seen[c] or 0) + 1
        all[#all + 1] = c
    end
    return seen, all
end

-- A warehouse thug (placed as thug_wh_1, random costume): the thug_wh classes.
difficulty("normal")
local seen = classes("thug", 12)
local kinds = 0
for c in pairs(seen) do
    assert(c:match("^thug_wh_%d$"), "a thug of its variant: " .. c)
    kinds = kinds + 1
end
print("random costumes:", kinds, "classes over 12")
assert(kinds >= 2, "costumes vary")

-- An ambush Striker (striker_easy_1, upgrades): on normal a Striker of its
-- own variant; on hard one of the medium Strikers.
seen = classes("striker", 6)
for c in pairs(seen) do
    assert(c:match("^striker_easy_%d$"), "normal: its own variant, " .. c)
end
difficulty("hard")
seen = classes("striker", 6)
for c in pairs(seen) do
    assert(c:match("^striker_med_%d$"), "hard: the upgraded variant, " .. c)
end

-- The scripts see the game's difficulty.
assert(mission_call("what"))
step(2)
assert(mission_info().objective == 7, "difficulty() on hard is 2")
print("ok")
