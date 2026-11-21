-- Character and object environment animations in step (`chr_envanim`,
-- `env_anim`): level 3's opening rides Konoko (`KonBipedBox01`) on the
-- motorcycle (`motorcycle02`), both started the same tick. A character's
-- clip starts at frame 1 (the script's 1 reset to 0 by `OBrAnim_Start`,
-- then stepped on), keeping her on the seat; a frame ahead, at the bike's
-- top speed (6–8 a frame) she sat over the front wheel.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/envanim_sync.lua

mission_begin(true)
local worst, moving = 0, 0
local last
for t = 1, 800 do
    step(1)
    local bike
    for _, o in ipairs(objects()) do
        if o.name:match("motorcycle") then bike = o end
    end
    if bike then
        local x, _, z = pos()
        local d = math.sqrt((bike.at[1] - x) ^ 2 + (bike.at[3] - z) ^ 2)
        if last and math.abs(bike.at[3] - last) > 4 then
            -- (Riding fast: where a frame's lead shows.)
            moving = moving + 1
            worst = math.max(worst, d)
        end
        last = bike.at[3]
    end
end
print(string.format("ticks at speed %d, Konoko at most %.2f from the bike", moving, worst))
assert(moving > 100, "the ride was seen")
assert(worst < 2.2, "on the seat, not ahead of it")
print("ok")
