-- Character scale (#92): each character is sized from its class's range when
-- it comes (`ONrGameState_NewCharacter`: scaleLow..scaleHigh), and the size
-- scales the whole posed body about its feet (`chrBaseMatrix`).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/scale.lua

local HEAD = 10
step(10)
local px, py, pz = pos()
assert(scale_of(0) == 1, "Konoko's class is one size")

-- Classes of one size: Mutant Muro 1.75 (1.74 high), the dream lab's
-- Striker 0.5; most others a random 0.95–1.05.
local muro = spawn(px + 40, py + 5, pz, 0, "mutantmuro")
local small = spawn(px - 40, py + 5, pz, 0, "striker_dream_lab")
local thugs = {}
for k = 1, 6 do
    thugs[k] = spawn(px + 20 * k, py + 5, pz + 60, 0, "thug_wh_1")
end
step(30)
print(string.format("sizes: muro %.3f, small striker %.2f", scale_of(muro), scale_of(small)))
assert(scale_of(muro) >= 1.74 and scale_of(muro) <= 1.75, "Muro 1.74-1.75")
assert(scale_of(small) == 0.5, "the dream lab's Striker 0.5")
local seen = {}
for _, t in ipairs(thugs) do
    local s = scale_of(t)
    assert(s >= 0.95 and s <= 1.05, "a thug 0.95-1.05: " .. s)
    seen[string.format("%.4f", s)] = true
end
local kinds = 0
for _ in pairs(seen) do kinds = kinds + 1 end
assert(kinds > 1, "thugs sized apart")

-- The size scales the posed body about the feet: twice the size, the head
-- twice as high above them.
local function head_above(id)
    local _, fy = pos_of(id)
    return bone(id, HEAD).at[2] - fy
end
local d = spawn(px, py + 5, pz - 60, 0)
step(30)
local h1 = head_above(d)
set_scale(d, 2)
local h2 = head_above(d)
print(string.format("head above the feet: %.2f, at twice the size %.2f", h1, h2))
assert(math.abs(h2 - 2 * h1) < 0.01, "the head twice as high")
print("scale ok")
