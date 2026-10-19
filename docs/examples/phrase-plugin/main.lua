-- Shows one phrase from this package's declared data for every played line.
-- Data arrives asynchronously as a `data` event; the first view has none yet.
local index, phrase, pending = 0, '…', nil

local function ask()
  pending = nana.data.request('phrases', {kind = 'slice', pointer = '/phrases', start = index, count = 1})
end

return {
  on_event = function(event)
    if event.type == 'init' then
      ask()
    elseif event.type == 'line_start' then
      index = index + 1
      ask()
    elseif event.type == 'data' then
      for _, result in ipairs(event.results) do
        if result.token == pending then
          if result.error then
            phrase = result.error
          elseif result.value[1] then
            phrase = result.value[1]
          else
            index = 0 -- past the last phrase: start over
            ask()
          end
        end
      end
    end
  end,
  view = function()
    return {views = {{slot = 'panel', root = {kind = 'text', id = 'phrase', text = phrase}}}}
  end,
}
