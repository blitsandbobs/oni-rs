-- A film plays its actor's recorded input in full (#94): its turns turn
-- and aim (scaled by the held gun's aiming speed) and its fire buttons fire
-- the gun it holds, as the player's do — not strike. Level 3's intro:
-- Barabas starts IntroSubboss01 side-on to the road (182°) and fires his
-- first shot that way (his gun charges first); his startle as Konoko jumps
-- him turns him to +x, where she lands — an animation's turn turns what he
-- wants to face too (`ONrCharacter_AdjustFacing`), so he stays turned — and
-- his second shot goes after her (film tick ~350).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/film_fire.lua

mission_begin(false)
mission_call("intro")
local function yaw(x, z) return math.deg(math.atan(x, z)) % 360 end
local b, t0, firing, aim0, aim_moved, second
for t = 1, 1000 do
    step(1)
    if not b then
        b = named("Barabus")
        t0 = t
    end
    if b then
        aim0 = aim0 or aim_of(b)
        if math.abs(aim_of(b) - aim0) > 0.01 then aim_moved = true end
        local _, _, f = weapon(b)
        if f then
            firing = (firing or 0) + 1
            -- The second shot's gun heading (film tick ~350).
            local g = held_gun(b)
            if t - t0 >= 340 and t - t0 <= 360 and g then
                second = second or yaw(g.x[1], g.x[3])
            end
        end
    end
end
print("Barabas fires for", firing, "ticks; his second shot heads", second)
assert(firing and firing > 50, "his recorded trigger fires his gun")
assert(aim_moved, "his recorded turns aim it")
assert(second and second > 60 and second < 110, "turned to +x, where Konoko landed")
print("film fire ok")
