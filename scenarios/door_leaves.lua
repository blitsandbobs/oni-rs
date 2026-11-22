-- Door leaves: while a door moves its leaves are
-- boxes sliding with the class's animation; a leaf closing on someone stops
-- (held still while they are in its way), and blocked 90 ticks the door turns
-- back and opens for them. On a double door the original counts only the
-- first leaf as blocking (Q14): in the way of the second, it stays half-shut.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/door_leaves.lua

local function wait_for(cond, max)
    for t = 1, max do
        step(1)
        if cond() then return t end
    end
    return nil
end

-- Door 18 (one leaf, sliding along x across z = -589.5): open, its leaf
-- moves aside.
step(5)
local leaf = door_leaves(18)[1]
assert(not leaf.solid, "closed: its own level quads stand in for the leaf")
local closed_x = leaf.at[1]
door(18, "open")
step(60)
leaf = door_leaves(18)[1]
print(string.format("open: the leaf slid %.1f", leaf.at[1] - closed_x))
assert(leaf.solid and leaf.at[1] - closed_x > 25, "it slides aside")

-- Konoko in the doorway; the door ordered shut closes on her.
teleport(0, -75, 2, -589.5)
step(20)
door(18, "close")
local held = wait_for(function() return door_leaves(18)[1].paused > 0 end, 60)
print("closing, the leaf meets her after", held)
assert(held, "the leaf stops at her")
local x = door_leaves(18)[1].at[1]
step(30)
assert(math.abs(door_leaves(18)[1].at[1] - x) < 0.01 and door_state(18) == "closing", "held still while she is in the way")
local back = wait_for(function() return door_state(18) == "opening" end, 120)
print("it turns back for her", back, "ticks later")
assert(back, "blocked a while, it opens for her")

-- Double door 1 (unlocked: a door turns back only for someone who may open
-- it): in the way of its second leaf only.
door(1, "unlock")
local function blocked_by_second_leaf()
    teleport(0, -700, 2, -700)
    step(5)
    door(1, "open")
    step(60)
    teleport(0, -664, 2, -753)
    step(20)
    door(1, "close")
    return wait_for(function() return door_state(1) == "opening" end, 300)
end
faithful(true)
local t = blocked_by_second_leaf()
print("faithful: the double door turns back:", t)
assert(not t and door_state(1) == "closing", "the original never counts the second leaf")
faithful(false)
t = blocked_by_second_leaf()
print("fixed: it turns back after", t)
assert(t, "either leaf counts")
print("door leaves ok")
