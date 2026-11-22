-- A turret's idle effect follows the view (`OBJiTurret_Update`):
-- on while it is drawn, off once it has been out of view for 2 s, on again
-- when seen. Level 2's wall turret 3, west of Konoko.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/02/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos -540,70,9 --script scenarios/turret_idle.lua

-- Facing the turret (west), then away (east), with the camera behind her.
facing(270)
step(30)
assert(turret_state(3).idle_effect, "seen: its idle effect is on")
facing(90)
step(60)
assert(turret_state(3).idle_effect, "out of view for 1 s: still on")
step(90)
assert(not turret_state(3).idle_effect, "out of view for over 2 s: off")
facing(270)
step(30)
assert(turret_state(3).idle_effect, "seen again: back on")
print("ok")
