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
local b, t0, firing, aim0, aim_moved, second, plo, phi
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
            -- While the wave goes out the gun holds still: it aims as the
            -- player's does (exactly), not riding the hand's sway.
            if t - t0 >= 350 and t - t0 <= 390 and g then
                local p = math.deg(math.asin(g.x[2]))
                plo, phi = math.min(plo or p, p), math.max(phi or p, p)
            end
        end
    end
end
print("Barabas fires for", firing, "ticks; his second shot heads", second)
assert(firing and firing > 50, "his recorded trigger fires his gun")
assert(aim_moved, "his recorded turns aim it")
assert(second and second > 60 and second < 110, "turned to +x, where Konoko landed")
print(string.format("barrel pitch while the second wave goes: %.2f..%.2f", plo, phi))
assert(phi - plo < 1, "the gun holds still while it fires")
print("film fire ok")
