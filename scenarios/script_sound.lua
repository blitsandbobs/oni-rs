-- What level scripts play (docs/AUDIO.md, *Scripts*): level 1's opening —
-- its music starts (an unplaced script ambient), and Shinatama's lines play
-- one at a time, each blocking script waiting for the line before.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/script_sound.lua

mission_begin()
local music, lines, overlap = false, {}, false
for _ = 1, 1500 do
    step(1)
    local on = 0
    for _, s in ipairs(sounds()) do
        if s.name == "atm_gr06" then music = true end
        if s.name:match("^c00_01_") and s.stage ~= "out" then
            on = on + 1
            if not lines[s.name] then lines[s.name] = true; lines[#lines + 1] = s.name end
        end
    end
    if on > 1 then overlap = true end
end
print("music:", music, "lines:", table.concat(lines, " "))
assert(music, "the level's music plays")
assert(#lines >= 4, "Shinatama's lines, one after another")
assert(not overlap, "never two at once")
assert(#mission_info().errors == 0, "no script errors")
print("script sound ok")
