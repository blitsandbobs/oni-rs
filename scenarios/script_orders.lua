-- Script orders to characters (docs/lua.md, *Character orders*): the
-- level's own scripts poison, toughen and smite characters. A self-written
-- script function (translated-Lua form) runs each command through the real
-- dispatcher.
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/script_orders.lua

mission_begin()
step(5)
-- (The training locks Konoko's keys at first: free them.)
mission_exec([[F.t_free = function() C.input(1) C.lock_keys("keys_all") end]])
mission_call("t_free")
step(1)

-- Poison: a trigger volume's inside script calls it every tick — 5 damage
-- every 30 calls (the first after 30); 30 ticks without and it starts over.
mission_exec([[
F.t_gas = function(ticks)
    for _ = 1, tonumber(ticks) do
        C.chr_poison(0, 5, 30, 30)
        SLEEP(1)
    end
end
]])
local hp0 = hp(0)
mission_call("t_gas", "95")
step(100)
print("poisoned for 95 ticks: hp", hp0, "->", hp(0))
assert(hp0 - hp(0) == 15, "three doses in 95 ticks")
mission_call("t_gas", "20")
step(25)
assert(hp0 - hp(0) == 15, "a short whiff later: the count started over")

-- Unstoppable: blows still hurt, but it neither reacts nor is shoved.
mission_exec([[
F.t_tough = function(on) C.ai2_passive(W("Bay4_Comguy_1"), 1) C.chr_unstoppable(W("Bay4_Comguy_1"), on) end
]])
local c = place("Bay4_Comguy_1")
local px, py, pz = pos()
local function punch_it(on)
    mission_call("t_tough", on)
    teleport(c, px, py + 2, pz + 6)
    set_facing(c, 0) -- its back to Konoko
    teleport(0, px, py + 2, pz)
    facing(0)
    step(60)
    local hp0 = hp(c)
    punch()
    local reacted = false
    for _ = 1, 30 do
        step(1)
        local a = anim_of(c)
        if a:find("hit") or a:find("knock") or a:find("stagger") then reacted = true end
    end
    return hp0 - hp(c), reacted
end
local lost, reacted = punch_it("0")
print("normal: lost", lost, "reacted", reacted)
assert(lost > 0 and reacted, "a normal character reels from the blow")
lost, reacted = punch_it("1")
print("unstoppable: lost", lost, "reacted", reacted)
assert(lost > 0 and not reacted, "an unstoppable one takes it without reacting")

-- ai2_kill: every AI dies (never the player).
mission_exec([[F.t_smite = function() C.ai2_kill() end]])
mission_call("t_smite")
step(5)
assert(dead(c), "ai2_kill smites every AI")
assert(hp(0) > 0, "but never the player")
print("script orders OK")
