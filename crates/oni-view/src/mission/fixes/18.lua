-- Level 18 (tctf_ii): our fixes to the shipped scripts (docs/SCRIPT_FIXES.md).

-- S3: past door 14, its lock behind Konoko comes while the door may still be
-- open; she can walk back out and be locked out. Fixed: closed, then locked.
FIX("S3", "t45", function(t45)
    return function(...)
        C.door_close(14)
        return t45(...)
    end
end)
