-- Skipping a cutscene (docs/GAME_FLOW.md, *Skipping a cutscene*): asked
-- for 10 ticks or more into one, the screen fades to black over 20 frames
-- with the sound off, the game then runs 32 ticks a frame unseen until the
-- scripts end the cutscene, and the screen fades back in over 20 frames
-- with the sound back. (As shipped nothing asks: Space is restored, Q18.)
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/cutscene_skip.lua

mission_begin(false)
mission_exec([[
F.cut = function() C.begin_cutscene() SLEEP(640) C.end_cutscene() end
]])
step(5)
local v0 = overall_volume()
assert(mission_call("cut"), "cut")
step(5)
skip_cutscene()
assert(cutscene_skip() == "idle", "too soon after it began")
step(6)
skip_cutscene()
local mode, black = cutscene_skip()
assert(mode == "starting" and black == 0, "the fade to black starts: " .. mode)

-- 20 frames of fade to black, then skipping.
local frames = 0
repeat
    step(1)
    frames = frames + 1
    mode, black = cutscene_skip()
until mode ~= "starting"
print("faded out in", frames, "frames; now", mode)
-- (The 20th frame's ticks run as usual, its black drawn: skipping from the
-- next.)
assert(frames == 20 and mode == "skipping" and black == 1, "black after 20 frames, then skipping")
step(10)
print("volume:", v0, "→", overall_volume())
assert(overall_volume() < 0.01, "the sound off")

-- The rest of the 640-tick cutscene goes by at 32 ticks a frame.
local skipping = 10
repeat
    step(1)
    skipping = skipping + 1
    mode = cutscene_skip()
until mode ~= "skipping" or skipping > 100
print("the cutscene over after", 11 + frames + skipping, "frames; now", mode)
assert(mode == "fading in", "it ended and the screen fades back in")
assert(11 + frames + skipping < 60, "far quicker than its 640 ticks")
for _ = 1, 20 do step(1) end
mode, black = cutscene_skip()
assert(mode == "idle" and black == nil, "the screen clear again")
step(20)
assert(math.abs(overall_volume() - v0) < 1e-3, "the sound back")
print("ok")
