-- Impulse sounds and channels (docs/AUDIO.md, docs/SOUND_ENGINE.md): level
-- 1's glass-breaking one-shots — heard by distance, panned, the PC volume
-- law; past three copies of the medium break the big one plays instead; out
-- of earshot nothing plays; six stereo channels, then the quietest is taken.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/sounds.lua

step(10)
facing(0)
step(1)
local x, y, z = pos()
local function clip(c) return channels()[c + 1].clip end

-- Close by, off to the right (−X when facing +Z): full on the right.
local c = impulse("glass_med", x - 60, y + 15, z)
assert(c and c >= 6, "a mono sound on a mono channel: " .. tostring(c))
local ch = channels()[c + 1]
print("glass_med on", c, ch.clip, "volume", ch.volume, "gains", ch.left, ch.right)
assert(ch.clip:match("^glass_med"), "one of its permutations")
assert(ch.right > ch.left and ch.left > 0, "panned to its side")

-- Three playing: the fourth is the big break.
impulse("glass_med", x, y, z + 30)
impulse("glass_med", x, y, z + 30)
local alt = impulse("glass_med", x, y, z + 30)
print("fourth:", clip(alt))
assert(clip(alt):match("^glass_big"), "past its threshold, its alt")

-- 260 ft is 780 units: beyond that nothing.
assert(impulse("glass_big", x + 900, y, z) == nil, "out of earshot")

-- The big break is stereo: six stereo channels; a seventh takes the
-- quietest of them (the farthest).
for k = 1, 6 do impulse("glass_big", x, y, z + 30 + k * 60) end
local quiet, q = 1e9, nil
for i = 1, 6 do
    assert(channels()[i].clip, "stereo channel " .. (i - 1) .. " busy")
    if channels()[i].volume < quiet then quiet, q = channels()[i].volume, i - 1 end
end
local taken = impulse("glass_big", x, y, z + 30)
print("the 7th took channel", taken, "the quietest was", q, quiet)
assert(taken == q, "it took the quietest")

-- They play out.
step(600)
for i = 1, 16 do assert(not channels()[i].clip, "all played out") end
print("sounds ok")
