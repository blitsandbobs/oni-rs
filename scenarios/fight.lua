-- Melee hit detection: the player spawns a training dummy in front, faces it,
-- and punches. The dummy takes damage and is knocked back. Exercises the whole
-- chain: Lua spawn → multi-fighter world → attack damage window → hit
-- resolution (reach + front test) → HP + knockback.
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/fight.lua

-- Settle the player standing.
step(20)
local px, py, pz = pos()

-- Spawn a dummy a few units ahead (+Z), facing back at the player, and let it
-- settle onto the floor. Fighter 0 is the player; spawn returns the new id.
local dummy = spawn(px, py + 5, pz + 6, 180)
assert(count() == 2, "player + dummy")
step(60)

-- Face the player at the dummy (it is at +Z, i.e. yaw 0) and punch.
facing(0)
local hp0 = hp(dummy)
local _, _, dz0 = pos_of(dummy)
punch()
for _ = 1, 40 do step(1) end
local hp1 = hp(dummy)
local _, _, dz1 = pos_of(dummy)

print(string.format(
    "dummy hp %d -> %d, z %.2f -> %.2f, player clip %s",
    hp0, hp1, dz0, dz1, anim()))

assert(hp0 == 100, "dummy starts at full hp, got " .. hp0)
assert(hp1 < hp0, "the punch damaged the dummy (" .. hp0 .. " -> " .. hp1 .. ")")
assert(dz1 > dz0 - 0.01, "the dummy was knocked back (not pulled in)")

-- A second, out-of-range dummy is never hit by the same swing distance.
local far = spawn(px, py + 5, pz + 40)
step(60)
facing(0)
local far0 = hp(far)
punch()
for _ = 1, 40 do step(1) end
assert(hp(far) == far0, "a far dummy is out of reach, got " .. hp(far))

print("fight ok: melee connects in reach, misses out of reach")
