-- Hypos: one taken heals a quarter of full health
-- over time, a point each time her class's 6-tick wait runs out (every 7); at full it boosts instead (up
-- to double, with no wait between points), and above full health drains back
-- a point every 10 ticks; a hurt eats two and a half times its damage from
-- what is still to come; none left, or already boosting, nothing happens.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/hypo.lua

step(5)
assert(not use_hypo(0), "none to use")
powerup(0, "hypo", 3)
set_hp(0, 50)
assert(use_hypo(0), "a hypo")
print("to come:", hypo_left(0))
assert(hypo_left(0) == 50, "a quarter of Konoko's 200")
step(60)
print("after 60 ticks:", hp(0))
assert(hp(0) == 59, "a point every 7 ticks: 9 so far")

-- A hurt eats into it: 10 damage takes 25 of what is to come.
damage(0, 10)
print("hurt:", hp(0), hypo_left(0))
assert(hp(0) == 49 and hypo_left(0) == 16, "hurt: 49 hp, 16 to come")
step(120)
assert(hp(0) == 65 and hypo_left(0) == 0, "the rest given")

-- At full: a boost, up to double, given at once a point a tick.
set_hp(0, 200)
assert(use_hypo(0), "a boost")
assert(hypo_left(0) == 200, "a whole boost: full again")
assert(not use_hypo(0), "already boosting: no more")
step(100)
print("boosting:", hp(0))
assert(hp(0) == 300, "a point a tick past full")
step(100)
assert(hp(0) == 400 and hypo_left(0) == 0, "double, all given")

-- Above full, it drains back a point every 11 ticks (a 10-tick wait).
step(110)
print("draining:", hp(0))
assert(hp(0) >= 390 and hp(0) <= 391, "draining")
print("ok")
