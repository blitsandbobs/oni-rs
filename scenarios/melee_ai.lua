-- The melee AI (issue #5): a character fights with its class's melee
-- profile — it picks techniques (run in and combo, kicks, throws…), lands
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
local a = duel("striker_easy_1", -40)

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
print("melee AI ok: profile techniques land hits; daze then get up")
