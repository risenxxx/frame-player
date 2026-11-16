-- The second subtitle, drawn by mpv's OSD where its position is ours.
--
-- mpv's own drawing of `secondary-sid` cannot be given a top margin:
-- `secondary-sub-pos` hangs the block from a fixed bottom, so its distance
-- from the top edge changes with the number of lines (second-line.ts has the
-- measurements). So mpv is told not to draw it (`secondary-sub-visibility=no`)
-- and still selects and times it, reporting the text as `secondary-sub-text`;
-- this script draws that text as an ASS overlay anchored at its top (`\an8`)
-- at a position the player sends.
--
-- Drawn here rather than in the web view because a line that changes every few
-- seconds keeps WebKit's GPU process repainting, and that holds ~200 MB for as
-- long as the line is on. Here it is libass, the same renderer as the main
-- line, on mpv's core thread: no bridge latency, no lost property event, and
-- the line moves with the picture during a resize.
--
-- The overlay takes its look from mpv's OSD style, not from an ASS style of its
-- own (osd_libass.c: an external overlay's events use the "OSD" style built
-- from the `osd-*` options, and the border style has no override tag). The
-- player draws its own OSD and controls (`osc=no`, no default bindings), so the
-- `osd-*` style is free, and this script keeps it a copy of the `sub-*` one.
--
-- Messages (script-message-to secondsub …):
--   place <shown 0|1> <top> <x>  top: the block's top edge as a fraction of
--                                the window's height; x: its centre as a
--                                fraction of the window's width.

local overlay = mp.create_osd_overlay("ass-events")

local text = ""
local shown = false
local top = 0.03
local center_x = 0.5
-- The overlay's coordinate space: 720 high, as wide as the window's aspect —
-- what mpv uses for `res_y = 0`, set explicitly so `\pos` is in known units.
local res_y = 720
local res_x = 1280

local dims = nil

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
local FACTOR = { "scale", "scale-with-window", "scale-by-window", "use-margins" }
local sub = {}

local function escape(s)
    -- As mpv's own scripts do: a backslash is broken with a zero-width no-break
    -- space so it cannot start a tag, `{` cannot open an override block, and a
    -- line break is ASS's own (`\n` would split the overlay into events).
    s = s:gsub("\\", "\\\239\187\191")
    s = s:gsub("{", "\\{")
    s = s:gsub("\n", "\\N")
    return s
end

local function render()
    local t = text:gsub("^%s+", ""):gsub("%s+$", "")
    if not shown or t == "" then
        if overlay.data ~= "" then
            overlay.data = ""
            overlay:remove()
        end
        return
    end
    local x = res_x * center_x
    local y = res_y * top
    overlay.res_x = res_x
    overlay.res_y = res_y
    overlay.data = string.format("{\\an8\\pos(%.2f,%.2f)}%s", x, y, escape(t))
    overlay:update()
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

for _, name in ipairs(COPIED) do
    mp.observe_property("sub-" .. name, "native", function(_, value)
        if value ~= nil then
            mp.set_property_native("osd-" .. name, value)
            render()
        end
    end)
end
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
    render()
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

mp.register_script_message("place", function(s, t, x)
    shown = s == "1"
    top = tonumber(t) or top
    center_x = tonumber(x) or center_x
    render()
end)
