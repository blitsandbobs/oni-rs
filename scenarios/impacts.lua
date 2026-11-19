-- Impact effects (docs/AUDIO.md, *Impact effects*): Konoko's footsteps sound
-- by the floor's material and her light step weight (and the AI hears them);
-- a punch that lands on a dummy sounds by the dummy's struck part.
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
listen(40)
local hits = {}
for clip in pairs(heard) do
    if clip:match("pnch") or clip:match("punch") or clip:match("^bdy") or clip:match("^face") then hits[#hits + 1] = clip end
end
print("punch heard:", table.concat(hits, " "), "dummy hp", hp(dummy))
assert(hp(dummy) < 200, "the punch landed")
assert(#hits > 0, "the blow sounds")
print("impacts ok")
