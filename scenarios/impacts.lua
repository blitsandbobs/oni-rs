-- Impact effects (docs/AUDIO.md, *Impact effects*): Konoko's footsteps sound
-- by the floor's material and her light step weight (and the AI hears them);
-- a punch that lands on a dummy sounds by the dummy's struck part, and
-- swooshes (her variant's punch sound); its hit flash takes the colour of
-- the dummy's health (the flash passing it to the spark it throws).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/impacts.lua

step(20)
facing(0)
local heard = {}
local function listen(n)
    for _ = 1, n do
        step(1)
        for _, c in ipairs(channels()) do
            if c.clip then heard[c.clip] = (heard[c.clip] or 0) + 1 end
        end
    end
end
local function any(pattern)
    local found = {}
    for clip in pairs(heard) do
        if clip:match(pattern) then found[#found + 1] = clip end
    end
    return found
end

-- Run: Konoko's light running steps.
input("forward")
listen(90)
input()
listen(30)
local steps = any("^kon_r_")
print("running steps heard:", table.concat(steps, " "))
assert(#steps > 0, "her running footsteps play")

-- A punch on a dummy's back.
heard = {}
local px, py, pz = pos()
local dummy = spawn(px, py + 5, pz + 6, 0)
step(60)
facing(0)
punch()
local flash, glow
for _ = 1, 40 do
    listen(1)
    flash = flash or particles_of("h2h_flash_e01")[1]
    glow = glow or particles_of("h2h_flash_p03")[1]
end
local hits = {}
for clip in pairs(heard) do
    if clip:match("^pnch_hvy") or clip:match("^bdy") or clip:match("^face") then hits[#hits + 1] = clip end
end
print("punch heard:", table.concat(hits, " "), "dummy hp", hp(dummy))
assert(hp(dummy) < 200, "the punch landed")
assert(#hits > 0, "the blow sounds")
local swoosh = any("^pnch_lt") -- (Konoko's punch: kon_pnch_lt's sounds)
print("swoosh:", table.concat(swoosh, " "))
assert(#swoosh > 0, "the punch swooshes")
assert(flash and glow, "the blow flashes")
local shade = health_shade(dummy)
print(string.format("flash tint %08x, its glow %08x, the dummy's health %08x", flash[7], glow[7], shade))
assert(flash[7] == shade, "the flash takes the victim's health colour")
assert(glow[7] == shade, "its glow inherits it")
assert(shade ~= 0xff00ff00 and shade ~= 0xffffffff, "hurt: no longer full green")
print("impacts ok")
