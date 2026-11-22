-- The launcher (#104), as drawn with no game installed: choosing the
-- Anniversary Edition, a source that won't do, and an install under way
-- (shown only: none runs). LAUNCHER_SHOTS=dir keeps the screenshots, at
-- 1280 × 720 and at 4K (the panel scaled as the game's screens are).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/launcher.lua

local shots = os.getenv("LAUNCHER_SHOTS")
local function shot(name, w, h)
    screenshot(shots and (shots .. "/" .. name .. ".png") or "/dev/null", w or 1280, h or 720)
end

launcher_show()
shot("choose")
shot("choose_4k", 3840, 2160)
launcher_show({ error = "Some of the game's files are missing: level4_Final.raw, the original level scripts." })
shot("error")
launcher_show({ step = "Converting level 3", done = 0.62 })
shot("progress")
launcher_show(false)
