-- Atomic zoom+pan. Even back-to-back mpv_set_property calls from an external
-- thread produce an intermediate redraw (the VO gets in right after the first
-- property) and the picture jerks. A script-message handler runs entirely on
-- mpv's core thread, so no render can happen between the assignments.
-- (mpv-image-viewer uses the same trick for cursor-anchored zoom.)
mp.register_script_message("zoompan", function(z, x, y)
    mp.set_property_number("video-zoom", tonumber(z) or 0)
    mp.set_property_number("video-pan-x", tonumber(x) or 0)
    mp.set_property_number("video-pan-y", tonumber(y) or 0)
end)
