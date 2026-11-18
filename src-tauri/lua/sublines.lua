-- The two subtitle lines, as far as they are the player's: the second one
-- drawn on mpv's OSD where its position is ours, and where both of them are on
-- screen, so the player can lay a grab handle over each.
--
-- THE SECOND LINE. mpv's own drawing of `secondary-sid` cannot be given a top
-- margin: `secondary-sub-pos` hangs the block from a fixed bottom, so its
-- distance from the top edge changes with the number of lines (second-line.ts
-- has the measurements). So mpv is told not to draw it
-- (`secondary-sub-visibility=no`) and still selects and times it, reporting the
-- text as `secondary-sub-text`; this script draws that text as an ASS overlay
-- anchored at one edge (`\an8` / `\an2` + `\pos`) at a place the player sends.
--
-- Drawn here rather than in the web view because a line that changes every few
-- seconds keeps WebKit's GPU process repainting, which holds ~200 MB for part of
-- the time. Here it is libass, the same renderer as the main line, on mpv's
-- core thread: no bridge latency, no lost property event, and the line moves
-- with the picture during a resize.
--
-- The overlay takes its look from mpv's OSD style, not from an ASS style of its
-- own (osd_libass.c: an external overlay's events use the "OSD" style built
-- from the `osd-*` options, and the border style has no override tag). The
-- player draws its own OSD and controls (`osc=no`, no default bindings), so the
-- `osd-*` style is free, and this script keeps it a copy of the `sub-*` one.
--
-- THE MAIN LINE stays mpv's own drawing — frame export keeps it, and ASS and
-- image subtitles have nothing else to be drawn by. Only where it is gets
-- measured: the same text set in a hidden overlay at the place libass puts a
-- plain-text subtitle (see `measure_main`), which is pixel for pixel the line
-- itself, since the overlay's style is the subtitles' own.
--
-- Messages (script-message-to sublines …):
--   place <shown 0|1> <x> <y> <edge>
--       The second line. x: the block's centre as a fraction of the window's
--       width; y: its `edge` (top|bottom) as a fraction of the window's height.
--       A block put in the lower half hangs from its bottom edge, so a cue with
--       more lines grows away from the frame's edge rather than off it.
--
-- What it reports, as fractions of the window, "" while there is nothing:
--   user-data/sublines/second  "x0 y0 x1 y1" — the second line's bounds.
--   user-data/sublines/main    "x0 y0 x1 y1 pos range" — the main line's bounds
--       for a plain-text track, the `sub-pos` it was measured at, and how much
--       of the window's height 100 units of `sub-pos` span.

local overlay = mp.create_osd_overlay("ass-events")
local probe = mp.create_osd_overlay("ass-events")
probe.hidden = true
probe.compute_bounds = true

local text = ""
local shown = false
local center_x = 0.5
local edge_y = 0.03
local edge = "top"
-- The overlays' coordinate space: 720 high, as wide as the window's aspect —
-- what mpv uses for `res_y = 0`, set explicitly so `\pos` is in known units.
local res_y = 720
local res_x = 1280

local dims = nil
local reported = {}

-- The `sub-*` style copied onto the OSD's. Names as mpv 0.41 spells them
-- (`*-border-*` are aliases of `*-outline-*`).
local COPIED = {
    "font", "bold", "italic", "outline-color", "border-style", "color",
    "back-color", "blur", "justify", "margin-x",
}
-- Copied times the subtitle's scale: `osd-scale` reaches only mpv's own OSD
-- text, never an external overlay (osd_libass.c), so the factor goes into the
-- sizes themselves.
local SCALED = { "font-size", "outline-size", "shadow-offset", "spacing" }
-- What the factor is made of. sd_ass.c, for a text subtitle: `sub-scale`,
-- times window height over picture height unless `sub-scale-with-window`, and
-- over 720 / window height unless `sub-scale-by-window`. The overlay is laid
-- out against the window's height already, so each "no" undoes part of that.
-- The rest is where the main line goes.
local FACTOR = {
    "scale", "scale-with-window", "scale-by-window", "use-margins", "margin-y",
    "pos", "visibility",
}
local sub = {}

-- The main track's codec, and the ones mpv turns into plain text in its own
-- style — the tracks whose place this script can predict. ASS lays itself out,
-- and an image or a closed caption is not text at all.
local main_codec = nil
local main_text = ""
local TEXT_CODECS = {
    subrip = true, srt = true, webvtt = true, mov_text = true, text = true,
    microdvd = true, subviewer = true, subviewer1 = true, sami = true,
    realtext = true, mpl2 = true, pjs = true, vplayer = true, jacosub = true,
    stl = true,
}

local function escape(s)
    -- As mpv's own scripts do: a backslash is broken with a zero-width no-break
    -- space so it cannot start a tag, `{` cannot open an override block, and a
    -- line break is ASS's own (`\n` would split the overlay into events).
    s = s:gsub("\\", "\\\239\187\191")
    s = s:gsub("{", "\\{")
    s = s:gsub("\n", "\\N")
    return s
end

local function trim(s)
    return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end

local function report(name, value)
    if reported[name] ~= value then
        reported[name] = value
        mp.set_property("user-data/sublines/" .. name, value)
    end
end

local function bounds(r)
    if r and r.x0 and r.x1 and r.x1 > r.x0 and r.y1 > r.y0 then
        return r.x0 / res_x, r.y0 / res_y, r.x1 / res_x, r.y1 / res_y
    end
    return nil
end

