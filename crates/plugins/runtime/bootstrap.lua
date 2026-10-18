assert(type(plugin) == 'table', 'plugin must return a table')
local function quote(s)
  return '"' .. s:gsub('[%z\1-\31\\"]', function(c)
    return string.format('\\u%04x', string.byte(c))
  end) .. '"'
end
local arrays = {views=true, commands=true, children=true, draw=true, points=true, rect=true, color=true}
local function encode(v, depth, key)
  assert(depth < 40, 'response nesting limit')
  local t = type(v)
  if t == 'nil' then return 'null' end
  if t == 'boolean' or t == 'number' then return tostring(v) end
  if t == 'string' then return quote(v) end
  assert(t == 'table', 'only data values may cross the host boundary')
  local parts = {}
  if #v > 0 or arrays[key] then
    for i = 1, #v do parts[i] = encode(v[i], depth + 1) end
    return '[' .. table.concat(parts, ',') .. ']'
  end
  for k, value in pairs(v) do
    assert(type(k) == 'string', 'object keys must be strings')
    parts[#parts + 1] = quote(k) .. ':' .. encode(value, depth + 1, k)
  end
  return '{' .. table.concat(parts, ',') .. '}'
end
function __nana_dispatch(event)
  if plugin.on_event then plugin.on_event(event) end
  return encode(plugin.view and plugin.view() or {}, 0)
end
return ''
