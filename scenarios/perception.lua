-- Perception (issue #3): an AI sees with its class's vision cones (far
-- ahead, near aside, not behind), hears noises within range, remembers
-- what it perceived (fading with time), raises its alert accordingly — and a
-- definite sighting of an enemy starts a fight.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/perception.lua

step(20)
local px, py, pz = pos()

-- A Striker facing away from Konoko, 40 units ahead of her: it can't see her.
local s = spawn(px, py + 5, pz + 40, 0, "striker_easy_1")
step(10)
watch(s)
for _ = 1, 120 do step(1) end
print(string.format("behind its back: contact %s, alert %s", tostring(contact_of(s, 0)), alert_of(s)))
assert(contact_of(s, 0) == nil, "it doesn't see behind it")

-- A taunt (an interesting noise) right behind it: it hears, glances, alerts.
noise(px, py, pz, "interesting", 150, 0)
step(5)
print(string.format("after a noise behind it: contact %s, alert %s", tostring(contact_of(s, 0)), alert_of(s)))
assert(contact_of(s, 0) == "strong", "it heard Konoko")
assert(alert_of(s) == "low", "an interesting noise: low alert")

-- It turns to glance at the noise — and sees her: definite, combat, a fight.
local fought = false
for _ = 1, 300 do
    step(1)
    if alert_of(s) == "combat" then fought = true; break end
end
print(string.format("after glancing: contact %s, alert %s", tostring(contact_of(s, 0)), alert_of(s)))
assert(fought and contact_of(s, 0) == "definite", "it saw her and went to combat")

-- A Striker far off to the side (peripheral range is only 150) doesn't see her.
local far = spawn(px + 120, py + 5, pz + 160, 0, "striker_easy_1")
step(10)
watch(far)
for _ = 1, 120 do step(1) end
print(string.format("far off to the side: contact %s", tostring(contact_of(far, 0))))
assert(contact_of(far, 0) == nil, "out of its cones")
print("perception ok: cones, hearing, glancing, alert, combat")
