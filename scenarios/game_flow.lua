-- The game flow, the world's side: save_game keeps the
-- player and restore_game puts them back; a script's splash screen is asked
-- for; win and lose set the level's outcome; the player dying runs their
-- die script, which loses the level. (The app shows the splashes and loads
-- the levels.)
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/game_flow.lua

mission_begin(false)
step(2)
mission_exec([[
F.t_save = function() C.save_game(3) end
F.t_restore = function() C.restore_game() end
F.t_splash = function() C.splash_screen("warehouse_splash_screen") end
F.t_win = function() C.win() end
]])

-- A save: her place, health, inventory and gun.
give_weapon(0, "w1_tap")
give_ammo(0, 2, 1)
set_hp(0, 50)
local x, y, z = pos()
mission_call("t_save")
step(1)
assert(game().save_point == 3, "saved at save point 3")

-- Things change; the restore puts them back.
teleport(0, x + 60, y, z + 60)
set_hp(0, 100)
drop_weapon(0)
step(5)
mission_call("t_restore")
step(2)
local rx, ry, rz = pos()
local clips = inventory(0)
print(string.format("restored: at (%.0f %.0f %.0f), hp %d, gun %s, clips %d", rx, ry, rz, hp(0), tostring(weapon(0)), clips))
assert(math.abs(rx - x) < 2 and math.abs(rz - z) < 2, "back where she saved")
assert(hp(0) == 50 and weapon(0) == "w1_tap" and clips == 2, "with her health, gun and clips")

-- A script's splash screen.
mission_call("t_splash")
step(1)
assert(game().splash == "warehouse_splash_screen", "a splash is asked for")

-- Winning.
mission_call("t_win")
step(1)
assert(game().victory == true, "won")

-- Konoko dying: her die script (from her placement) fades out and loses the
-- level, some 8 s on.
mission_begin(true, 1)
step(60)
damage(0, 100000)
local lost
for t = 1, 700 do
    step(1)
    if game().victory == false then lost = t break end
end
print("lost after", lost, "ticks; faded to", hud().fade)
assert(lost and lost > 400 and hud().fade > 0.99, "she died: faded out, the level lost")
print("ok")
