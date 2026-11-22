-- The arm follows the gun (*Auto-aim*): aiming
-- to either side, her arm's aim pose turns as far as the gun does (the aiming
-- screen's turns go on in the original's order, so a turn right isn't lost);
-- near an enemy the auto-aim turns the gun onto it and the arm with it (the
-- original's aiming screen takes the auto-aim's turn too). Either way the gun
-- doesn't turn in her hand.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/auto_aim.lua

local function yaw(v) return math.deg(math.atan(v[1], v[3])) end
local function gap() local h, g = bone(0, 18), held_gun(0); return yaw(g.x) - yaw(h.x) end
facing(0)
give_weapon(0, "w1_tap")
step(40)
local base = gap()

-- Aim to each side without turning the body: the hand keeps up with the gun.
local h0, g0 = yaw(bone(0, 18).x), yaw(held_gun(0).x)
for _, d in ipairs({-60, -30, 30, 60}) do
  facing(0)
  turn(d)
  step(20)
  local hand, gun = yaw(bone(0, 18).x) - h0, yaw(held_gun(0).x) - g0
  print(string.format("aim %3d: hand %6.1f, gun %6.1f", d, hand, gun))
  assert(math.abs(gun - hand) < 15, "the hand turned with the gun")
end
facing(0)
turn(0)
step(30)

-- An enemy 60 ahead, 4 degrees off her aim.
local px, py, pz = pos()
local a = math.rad(4)
local e = spawn(px + 60 * math.sin(a), py + 5, pz + 60 * math.cos(a), 180, "striker_easy_1")
team(e, "syndicate")
step(40)
local s = sight(0)
local off = gap() - base
print(string.format("sight on: %s; the gun turned off her hand by %.1f degrees", s and s.target or "nothing", off))
assert(s and s.target == "character", "the auto-aim puts the sight on the enemy")
assert(math.abs(off) < 3, "her arm turned with the gun")
print("ok")
