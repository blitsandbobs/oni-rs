-- Flinches (#8, docs/COMBAT.md, *Flinches*): a minor hit reaction plays the
-- class's hit overlay over the upper body while the body carries on — a
-- shot's normal damage (a Striker shot with the pistol), and a light blow a
-- knockdown-resistant character shrugs off (the Mutant Muro punched from
-- behind: no full-body reaction, a flinch).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/flinch.lua

local HIT_OVERLAY, HIT_OVERLAY_AI = 57, 232
local function flinch(id)
  for _, o in ipairs(overlays_of(id)) do
    if o.slot == "hit" and (o.type == HIT_OVERLAY or o.type == HIT_OVERLAY_AI) then return o end
  end
end

-- Shot: the pistol's normal damage.
facing(0)
give_weapon(0, "w1_tap")
step(30)
local px, py, pz = pos()
local s = spawn(px, py + 5, pz + 40, 180, "striker_easy_1")
team(s, "syndicate")
step(30)
local hp0 = hp(s)
local seen, gone
hold_fire(true)
step(2)
hold_fire(false)
for t = 1, 90 do
  step(1)
  local o = flinch(s)
  if o and not seen then seen = o print(string.format("shot: %s flinches (%s), body %s", "striker", o.clip, anim_of(s))) end
  if seen and not o and not gone then gone = t end
end
assert(hp(s) < hp0, "the shot hit")
assert(seen, "a shot flinches the Striker")
assert(gone, "the flinch ends")
drop_weapon(0)
step(10)

-- Punched from behind, the knockdown-resistant Mutant Muro only flinches.
teleport(0, px, py + 2, pz)
facing(0)
step(10)
local m = spawn(px, py + 5, pz + 9, 0, "mutantmuro")
step(30)
local mhp = hp(m)
local mseen
for t = 1, 60 do
  if t == 1 then punch(true) elseif t == 3 then punch(false) end
  step(1)
  local o = flinch(m)
  if o and not mseen then mseen = o print(string.format("punch: the Mutant Muro flinches (%s), body %s", o.clip, anim_of(m))) end
end
assert(hp(m) < mhp, "the punch landed")
assert(mseen, "a resistant character shrugs off a light hit with a flinch")
print("ok")
