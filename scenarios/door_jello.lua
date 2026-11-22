-- Doors in the camera's way (-): a closed door is its own level quads (its "gunk"), which
-- go see-through as any wall's; open or moving, its leaves are objects, see-
-- through when the camera's ray meets them (`OBcFlags_JelloObjects`), and
-- they hide what is behind them from a lens flare's test (`AMrRayToObject`)
-- — a light no longer shows through a door sliding across it.
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/01/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 200,8,300 --script scenarios/door_jello.lua

-- Door 18 (one leaf, sliding along x across z = -589.5), locked shut;
-- Konoko just past it, the camera behind her on the door's other side.
step(5)
door(18, "lock")
teleport(0, -75, 2, -604)
set_facing(0, 180)
step(60)
assert(door_state(18) == "closed")
local j = jello_doors()
print("closed: its quads see-through", j.quads)
assert(j.quads > 0, "the closed door between the camera and her goes see-through")
screenshot("/tmp/oni_door_jello.png", 160, 120)
local drawn, doors = jello_drawn()
print("see-through triangles drawn:", drawn, "the door's:", doors)
assert(doors >= j.quads, "and is drawn see-through")

-- Through the doorway, shut: the leaf isn't an object (its quads are the
-- level's), so a flare's object test meets nothing.
assert(not object_in_way(-75, 10, -610, -75, 10, -570), "closed: no object in the doorway")

-- Opening: the leaf is an object sliding across the doorway.
door(18, "unlock")
door(18, "open")
step(3)
assert(door_state(18) == "opening")
j = jello_doors()
print("opening: quads", j.quads, "leaves", table.concat(j.leaves, ","))
assert(j.quads == 0, "its level quads are hidden while it moves")
-- (As shipped the ray toward the camera's point is read as a point, so it
-- runs toward the world's origin: Q17.)
if not faithful() then
    assert(#j.leaves == 1 and j.leaves[1] == "18:0", "the moving leaf in the camera's way goes see-through")
end
assert(object_in_way(-75, 10, -610, -75, 10, -570), "the moving leaf is in the doorway: a flare beyond is hidden")
print("ok")
