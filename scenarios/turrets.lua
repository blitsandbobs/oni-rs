-- Turrets (#19; docs/TURRETS.md): level 2's pistol wall turret (script id
-- 3), switched on, turns toward the player, fires while its barrel is on her
-- and hurts her; "deactivate every turret" (−1) switches none, as shipped;
-- after its time (900 ticks) it goes back to rest and off.
--
--   cargo run --release -p oni-view -- \
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
step(900)
t = turret_state(3)
print("after its time: active", t.active, "yaw", t.yaw, "pitch", t.pitch)
assert(not t.active, "timed out")
print("turrets ok")
