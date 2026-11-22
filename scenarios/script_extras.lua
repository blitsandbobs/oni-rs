-- The last of the level scripts' engine commands (#54; docs/lua.md): the frame clock a
-- cutscene
-- keeps up with (`cutscene_sync`), slow motion (`slowmo`), level parts and
-- objects lit flat (`env_shade`, `obj_shade`), whether Griffin was killed,
-- a Daodan power (`chr_super`) with its super particles, a boss shield, an
-- AI coming to the player (`ai2_followme`), Barabbas taking his gun back and
-- an AI watching her as she moves (`ai2_lookatme`).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/script_extras.lua

mission_begin(false)
mission_exec([[
F.cut = function() C.begin_cutscene() end
F.mark = function() C.cutscene_sync("mark") end
F.uncut = function() C.end_cutscene() end
F.slow = function() C.slowmo(30) end
F.shade = function() C.env_shade(3001, 3003, 1, 0.5, 0) end
F.objs = function() C.obj_create(101, 109) C.obj_shade(101, 102, 0.2, 0.4, 0.6) C.obj_force_draw(101) end
F.griffin = function() C.killed_griffen(1) if C.did_kill_griffen() == 1 then C.objective_set(9) end end
F.super = function() C.chr_super(0, 1) end
F.unsuper = function() C.chr_super(0, 0) end
F.boss = function() C.chr_boss_shield("Bay2_Thug_2") C.ai2_passive("Bay2_Thug_2", 1) end
F.follow = function() C.ai2_passive("Bay2_Thug_2", 0) C.ai2_followme("Bay2_Thug_2") end
F.look = function() C.ai2_passive("Mid_Thug_1", 1) C.ai2_lookatme("Mid_Thug_1") end
F.gun = function() C.ai2_barabbas_retrievegun("Mid_Thug_1") end
F.gun2 = function() C.ai2_barabbas_retrievegun("Bay2_Thug_2") end
]])
local function run(f)
    assert(mission_call(f))
    step(1)
end

-- The frame clock: at most 6 ticks a frame; outside a cutscene the rest is
-- dropped, in one it is owed to the next frame until a sync mark.
assert(frame_ticks(10) == 6 and frame_ticks(0) == 0, "outside: dropped")
run("cut")
assert(frame_ticks(10) == 6 and frame_ticks(0) == 4, "in a cutscene: owed, then caught up")
assert(frame_ticks(10) == 6)
run("mark")
assert(frame_ticks(0) == 0, "a mark clears what is owed")
run("uncut")

-- Slow motion: half speed for its ticks.
run("slow")
assert(time_scale() == 2)
step(40)
assert(time_scale() == 1, "over")

-- Lit flat: level parts by reference, objects by id.
run("shade")
local c = env_shade_of(3001)
assert(c and c[1] == 1 and math.abs(c[2] - 128 / 255) < 1e-6 and c[3] == 0, "3001 shaded (8 bits)")
assert(env_shade_of(3004) == nil, "3004 not")
run("objs")
local shades, shaded = obj_shades(), 0
for _, s in ipairs(shades) do
    if s then shaded = shaded + 1 end
end
assert(#shades == 9 and shaded == 2, "objects 101-102 of 9 lit flat")

-- Griffin: what the scripts are told, they are told back.
run("griffin")
assert(mission_info().objective == 9, "did_kill_griffen after killed_griffen 1")

-- Konoko's Daodan power: up 0.03 a tick to 1, its three super particles.
run("super")
step(40)
local e = effects(0)
assert(e.super == 1 and e.super_particles == 3, "super, with its particles")
assert(#particles_of("superglow_e01") == 1, "the glow on her pelvis")
run("unsuper")
step(20)
assert(effects(0).super == 0, "down 0.06 a tick")

-- A thug with a boss shield: shot, it flares red where struck.
local px, py, pz = pos()
local t = place("Bay2_Thug_2")
run("boss")
teleport(t, px, py + 5, pz + 30)
step(30)
assert(effects(t).boss_shield)
assert(give_weapon(0, "w1_tap"))
facing(0)
aim(0, -0.1)
step(15)
fire(1)
local flared = false
for _ = 1, 30 do
    step(1)
    for _, p in ipairs(effects(t).shield_parts) do
        if p > 200 then flared = true end
    end
    if flared then break end
end
assert(flared, "the boss shield flared")
screenshot("/dev/null")
assert(magic_drawn() >= 1, "drawn")

-- Told to follow Konoko, it walks up to her (within 12 of where she stood).
teleport(t, px + 80, py + 5, pz)
step(20)
run("follow")
local near = false
for _ = 1, 400 do
    step(1)
    local x, _, z = pos_of(t)
    if math.sqrt((x - px) ^ 2 + (z - pz) ^ 2) < 16 then near = true break end
end
assert(near, "it came to her")

-- Barabbas's gun (the unreloadable one) back in the hand of one without a
-- gun (one holding a gun keeps it).
run("gun2")
assert(weapon(t) ~= "w11_ba1", "a thug with a gun keeps his")
local m = place("Mid_Thug_1")
spawn_weapon("w11_ba1", px + 60, py + 2, pz + 60)
step(5)
run("gun")
assert(weapon(m) == "w11_ba1", "the gun in its hand")

-- Told to look at Konoko (held passive, so no fight), its eyes keep
-- following her as she moves.
local mx, my, mz = pos_of(m)
local function bearing_to_her()
    local x, _, z = pos()
    return math.deg(math.atan(x - mx, z - mz)) % 360
end
local function off(a, b) return math.abs((a - b + 540) % 360 - 180) end
teleport(0, mx + 30, my, mz)
step(5)
run("look")
step(20)
local before = eye_of(m) or facing_of(m)
assert(off(before, bearing_to_her()) < 15, "it looks at her")
teleport(0, mx, my, mz + 30)
step(40)
local after = eye_of(m) or facing_of(m)
print(string.format("looking at her: %.0f°, then %.0f° once she moved (toward her: %.0f°)",
    before, after, bearing_to_her()))
assert(off(after, bearing_to_her()) < 15, "and follows her with its eyes")
print("ok")
