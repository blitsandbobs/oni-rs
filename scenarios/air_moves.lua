-- The combat jump: once airborne, punch / kick play the jump attacks and a
-- crouch tap plays the flip. Each comes from the same move rules as on the
-- ground — the airborne body state is what selects the jump version (the
-- jump-attack clips are reachable from the airborne states via shortcuts).
--
--   cargo run --release -p oni -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/air_moves.lua

local PUNCH, KICK, FLIP = 22, 12, 75

-- Jump, wait until actually airborne (the takeoff clip plays on the ground
-- first), then let the flight settle a few ticks.
local function get_airborne()
    input(false, false); strafe(false, false); crouch(false)
    step(90) -- rest
    jump()
    local t = 0
    while not airborne() and t < 60 do step(1); t = t + 1 end
    assert(airborne(), "the jump left the ground")
    step(6)
end

local function check(label, want)
    print(string.format("%-10s anim=%s type=%d airborne=%s", label, anim(), anim_type(), tostring(airborne())))
    assert(airborne(), label .. ": still in the air")
    assert(anim_type() == want, label .. ": expected type " .. want .. ", got " .. anim_type() .. " (" .. anim() .. ")")
    assert(anim():find("jump") ~= nil, label .. ": a jump clip, got " .. anim())
end

get_airborne(); punch(); step(2); check("air punch", PUNCH)
get_airborne(); kick(); step(2); check("air kick", KICK)
get_airborne(); crouch_tap(); step(2); check("air flip", FLIP)

-- Landing afterwards brings her back to the ground.
step(120)
assert(not airborne(), "lands after the air move")
print("air moves ok: jump punch, jump kick and flip all play in the air")
