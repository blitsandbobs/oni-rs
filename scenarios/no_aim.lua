-- Aiming with the gun, or just looking (#67;, *Aiming and
-- looking*): an AI at ease moves in the no-aim modes, its gun in hand but
-- its gun's poses off (`ONcCharacterFlag_DontUseGunVarient`) — the unarmed
-- clips. In a fight its targeting makes it aim with the gun: the pistol
-- clips.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/no_aim.lua

step(20)
local px, py, pz = pos()
facing(180)
-- A thug well behind Konoko (not seeing her), a pistol in hand, at ease.
local d = spawn(px, py + 5, pz + 120, 0, "striker_easy_1")
assert(give_weapon(d, "w1_tap"))
goals(d)
step(60)
print("at ease:", anim_of(d), weapon(d))
assert(weapon(d) == "w1_tap" and not anim_of(d):match("PIS"), "gun in hand, unarmed clips")

-- Seen and fought: it aims with the gun.
team(d, "syndicate")
set_facing(d, 180)
teleport(0, px, py + 5, pz + 60)
facing(0)
local aimed
for t = 1, 600 do
    step(1)
    set_hp(0, 1000)
    if anim_of(d):match("PIS") then aimed = t break end
end
print("fighting:", anim_of(d), "after", aimed, goal_of(d))
assert(aimed, "the pistol clips in a fight")
print("ok")
