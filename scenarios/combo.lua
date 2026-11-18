-- Melee combos: punch chains P → PP → PPP, kick chains K → KK → KKK. Attacks
-- play from the KONCOM (combat) set, gated by the Fight variant.
--
--   cargo run --release -p oni-view -- \
--     generated/oni/levels/01/environment.glb \
--     --character generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/combo.lua

-- Settle standing.
step(30)

-- Helper: queue an attack, then play until the clip is about to end, collecting
-- the clip name. Returns the clip that played.
local function do_attack(kind)
    if kind == "punch" then punch() else kick() end
    step(1)
    local clip = anim()
    -- play out most of the clip (combo window opens near the end)
    for _ = 1, 20 do step(1); if anim() ~= clip then break end end
    return clip
end

-- Punch combo: P then (chain) PP then PPP.
local p1 = do_attack("punch")
punch(); for _ = 1, 30 do step(1) end  -- buffer + let it chain + play
local p2 = anim()
punch(); for _ = 1, 30 do step(1) end
local p3 = anim()
print(string.format("punch chain: %s -> %s -> %s", p1, p2, p3))
assert(p1:find("comb_p") or p1:find("punch"), "a punch clip started, got " .. p1)

-- Mash punch (press, release, press, …) and confirm all three combo clips
-- appear. A press only counts when the button went down this tick, so the
-- mash leaves a released tick between presses, like real button-mashing.
step(60) -- idle out the combo-drop window
local seen_p, seen_pp, seen_ppp = false, false, false
for _ = 1, 100 do
    punch()
    step(2)
    local a = anim()
    if a == "KONCOMcomb_p" then seen_p = true end
    if a == "KONCOMcomb_p_p" then seen_pp = true end
    if a == "KONCOMcomb_p_p_p" then seen_ppp = true end
end
print(string.format("held-punch saw P=%s PP=%s PPP=%s", tostring(seen_p), tostring(seen_pp), tostring(seen_ppp)))
assert(seen_p and seen_pp and seen_ppp, "the P/PP/PPP combo chain plays")

-- Kick starter works too.
step(60)
kick()
step(2)
print("kick clip: " .. anim())
assert(anim():find("comb_k") or anim():find("kick"), "a kick clip started, got " .. anim())
