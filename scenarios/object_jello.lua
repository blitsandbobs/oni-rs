-- Objects in the camera's way go see-through (*See-through
-- walls*): the ray the walls' test uses, from the camera to beside the
-- player's head, flags each placed object whose sphere it passes through,
-- drawn that frame at 31 of 255 (`CAiTrackJello`, `OBrDraw`). As shipped the
-- ray's vector is read as its far end, so the objects it meets are those
-- toward the world's origin — not the van's door here (Q17; `--faithful`).
--
--   cargo run --release -p oni-view -- \
--     ../generated/oni/levels/03/environment.glb \
--     --character ../generated/oni/common/characters/konoko_body_high/model.glb \
--     --anim ../generated/oni/common/animations/KONOKOrun_lt.glb \
--     --char-pos 0,0,0 --script scenarios/object_jello.lua

mission_begin(false)
mission_exec([[F.mk = function() C.obj_create(0, 10000) end]])
step(2)
assert(mission_call("mk"))
-- Behind level 3's van, its rear door between Konoko and the camera.
local function door_seen_through(shipped)
  faithful(shipped)
  teleport(0, 1178, -50, -80)
  set_facing(0, 0)
  step(40)
  local seen = jello_objects()
  print(shipped and "as shipped:" or "fixed:", table.concat(seen, ", "))
  for _, n in ipairs(seen) do if n == "object_Blackvan_doors03" then return true end end
  return false
end
assert(not door_seen_through(true), "as shipped: the ray toward the origin misses the door")
assert(door_seen_through(false), "the van's door in the camera's way goes see-through")
-- Well away from the van: nothing in the way, nothing see-through.
teleport(0, 1178, -50, 60)
set_facing(0, 0)
step(90)
print("away:", table.concat(jello_objects(), ", "))
assert(#jello_objects() == 0, "nothing in the way, nothing see-through")
print("ok")
