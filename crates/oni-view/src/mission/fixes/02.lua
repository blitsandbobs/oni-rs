-- Level 2 (manplant): our fixes to the shipped scripts (docs/SCRIPT_FIXES.md).

-- S1: the Deadly Brain's save point can lock Konoko out of its room. The
-- cutscene starts when she leaves the volume in the room's antechamber —
-- by either side, and its far side is at door 26 — and the save that follows
-- locks door 26 without closing it, wherever she is. Run back out in the
-- moment after the cutscene and the save keeps her locked out. Fixed: the
-- save waits until she is in the antechamber, then closes the door before
-- locking it.
local function in_antechamber()
    local x, y, z = PLAYER_AT()
    return z < 243 and x > -290 and x < -210 and math.abs(y + 76) < 20
end

FIX("S1", "save_point_4", function(save)
    return function(...)
        while not in_antechamber() do
            SLEEP(1)
        end
        C.door_close(26)
        return save(...)
    end
end)

-- S4: restoring that save point takes one console off the count for the
-- brain's next round, so it needs only 3 of the 4. Fixed: all 4 again.
FIX("S4", "func_start", function(start)
    return function(...)
        start(...)
        if EQ(V.my_save_point, 4) then
            V.console_count = 4
        end
    end
end)