local function factor()
    local f = tonumber(sub["scale"]) or 1
    if dims and dims.h > 0 then
        local video_h = dims.h - (dims.mt or 0) - (dims.mb or 0)
        if sub["scale-with-window"] == false and video_h > 0 then
            -- Against the picture rather than the window, as mpv's
            -- get_libass_scale_height has it.
            local scale_h = video_h
            if sub["use-margins"] then
                local video_w = dims.w - (dims.ml or 0) - (dims.mr or 0)
                if video_w >= 1 then
                    scale_h = math.min(dims.h, dims.w / video_w * video_h)
                end
            end
            f = f * scale_h / dims.h
        end
        if sub["scale-by-window"] == false then
            f = f * 720 / dims.h
        end
    end
    return f
end

local function apply_scaled()
    local f = factor()
    for _, name in ipairs(SCALED) do
        local v = tonumber(sub[name])
        if v then mp.set_property_number("osd-" .. name, v * f) end
    end
end

local function render_second()
    local t = trim(text)
    if not shown or t == "" then
        if overlay.data ~= "" then
            overlay.data = ""
            overlay:remove()
        end
        report("second", "")
        return
    end
    overlay.res_x = res_x
    overlay.res_y = res_y
    overlay.compute_bounds = true
    local an = edge == "bottom" and 2 or 8
    overlay.data = string.format("{\\an%d\\pos(%.2f,%.2f)}%s", an, res_x * center_x, res_y * edge_y, escape(t))
    local x0, y0, x1, y1 = bounds(overlay:update())
    report("second", x0 and string.format("%.5f %.5f %.5f %.5f", x0, y0, x1, y1) or "")
end

-- Where libass puts a plain-text subtitle (ass_render.c, a bottom-aligned
-- event with a line position): the bottom of its line box at
--   frame top + (frame height − margin) · sub-pos / 100,
-- the margin being `sub-margin-y` in 720-high units scaled like the font
-- (measured on 0.41: exact to 2 px at margins 0–60, `sub-scale` 1.5 and a
-- 900-high window). The frame is the window — `sub-use-margins`, mpv's default
-- — or the picture without it. Past the top the block is clamped to it.
local function measure_main()
    local t = trim(main_text)
    local pos = tonumber(sub["pos"]) or 100
    if t == "" or not dims or sub["visibility"] == false or not TEXT_CODECS[main_codec or ""] then
        report("main", "")
        return
    end
    local top, h = 0, dims.h
    if sub["use-margins"] == false then
        top = dims.mt or 0
        h = dims.h - top - (dims.mb or 0)
    end
    local margin = (tonumber(sub["margin-y"]) or 34) * factor() * dims.h / 720
    local range = h - margin
    local bottom = (top + range * math.min(pos, 150) / 100) / dims.h
    -- Measured where nothing can clip it — bounds are cut at the window's
    -- edges, and a block pushed against the top would come back short — and
    -- then moved to where the line is.
    local probe_at = 0.75
    probe.res_x = res_x
    probe.res_y = res_y
    probe.data = string.format("{\\an2\\pos(%.2f,%.2f)}%s", res_x / 2, probe_at * res_y, escape(t))
    local x0, y0, x1, y1 = bounds(probe:update())
    if not x0 then
        report("main", "")
        return
    end
    local height = y1 - y0
    y1 = bottom + (y1 - probe_at)
    y0 = y1 - height
    -- libass stops a line pushed past the top at the frame's top edge.
    local floor = top / dims.h
    if y0 < floor then
        y0 = floor
        y1 = floor + height
    end
    report("main", string.format("%.5f %.5f %.5f %.5f %.3f %.5f", x0, y0, x1, y1, pos, range / dims.h))
end

local function render()
    render_second()
    measure_main()
end

for _, name in ipairs(COPIED) do
    mp.observe_property("sub-" .. name, "native", function(_, value)
        if value ~= nil then
            mp.set_property_native("osd-" .. name, value)
            render()
        end
    end)
end
-- The fonts the viewer imported (subtitle_fonts.rs) are a folder of the
-- subtitles' renderer, and the OSD has its own: without the copy a family
-- picked from that folder draws the main line and not the second, and the
-- probe measures the main line in a fallback font. An overlay reads the folder
-- once, when its renderer is made (osd_libass.c), so a change removes both —
-- the next update makes them again with the folder as it is now.
mp.observe_property("sub-fonts-dir", "string", function(_, value)
    if value == nil then return end
    mp.set_property("osd-fonts-dir", value)
    overlay:remove()
    probe:remove()
    render()
end)

for _, list in ipairs({ SCALED, FACTOR }) do
    for _, name in ipairs(list) do
        mp.observe_property("sub-" .. name, "native", function(_, value)
            sub[name] = value
            apply_scaled()
            render()
        end)
    end
end

mp.observe_property("secondary-sub-text", "string", function(_, value)
    text = value or ""
    render_second()
end)

mp.observe_property("sub-text", "string", function(_, value)
    main_text = value or ""
    measure_main()
end)

mp.observe_property("current-tracks/sub/codec", "string", function(_, value)
    main_codec = value
    measure_main()
end)

mp.observe_property("osd-dimensions", "native", function(_, dim)
    if dim and dim.w and dim.h and dim.w > 0 and dim.h > 0 then
        dims = dim
        local aspect = dim.w / dim.h
        if dim.par and dim.par > 0 then aspect = aspect / dim.par end
        res_x = math.floor(res_y * aspect + 0.5)
        apply_scaled()
        render()
    end
end)

mp.register_script_message("place", function(s, x, y, e)
    shown = s == "1"
    center_x = tonumber(x) or center_x
    edge_y = tonumber(y) or edge_y
    edge = e == "bottom" and "bottom" or "top"
    render_second()
end)

report("second", "")
report("main", "")
