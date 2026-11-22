-- Hearing what bodies do (#13): footfalls
-- and blows are noises AIs hear — a run carries 80, a sprint 100 (and is
-- "interesting"), a crouched walk nothing, a landed blow 150+ ("melee").
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/hearing.lua

step(20)
local px, py, pz = pos()
-- A watcher 40 units ahead, its back to Konoko: it can only hear her.
local w = spawn(px, py + 5, pz + 40, 0, "striker_easy_1")
step(30)
watch(w)
step(60)
assert(contact_of(w, 0) == nil, "nothing yet")

-- Creeping toward it (crouched first — crouch and forward together from
-- standing is a roll, which is heard): silent.
facing(0)
crouch(true)
step(40)
input(true, false)
step(60)
input(false, false)
crouch(false)
step(30)
print("after creeping:", tostring(contact_of(w, 0)), alert_of(w))
assert(contact_of(w, 0) == nil, "a crouched walk makes no noise")

-- Running about: it hears the footfalls (faint ones: no alarm).
input(true, false)
step(20)
input(false, false)
step(10)
facing(180)
input(true, false)
step(40)
input(false, false)
step(20)
print("after running:", tostring(contact_of(w, 0)), alert_of(w))
assert(contact_of(w, 0) ~= nil, "it heard her run")
-- A fight: Konoko punches a dummy beside her — a "melee" noise, which
-- alarms the watcher (medium).
local x, y, z = pos()
local d = spawn(x, y + 5, z + 6, 0)
step(40)
facing(0)
punch()
step(30)
print("after a blow:", tostring(contact_of(w, 0)), alert_of(w))
assert(alert_of(w) == "medium", "a landed blow is heard as a fight")
print("hearing OK")
