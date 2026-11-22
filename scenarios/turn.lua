-- Turning: the body does not snap to the mouse target — it slews toward it at
-- a capped rate (Oni's HandleTurning), and a large pending turn plays a
-- turn-in-place clip (feet re-plant) past the 70-deg dead-zone.
--
--   cargo run --release -p oni -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/turn.lua

-- Settle standing, facing 0. Request a 120-deg turn (mouse flick).
step(30)
facing(0)
turn(120)
assert(math.abs(desired_deg() - 120) < 1.0, "target set to 120, got " .. desired_deg())

-- One tick slews only a few degrees (medTurn ~4.5 deg/tick), NOT a snap.
step(1)
local after1 = facing_deg()
print(string.format("after 1 tick: facing=%.1f (target 120)  anim=%s", after1, anim()))
assert(after1 > 1.0 and after1 < 10.0,
    "body slews a few degrees, not a snap, got " .. after1)

-- While a big turn is pending, a Standing_Turn clip plays (feet re-plant).
local saw_turn = anim():find("turn") ~= nil
for _ = 1, 40 do
    step(1)
    if anim():find("turn") ~= nil then saw_turn = true end
end
assert(saw_turn, "a turn-in-place clip plays during the big turn")

-- The body reaches the target and settles (within a degree or two).
assert(math.abs(facing_deg() - 120) < 3.0,
    "body reaches the target, got " .. facing_deg())
print(string.format("settled facing=%.1f  anim=%s", facing_deg(), anim()))

cam(214, 8, 300,  200, 8, 300)
screenshot("/tmp/oni_turn.png", 700, 1000)
