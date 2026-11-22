-- Scripts steering characters (#54):
-- chr_create makes a level's character setup by script id at its flag;
-- begin_cutscene makes every AI passive and puts the player's gun away,
-- end_cutscene undoes both; ai2_takecontrol hands the player to the AI
-- movement system, which ai2_movetoflag walks to a flag and turns to its
-- facing, and ai2_lookatchar turns toward someone; ai2_makeblind keeps an AI
-- from seeing, ai2_forget wipes what it knew.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 134,1,-42 --script scenarios/script_ai.lua

mission_begin(false)
mission_exec([[
F.t_create = function() C.chr_create(1050, "start") C.chr_neutral(1050, 1) end
F.t_begin = function() C.begin_cutscene() end
F.t_end = function() C.end_cutscene() end
F.t_walk = function() C.ai2_takecontrol(1) C.ai2_movetoflag(0, 7000, "setfacing") end
F.t_lookat = function() C.ai2_lookatchar(0, 1050) end
F.t_release = function() C.ai2_takecontrol(0) end
F.t_blind = function() C.ai2_makeblind(1050, 1) end
F.t_unblind = function() C.ai2_makeblind(1050, 0) end
F.t_forget = function() C.ai2_forget(1050) end
]])
local function run(f) assert(mission_call(f), f) step(2) end
step(10)

-- chr_create: setup 1050 (a Muro) at its flag 0 (160, -1, -40), known by
-- its script id: chr_neutral 1050 makes it passive.
local before = count()
run("t_create")
assert(count() == before + 1, "a character was made")
local m = count() - 1
local mx, my, mz = pos_of(m)
print(string.format("made fighter %d (script id %s) at %.0f %.0f %.0f", m, tostring(script_id_of(m)), mx, my, mz))
assert(script_id_of(m) == 1050, "known by its setup's script id")
assert(math.abs(mx - 160) < 3 and math.abs(mz + 40) < 3, "at its flag")
assert(passive_of(m), "chr_neutral by script id reached it")

-- A cutscene: every AI passive, the gun put away; afterwards both undone
-- (the AI's own passivity too, as AI2rAllPassive(false) clears it).
give_weapon(0, "w1_tap")
step(30)
run("t_begin")
step(60)
print("in the cutscene: gun", tostring(weapon(0)), "Muro passive", tostring(passive_of(m)))
-- (Into the hidden slot, as a forced holster puts it.)
assert(weapon(0) == nil, "the gun put away for the cutscene")
assert(passive_of(m), "AIs passive in a cutscene")
run("t_end")
step(60)
print("after it: gun", tostring(weapon(0)), "Muro passive", tostring(passive_of(m)))
assert(weapon(0) == "w1_tap", "the gun drawn again")
assert(not passive_of(m), "AIs out of passivity")

-- The AI moves the player: to flag 7000 (77, -1, -48), facing its 90°.
run("t_walk")
local arrived
for t = 1, 600 do
  step(1)
  local x, _, z = pos()
  if math.abs(x - 77) < 4 and math.abs(z + 48) < 4 and math.abs(((facing_deg() - 90) + 180) % 360 - 180) < 8 then arrived = t break end
end
local x, _, z = pos()
print(string.format("walked to the flag: %s ticks, at %.0f %.0f facing %.0f", tostring(arrived), x, z, facing_deg()))
assert(arrived, "the AI walked her to the flag and turned her to its facing")

-- ... and turns her toward the Muro (from facing away).
facing(270)
step(5)
run("t_lookat")
step(120)
local px, _, pz = pos()
local want = math.deg(math.atan(mx - px, mz - pz)) % 360
print(string.format("looking at the Muro: facing %.0f, it lies at %.0f", facing_deg(), want))
assert(math.abs(((facing_deg() - want) + 180) % 360 - 180) < 10, "turned toward the Muro")
run("t_release")

-- Blind (and having forgotten her), it doesn't see her standing in front
-- of it; seeing again, it does; forgetting wipes that.
team(m, "syndicate")
teleport(0, mx, my + 2, mz + 30)
set_facing(m, 0)
run("t_blind")
run("t_forget")
step(240)
print("blind: knows of her:", tostring(contact_of(m, 0)))
assert(contact_of(m, 0) == nil, "a blind AI saw nothing")
run("t_unblind")
for _ = 1, 240 do step(1) if contact_of(m, 0) then break end end
print("seeing: knows of her:", tostring(contact_of(m, 0)))
assert(contact_of(m, 0) ~= nil, "seeing again, it noticed her")
run("t_blind")
run("t_forget")
print("forgot: knows of her:", tostring(contact_of(m, 0)))
assert(contact_of(m, 0) == nil, "ai2_forget wiped it")
print("ok")
