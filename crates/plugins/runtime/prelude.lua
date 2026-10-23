-- Defined before the plugin chunk runs. Every limit here is checked again by the host.
__nana_data = {pending = {}, outbox = {}, next = 0}
nana = {data = {}}
function nana.data.request(id, query)
  assert(type(id) == 'string' and #id > 0 and #id <= 128, 'data ID must be a short string')
  assert(type(query) == 'table', 'data query must be a table')
  local count = 0
  for _ in pairs(__nana_data.pending) do count = count + 1 end
  assert(count < 8, 'at most 8 pending data requests')
  __nana_data.next = __nana_data.next + 1
  local token = __nana_data.next
  __nana_data.pending[token] = true
  __nana_data.outbox[#__nana_data.outbox + 1] = {token = token, id = id, query = query}
  return token
end

-- Lines for the host's log, collected per dispatch; see docs/plugins.md.
__nana_log = {lines = {}, dropped = 0}
local log_levels = {debug = 0, info = 1, warn = 2, error = 3}
function nana.log(level, message)
  local code = log_levels[level]
  assert(code ~= nil, "log level must be 'debug', 'info', 'warn' or 'error'")
  message = tostring(message)
  if #message > 512 then
    -- Cut on a character boundary: never before a UTF-8 continuation byte.
    local n = 512
    while n > 0 and (string.byte(message, n + 1) & 0xC0) == 0x80 do n = n - 1 end
    message = string.sub(message, 1, n)
  end
  if #__nana_log.lines >= 16 then
    __nana_log.dropped = __nana_log.dropped + 1
    return
  end
  __nana_log.lines[#__nana_log.lines + 1] = {level = code, message = message}
end
