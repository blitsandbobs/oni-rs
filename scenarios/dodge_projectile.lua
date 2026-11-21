-- Q24, projectile dodging (docs/QUIRKS.md; docs/AI.md, *Dodging*): a Super
-- Ball fired past an armed soldier (22 to its side: inside the ball's dodge
-- radius, clear of its body) is, as with the Daodan DLL's `projaware`, a
-- direct hit at full weight: the soldier sidesteps, square to the ball's path
-- (fixed). The shipped game measures the ball's closest approach from the
-- world's origin, so the danger never weighs enough: the soldier stands its
-- ground (--faithful).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/dodge_projectile.lua

step(20)
local px, py, pz = pos()
facing(0)
local d = spawn(px + 22, py + 5, pz + 80, 180, "striker_easy_1")
goals(d)
watch(d)
team(d, "syndicate")
assert(give_weapon(d, "w1_tap"))
set_hp(d, 2000)
assert(give_weapon(0, "w5_sbg"))
step(40)
local x0 = pos_of(d)
local most, aside = 0, 0
fire(1)
for _ = 1, 120 do
    aim(0, 0)
    step(1)
    local _, _, _, n = dodge_of(d)
    most = math.max(most, n or 0)
    aside = math.max(aside, math.abs(pos_of(d) - x0))
end
print(string.format("projectile dodges: %d; stepped aside %.1f", most, aside))
if faithful() then
  assert(most == 0 and aside < 2, "shipped: the ball goes undodged")
else
  assert(most > 0, "fixed: it takes the ball as a danger")
  assert(aside > 5, "and sidesteps")
end
print("ok")
