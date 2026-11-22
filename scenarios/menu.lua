-- The main menu: at the start Options shows and Escape does
-- nothing; New Game asks first (with the port's faithful box, off), Load
-- Game lists the levels reached and their continues (level 1's warehouse
-- always), Options shows the defaults (a slider held pages on), Quit asks
-- first; over a game Resume shows and Escape resumes. Each dialog drawn in
-- the out-of-game skin, and the level-load dialog.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/menu.lua

local shots = os.getenv("MENU_SHOTS")
local function shot(name)
    if shots then screenshot(shots .. "/" .. name .. ".png", 640, 480) end
end

menu_open()
local m = menu_state()
assert(m.dialog == "mainmenu", "the main menu")
assert(m.visible[102] == "Options" and m.visible[104] == nil, "Options, not Resume, at the start")
shot("main")
assert(menu_key("escape") == nil and menu_state().dialog == "mainmenu", "Escape does nothing at the start")

-- New Game: the prompt, its faithful box off; Start with it ticked.
assert(menu_click(100) == nil)
m = menu_state()
assert(m.dialog == "NewGame" and m.depth == 2, "the new game prompt")
assert(m.checks[3] == false, "faithful off by default")
shot("new_game")
assert(menu_click(1) == "new_game", "Start: a new game as fixed")
menu_click(3)
assert(menu_state().checks[3] == true)
assert(menu_click(1) == "new_game faithful", "Start with the box ticked: faithful")
assert(menu_key("escape") == nil and menu_state().dialog == "mainmenu", "Escape: back")

-- Load Game: level 1 and its warehouse (no saved game here).
menu_click(101)
m = menu_state()
assert(m.dialog == "loadgame", "the load game dialog")
print("load rows:", table.concat(m.rows, " | "))
assert(m.rows[1] == "TCTF Training" and m.rows[2] == "Syndicate Warehouse", "level 1, then the warehouse")
shot("load_game")
assert(menu_key("enter") == nil, "nothing selected: Load does nothing")
menu_key("down")
assert(menu_state().selected == "1 0")
menu_key("down")
assert(menu_key("enter") == "load 1 1", "the warehouse: level 1 from save point 1")
menu_click(2)

-- Options: the original's defaults.
menu_click(102)
m = menu_state()
assert(m.dialog == "options")
assert(m.sliders[102] == 100 and m.sliders[110] == 50 and m.sliders[100] == 2, "volume 100, gamma 50, quality 2")
assert(m.checks[107] == false and m.checks[109] == true, "subtitles off, invert mouse on")
assert(m.popups[108] == 1, "difficulty normal")
-- The port's own row on the Graphics panel (#58): characters hide the lens
-- flares behind them, on by default; a click turns it off, another on.
assert(m.checks[112] == true, "lens flares hidden behind characters")
assert(menu_click(112) == "options" and menu_state().checks[112] == false, "unticked")
assert(menu_click(112) == "options" and menu_state().checks[112] == true, "ticked again")
-- (Q19: shipped, the Invert Mouse box, ticked for inverted, reads "Off".)
assert(m.visible[109] == (faithful() and "Off" or "On"), "the invert box's title: " .. tostring(m.visible[109]))
shot("options")
assert(menu_click(107) == "options" and menu_state().checks[107] == true, "subtitles on")
-- A slider held on its track pages on (its timer, WMiSlider_HandleTimer)
-- until the thumb comes under the mouse, which then holds it.
assert(menu_press(110, 0.75) == "options" and menu_state().sliders[110] == 60, "pressed right of the thumb: a page more")
menu_timer(1)
assert(menu_state().sliders[110] == 70, "held: a page each firing")
menu_timer(20)
local held = menu_state().sliders[110]
print("gamma held at 75% across:", held)
assert(held > 70 and held < 100, "paging stopped where the thumb met the mouse")
menu_timer(5)
assert(menu_state().sliders[110] == held, "the thumb taken: no more paging")
menu_release()
menu_timer(5)
assert(menu_state().sliders[110] == held, "let go: nothing")
menu_click(2)

-- Quit: asks first.
menu_click(103)
assert(menu_state().dialog == "QuitYesNo")
shot("quit")
assert(menu_click(2) == nil and menu_state().dialog == "mainmenu", "No: back")
menu_click(103)
assert(menu_click(1) == "quit", "Yes: quit")

-- Over a game: Resume, and Escape resumes.
menu_open(true)
m = menu_state()
assert(m.visible[104] == "Resume" and m.visible[102] == nil, "Resume, not Options, in a game")
assert(menu_key("escape") == "resume")
assert(menu_click(104) == "resume")
menu_close()

-- The level-load dialog (the shipping build's Progress, 155): alone over
-- black, its bar filling.
for _, p in ipairs({0, 55, 100}) do
    menu_loading(p)
    shot("loading_" .. p)
end
menu_loading()
print("ok")
