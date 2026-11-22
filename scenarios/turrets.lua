-- Turrets (#19): level 2's pistol wall turret (script id
-- 3), switched on, hums (its in sound, then the loop:), turns
-- toward the player, fires while its barrel is on her and hurts her;
-- "deactivate every turret" (−1) switches none, as shipped; after its time
-- (900 ticks) it goes back to rest and off, and its hum plays out.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos -540,70,9 --script scenarios/turrets.lua

step(60)
local t = turret_state(3)
assert(t and not t.active, "turret 3 is there, off")
turret(3, "activate")
step(1)
assert(turret_state(3).active, "switched on")
local function hum()
    for _, s in ipairs(sounds()) do
        if s.name == "turret_active" then return s end
    end
end
local s = hum()
assert(s and s.stage == "in" and s.clip == "turret_active_in_aif", "the hum starts with its in sound")
-- (The turret is ~19 ft off to her right: heard, louder on the right.)
assert(s.left > 0 and s.right > s.left, "heard, panned right: " .. s.left .. " / " .. s.right)
step(60)
s = hum()
assert(s and s.stage == "body" and s.clip == "turret_active_aif", "then its loop")
local hp0 = hp(0)
step(240)
t = turret_state(3)
print("turret 3: target", t.target, "yaw", t.yaw, "pitch", t.pitch, "shots", t.shots, "player hp", hp0, "→", hp(0))
assert(t.target == 0, "it targets the player")
assert(t.shots > 0, "it fires")
assert(hp(0) < hp0, "its shots land")
turret(-1, "deactivate")
step(1)
assert(turret_state(3).active, "deactivating 'every turret' switches none (as shipped)")
for _ = 1, 900 do
    step(1)
    if not turret_state(3).active then break end
end
t = turret_state(3)
print("after its time: active", t.active, "yaw", t.yaw, "pitch", t.pitch)
assert(not t.active, "timed out")
s = hum()
assert(s and (s.stage == "stopping" or s.stage == "out"), "the hum plays out")
local out = false
for _ = 1, 150 do
    step(1)
    s = hum()
    out = out or (s and s.clip == "turret_active_out_aif")
    if not s then break end
end
assert(out and not hum(), "its out sound, then silence")
print("turrets ok")
