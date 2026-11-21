-- The gamma option (docs/MENU.md, gamma.rs): the finished frame through the
-- original's display ramp, v^(0.4 + 1.2 (1 − gamma)) on the encoded values
-- — at 0.5 unchanged, brighter up to 1, darker down to 0. Checked on the
-- main menu's picture (a still frame).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/gamma.lua

local dir = os.getenv("TMPDIR") or "/tmp"
local function mean(g, name)
    if g then set_gamma(g) end
    local p = dir .. "/oni_gamma_" .. name .. ".png"
    screenshot(p, 320, 240)
    local r, gg, b = image_mean(p, 0.0, 0.0, 1.0, 1.0)
    os.remove(p)
    return (r + gg + b) / 3
end

menu_open()
local plain = mean(nil, "plain")
local half = mean(0.5, "half")
local bright = mean(1.0, "bright")
local dark = mean(0.0, "dark")
print(string.format("mean: as drawn %.4f, 0.5 %.4f, 1.0 %.4f, 0.0 %.4f", plain, half, bright, dark))
assert(math.abs(half - plain) < 1e-6, "0.5 leaves the frame as drawn")
assert(bright > plain + 0.05, "1.0 brightens")
assert(dark < plain - 0.02, "0.0 darkens")
-- (A mid-grey encoded 0.5 at gamma 1: 0.5^0.4 ≈ 0.76.)
set_gamma(0.5)
menu_close()
print("ok")
