local count = 0
local function component()
    return {
        kind = 'column', id = 'counter', children = {
            {kind = 'text', id = 'label', text = 'Lines played: ' .. count},
            {kind = 'canvas', id = 'meter', width = 240, height = 24, draw = {
                {kind = 'rect', rect = {0, 0, 240, 24}, color = {225, 230, 235, 255}, radius = 8},
                {kind = 'rect', rect = {0, 0, math.min(count, 20) * 12, 24}, color = {60, 150, 210, 255}, radius = 8},
            }},
            {kind = 'button', id = 'reset-button', text = 'Reset', command = 'reset'},
        },
    }
end
return {
    on_event = function(event)
        if event.type == 'line_start' then count = count + 1 end
        if event.type == 'command' and event.id == 'reset' then count = 0 end
    end,
    view = function()
        return {
            commands = {{id = 'reset', label = 'Reset counter'}},
            views = {{slot = 'panel', root = component()}},
        }
    end,
}
