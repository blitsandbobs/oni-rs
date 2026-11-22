-- Level 4 (airport): our fixes to the shipped scripts (docs/deviations.md).

-- S2: restoring save point 3 leaves the earlier tarmac's volumes live (the
-- save point 2 restore turns them off). Going back, they spawn the tarmac's
-- guards again, replay the bomber's blast — and the tarmac's own volume
-- saves the game, back at save point 2. Fixed: off, as at save point 2.
FIX("S2", "func_start", function(start)
    return function(...)
        if EQ(W("save_point"), 3) then
            C.trigvolume_enable("tarmac", 0)
            C.trigvolume_enable("bomberboom", 0)
            C.trigvolume_enable("madbomberbait", 0)
        end
        return start(...)
    end
end)
