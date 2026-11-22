-- The pause screen: F1 opens it on the
-- objectives — the current one first, Previous back to older ones — with
-- items and weapons only once met, the diary up to this level (starting on
-- its first page) and the help, with the help overlay naming the rings'
-- parts; the game stands still while it is up, its sound on at half volume.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/pause.lua

mission_begin(false)
mission_exec([[
F.obj1 = function() C.objective_set(1) end
F.obj2 = function() C.objective_set(2) end
]])
step(5)
assert(mission_call("obj1"))
step(2)
assert(mission_call("obj2"))
step(2)

local v0 = overall_volume()
pause_toggle()
local p = pause_screen()
assert(p and p.tab == "objective", "F1 opens it on the objectives")
-- The sound runs on, at half volume (`ONcPauseScreen_VolumeCut`).
step(20)
print("volume:", v0, "→", overall_volume())
assert(math.abs(overall_volume() - v0 * 0.5) < 1e-3, "half volume while it is up")
print("objective page", p.page, "of", p.pages, ":", p.lines[1])
assert(p.page == 1 and p.pages == 2 and p.lines[1]:match("Muro and his Strikers"), "the current (second) objective first")
assert(p.previous and not p.next, "only Previous: back to the first")
pause_press("previous")
p = pause_screen()
assert(p.page == 2 and p.lines[1]:match("Barabas"), "Previous: the first objective")

-- The game stands still.
local x0, _, z0 = pos()
input(true, false)
step(30)
local x1, _, z1 = pos()
assert(x1 == x0 and z1 == z0, "nothing moves while it is up")
input(false, false)

-- Items and weapons: none met, no pages.
pause_press("items")
assert(pause_screen().pages == 0, "no items met yet")
pause_press("weapons")
assert(pause_screen().pages == 0, "no weapons met yet")

-- The diary: levels 1 to 3, on this level's first page.
pause_press("diary")
p = pause_screen()
print("diary page", p.page, "of", p.pages, ":", p.lines[1])
assert(p.pages > 10 and p.page < p.pages and p.previous, "earlier levels' pages before this level's")
assert(not hud_help(), "no help overlay off the Help page")
pause_press("help")
p = pause_screen()
assert(p.lines[1] == "Controls" and not p.previous and not p.next, "the help: one page, no paging")
-- The help overlay over the rings: their textures and 13 labels.
local with_help = hud_triangles()
pause_press("diary")
assert(not hud_help(), "the overlay goes with another page")
local without = hud_triangles()
pause_press("help")
assert(hud_help(), "the help overlay on the Help page")
print("help overlay:", with_help, "triangles; the diary page:", without)
pause_press("close")
assert(pause_screen() == nil, "Close")
assert(not hud_help(), "the overlay goes with the pause screen")
-- ui_show_help (a debugging command) shows it in play.
mission_exec([[
F.t_help = function(on) C.ui_show_help(on) end
]])
mission_call("t_help", "1")
step(1)
assert(hud_help(), "ui_show_help 1")
mission_call("t_help", "0")
step(1)
assert(not hud_help(), "ui_show_help 0")
step(20)
assert(math.abs(overall_volume() - v0) < 1e-3, "the volume back when it closes")

-- Met a pistol and its clips: their pages, the new ones flagged first.
assert(give_weapon(0, "w1_tap"), "a pistol")
step(2)
pause_toggle()
pause_press("weapons")
p = pause_screen()
print("weapons:", p.pages, p.lines[1])
assert(p.pages == 1, "the pistol's page")
pause_press("close")
print("ok")
