-- Voices (docs/AUDIO.md, *Voices*): a striker hit again and again grunts
-- (its pain sounds, by its run of pain), and cries out when killed; an AI
-- that sees an enemy says its startle (or alert) line, once.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/voices.lua

step(20)
local px, py, pz = pos()
local clips = {}
local function listen(n)
    for _ = 1, n do
        step(1)
        for _, c in ipairs(channels()) do
            if c.clip then clips[c.clip] = true end
        end
    end
end
local function any(pattern)
    for clip in pairs(clips) do
        if clip:match(pattern) then return clip end
    end
end

-- A passive striker, its back to the player: punch it till it falls.
local d = spawn(px, py + 5, pz + 6, 0, "striker_easy_1")
step(60)
facing(0)
for _ = 1, 40 do
    if hp(d) <= 0 then break end
    punch()
    listen(25)
end
print("striker hp", hp(d), "grunts:", any("^tim_hrt") or "-", "death:", any("^tim_dth") or "-")
assert(any("^tim_hrt"), "it grunts when hurt")
assert(hp(d) <= 0, "it was beaten")
assert(any("^tim_dth") or any("^tim_atk_md") or any("^tim_hrt_md"), "it cries out as it dies")

-- An AI striker, hostile, in sight: its startle or alert line.
local e = spawn(px, py + 5, pz + 60, 180, "striker_easy_1")
goals(e)
team(e, "syndicate")
local said
for _ = 1, 300 do
    step(1)
    said = said or saying(e)
    if said then break end
end
print("the striker says:", said)
assert(said == "c18_70_04striker" or said == "c18_69_04striker", "its startle / alert line")
print("voices ok")
