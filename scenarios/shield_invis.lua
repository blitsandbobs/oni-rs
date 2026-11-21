-- Shield belts and invisibility at work (#60; docs/POWERUPS.md), and the
-- cheat codes that try them out (docs/CHEATS.md): typed on the pause screen,
-- `forcefield` (the port's) gives a full shield belt, `moonshadow` the
-- endless phase cloak.
--
-- A shield shows climbing to it, 2 a tick; it soaks shots — no hurt — and
-- the parts struck flare. Invisible, the body fades out; after 90 ticks the
-- AI can't see it; a blow shows it for a moment.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/shield_invis.lua

step(20)
local px, py, pz = pos()

-- The codes: nothing until allowed (the game won), then on the pause screen.
pause_toggle()
assert(type_keys("forcefield") == nil, "not before the game is won")
cheats(true)
assert(type_keys("forcefield") == "Force Field Received")
pause_press("close")
local e = effects(0)
assert(e.shield == 100 and e.shield_shown == 0, "a full shield, not shown yet")
step(10)
assert(effects(0).shield_shown == 20, "shown climbing 2 a tick")
step(100)
e = effects(0)
assert(e.shield_active and e.shield_parts[1] == 89, "the glow up to 69 + 20")

-- A dummy with a shield, shot: it soaks the shot, the parts struck flare.
local d = spawn(px, py, pz + 30, 180)
assert(powerup(d, "shield") == 100)
step(120)
local hp0 = hp(d)
assert(give_weapon(0, "w1_tap"))
facing(0)
aim(0, -0.1)
step(15)
fire(1)
local flared = false
for _ = 1, 30 do
    step(1)
    for _, p in ipairs(effects(d).shield_parts) do
        if p > 200 then flared = true end
    end
    if flared then break end
end
print("dummy's shield after the shot:", effects(d).shield, "hp", hp(d), "of", hp0)
assert(effects(d).shield < 100, "the shield took the shot")
assert(hp(d) == hp0, "no hurt through a shield")
assert(flared, "where it struck, the shield flared")
screenshot("/dev/null")
assert(magic_drawn() >= 2, "both shields drawn")

-- The phase cloak: Konoko fades out; after 90 ticks the AI can't see her.
pause_toggle()
assert(type_keys("moonshadow") == "Phase Cloak Enabled")
pause_press("close")
step(1)
assert(effects(0).invis_active and effects(0).invis_alpha > 0.95, "it starts seen")
local s = spawn(px, py + 5, pz - 40, 0, "striker_easy_1")
watch(s)
step(130)
e = effects(0)
assert(e.invis_alpha == 0, "faded out")
assert(not e.seen, "unseen after 90 ticks")
assert(contact_of(s, 0) == nil, "a Striker facing her doesn't see her")
assert(e.invisibility == 65535, "the cheat's never runs out")
-- A blow shows her for a moment.
teleport(0, px, py, pz - 47)
facing(0)
step(5)
punch()
local shown = false
for _ = 1, 40 do
    step(1)
    if effects(0).invis_alpha >= 0.59 then shown = true break end
end
assert(shown, "struck or striking, she shows for a moment")
assert(effects(0).seen, "and the AI's count starts again")

-- Off: she fades back in.
pause_toggle()
assert(type_keys("moonshadow") == "Phase Cloak Disabled")
pause_press("close")
step(60)
assert(effects(0).invisibility == 0 and effects(0).invis_alpha == 1, "back")

-- A picked-up cloak runs down: 30 s.
assert(powerup(0, "invis") == 1800)
step(10)
assert(effects(0).invisibility == 1790)
print("ok")
