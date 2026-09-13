local M = {}

function M.new()
  return {
    sequence = 0,
    order = {},
    nodes = {},
  }
end

local function upsert(projection, node)
  if projection.nodes[node.id] == nil then
    table.insert(projection.order, node.id)
  end
  projection.nodes[node.id] = node
  return node.id
end

function M.apply(projection, change)
  if change.sequence ~= nil then
    local expected = projection.sequence + 1
    if change.sequence ~= expected then
      return nil, string.format("transcript sequence gap: expected %d, got %d", expected, change.sequence)
    end
    projection.sequence = change.sequence
  end

  if change.kind == "message" then
    return upsert(projection, {
      id = change.id,
      kind = "message",
      role = change.role,
      text = change.text or "",
    })
  end
  if change.kind == "text_delta" then
    local node = projection.nodes[change.id]
    if node == nil or node.kind ~= "message" then
      return nil, "text delta targets unknown message " .. tostring(change.id)
    end
    node.text = node.text .. (change.text or "")
    return node.id
  end
  if change.kind == "tool_call" then
    return upsert(projection, {
      id = change.call_id,
      kind = "tool",
      callable_id = change.callable_id,
      state = "running",
      input = change.input,
    })
  end
  if change.kind == "tool_result" or change.kind == "tool_failed" then
    local node = projection.nodes[change.call_id]
    if node == nil or node.kind ~= "tool" then
      return nil, "tool result targets unknown call " .. tostring(change.call_id)
    end
    node.state = change.kind == "tool_result" and "completed" or "failed"
    node.output = change.output or change.error
    return node.id
  end
  if change.kind == "progress" then
    return upsert(projection, {
      id = change.id,
      kind = "progress",
      message = change.message,
      fraction = change.fraction,
    })
  end
  return nil, "unsupported transcript change " .. tostring(change.kind)
end

return M
