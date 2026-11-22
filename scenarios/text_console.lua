-- Data consoles' text: `text_console name`
-- opens the console's pages in a modal dialog (`ONrInGameUI_TextConsole_
-- Display`): the game stands still while it is up; Space or a click on its
-- button turns the page — Next until the last, which shows Close — and Esc
-- closes it at any time.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/text_console.lua

mission_begin(false)
mission_exec([[
F.tc = function() C.text_console("level_3b") end
F.tc_none = function() C.text_console("no_such_console") end
]])
step(5)
assert(mission_call("tc"))
step(1)
local c = text_console()
assert(c and c.name == "level_3b" and c.page == 1 and c.pages == 3 and not c.last, "level 3's console: page 1 of 3")
print("page 1:", c.lines[1])
assert(c.lines[1]:match("HyBAC"), "its first page's heading")

-- The game stands still: running forward goes nowhere.
local x0, _, z0 = pos()
input(true, false)
step(60)
local x1, _, z1 = pos()
assert(x1 == x0 and z1 == z0, "nothing moves while it is up")

-- Space: page 2, then the last (Close), then closed.
text_console_key("next")
assert(text_console().page == 2, "Space: the next page")
text_console_key("next")
assert(text_console().last, "the last page shows Close")
text_console_key("next")
assert(text_console() == nil, "Space on the last page closes it")
step(30)
local x2, _, z2 = pos()
assert(math.abs(x2 - x0) + math.abs(z2 - z0) > 1, "and the game goes on")
input(false, false)

-- The mouse: a click on the button turns the page (Next), and on the
-- last closes it (Close); a click beside it does nothing.
assert(mission_call("tc"))
step(1)
local b = text_console().button
assert(b, "the button's place")
assert(not text_console_click(b[1] - 5, b[2] + b[4] / 2), "beside the button: nothing")
assert(text_console().page == 1)
assert(text_console_click(b[1] + b[3] / 2, b[2] + b[4] / 2) and text_console().page == 2, "Next clicked: page 2")
text_console_click(b[1] + b[3] / 2, b[2] + b[4] / 2)
assert(text_console().last, "page 3, Close")
text_console_click(b[1] + b[3] / 2, b[2] + b[4] / 2)
assert(text_console() == nil, "Close clicked: closed")

-- Esc closes it at once; a console that isn't there opens nothing.
assert(mission_call("tc"))
step(1)
text_console_key("close")
assert(text_console() == nil, "Esc closes it")
assert(mission_call("tc_none"))
step(1)
assert(text_console() == nil, "no such console: nothing")
print("ok")
