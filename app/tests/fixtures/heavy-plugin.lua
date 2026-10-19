local tick = 0
return {
  on_event = function(e) tick = tick + 1 end,
  view = function()
    local draw = {}
    for i = 1, 2047 do
      local x = (i * 7 + tick) % 300
      draw[i] = {kind = 'rect', rect = {x, (i * 3) % 160, 6, 6}, color = {i % 255, 80, 200, 255}, radius = 2}
    end
    local rows = {}
    for i = 1, 60 do
      rows[i] = {kind = 'text', id = 'row' .. i, text = 'row ' .. i .. ' · ' .. tick}
    end
    return {views = {{slot = 'panel', root = {kind = 'column', id = 'root', children = {
      {kind = 'canvas', id = 'c', width = 320, height = 180, draw = draw},
      {kind = 'column', id = 'rows', children = rows},
    }}}}}
  end,
}
