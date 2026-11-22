-- A film plays its actor's recorded input in full (#94): its turns aim (up /
-- down, scaled by the gun's aiming speed) and its fire buttons fire the gun
-- it holds, as the player's do — not strike. Level 3's intro: Barabas,
-- walking in with his gun raised (`ai2_setmovementmode walk`), fires down
-- the road at Konoko's motorcycle (IntroSubboss01 holds Fire1 from its tick
-- 44).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/film_fire.lua

mission_begin(false)
mission_call("intro")
local b, firing, first, aim0, aim_moved
for t = 1, 900 do
    step(1)
    b = b or named("Barabus")
    if b then
        aim0 = aim0 or aim_of(b)
        if math.abs(aim_of(b) - aim0) > 0.01 then aim_moved = true end
        local _, _, f = weapon(b)
        if f then
            firing = (firing or 0) + 1
            first = first or t
        end
    end
end
print("Barabas fires for", firing, "ticks from", first)
assert(firing and firing > 50, "his recorded trigger fires his gun")
assert(aim_moved, "his recorded turns aim it")
print("film fire ok")
