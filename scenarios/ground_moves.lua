-- Crouch, crouch-run, the combat roll, and a directional jump out of a
-- sidestep.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/ground_moves.lua

local CROUCH_STATE, CROUCH_RUN, ROLL_FORWARD = 19, 58, 144
local FLYING_RIGHT = 45

local function rest()
    input(false, false); strafe(false, false); crouch(false); walk(false)
    step(90)
end
local function show(label)
    print(string.format("%-22s anim=%-24s type=%3d state=%2d", label, anim(), anim_type(), state()))
end

-- Holding crouch ducks into the crouched state; forward then crouch-runs.
rest()
crouch(true); step(40); show("crouched")
assert(state() == CROUCH_STATE, "holding crouch reaches the crouched state, got " .. state())
input(true, false); step(30); show("crouch-moving")
assert(anim_type() == CROUCH_RUN, "crouch + forward crouch-runs, got " .. anim_type())

-- Crouch tapped while running: the forward combat roll.
rest()
input(true, false); step(25); show("running")
crouch_tap(); step(3); show("crouch tap")
assert(anim_type() == ROLL_FORWARD, "a crouch tap mid-run rolls forward, got " .. anim_type())

-- Jumping while sidestepping right takes off into the right-jump flight.
rest()
strafe(false, true); step(25); show("sidestepping right")
jump(); step(2); show("jump")
assert(state() == FLYING_RIGHT, "the jump heads right (state " .. FLYING_RIGHT .. "), got " .. state())

rest()
print("ground moves ok: crouch, crouch-run, roll, sidestep jump")
