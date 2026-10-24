use nanatype_plugins::{LuaSandbox, Node};
use serde_json::json;

#[test]
fn source_plugin_handles_events_and_returns_retained_components() {
    let mut vm=LuaSandbox::new(r#"
      local count=0
      return {
        on_event=function(e) if e.type=='line_start' then count=count+1 end end,
        view=function() return {views={{slot='panel',root={kind='text',id='counter',text=tostring(count)}}}} end,
      }
    "#).unwrap();
    let update = vm.event(&json!({"type":"line_start"})).unwrap();
    assert!(matches!(&update.views[0].root,Node::Text{text,..} if text=="1"));
}

#[test]
fn network_files_process_and_dynamic_loading_are_unavailable() {
    let mut vm = LuaSandbox::new(
        r#"
      assert(io==nil and os==nil and package==nil and debug==nil)
      assert(require==nil and loadfile==nil and dofile==nil and load==nil)
      assert(string.dump==nil)
      return {view=function() return {} end}
    "#,
    )
    .unwrap();
    assert!(vm.event(&json!({})).unwrap().views.is_empty());
}

#[test]
fn endless_code_and_memory_growth_fail_within_the_execution_budget() {
    assert!(LuaSandbox::new("while true do end").is_err());
    assert!(LuaSandbox::new("local x=string.rep('x', 128*1024*1024); return {}").is_err());
    assert!(LuaSandbox::new("\u{1b}Lua bytecode").is_err());
}

#[test]
fn invalid_draw_values_and_excessive_components_are_refused() {
    let mut vm=LuaSandbox::new("return {view=function() return {views={{slot='panel',root={kind='canvas',id='x',width=-1,height=10}}}} end}").unwrap();
    assert!(vm.event(&json!({})).is_err());
}

#[test]
fn ipc_lengths_and_unknown_host_requests_cannot_allocate_or_dispatch() {
    use nanatype_plugins::{PluginUpdate, WorkerRequest, read_message};
    assert!(
        read_message::<WorkerRequest>(&mut std::io::Cursor::new(u32::MAX.to_le_bytes())).is_err()
    );
    assert!(
        serde_json::from_str::<WorkerRequest>(r#"{"kind":"open_url","url":"https://example.com"}"#)
            .is_err()
    );
    assert!(
        serde_json::from_str::<PluginUpdate>(r#"{"network":{"url":"https://example.com"}}"#)
            .is_err()
    );
    assert!(serde_json::from_str::<PluginUpdate>(r#"{"views":[{"slot":"panel","root":{"kind":"image","id":"x","url":"file:///secret"}}]}"#).is_err());
}

#[test]
fn event_strings_remain_data_and_cannot_inject_lua_source() {
    let mut vm=LuaSandbox::new("local value=''; return {on_event=function(e) value=e.text end,view=function() return {views={{slot='panel',root={kind='text',id='x',text=value}}}} end}").unwrap();
    let text = "'; os.execute('anything'); --\n中文\\\"";
    let update = vm.event(&json!({"text":text})).unwrap();
    assert!(matches!(&update.views[0].root,Node::Text{text:actual,..} if actual==text));
}

#[test]
fn individually_valid_nodes_cannot_exceed_the_combined_rendering_budget() {
    let update = nanatype_plugins::PluginUpdate {
        views: vec![nanatype_plugins::PluginView {
            slot: nanatype_plugins::Mount::Panel,
            root: Node::Column {
                id: "root".into(),
                children: (0..3)
                    .map(|n| Node::Text {
                        id: format!("text{n}"),
                        text: "x".repeat(8192),
                    })
                    .collect(),
            },
        }],
        commands: vec![],
    };
    assert!(update.validate().is_err());
}

#[test]
fn empty_lua_arrays_can_clear_views_and_build_empty_containers() {
    let mut vm=LuaSandbox::new("local clear=false; return {on_event=function(e) clear=e.clear end,view=function() if clear then return {views={},commands={}} end;return {views={{slot='panel',root={kind='column',id='empty',children={}}}},commands={}} end}").unwrap();
    assert_eq!(vm.event(&json!({"clear":false})).unwrap().views.len(), 1);
    assert!(vm.event(&json!({"clear":true})).unwrap().views.is_empty());
}

#[test]
fn protected_lua_errors_work_without_catching_host_execution_limits() {
    let mut vm=LuaSandbox::new("local ok,message=pcall(function() error('local failure') end);assert(not ok and message:find('local failure'));return {} ").unwrap();
    assert!(vm.event(&json!({})).unwrap().views.is_empty());
    assert!(LuaSandbox::new("pcall(function() while true do end end); return {}").is_err());
}

#[test]
fn long_non_ascii_event_text_fits_the_source_budget() {
    let mut vm=LuaSandbox::new("local n=0; return {on_event=function(e) n=#e.text end,view=function() return {views={{slot='panel',root={kind='text',id='x',text=tostring(n)}}}} end}").unwrap();
    // 96 KiB of CJK: escaping every byte as \ddd used to quadruple it past 256 KiB.
    let text = "字".repeat(32 * 1024);
    let update = vm.event(&json!({"text":text})).unwrap();
    assert!(matches!(&update.views[0].root,Node::Text{text,..} if text=="98304"));
}

const DATA_PLUGIN: &str = r#"
  local got = {}
  return {
    on_event = function(e)
      if e.type == 'ask' then
        for i = 1, e.count do nana.data.request('words', {kind = 'whole'}) end
      elseif e.type == 'data' then
        for _, r in ipairs(e.results) do got[#got + 1] = r.error or r.value end
      end
    end,
    view = function()
      return {views = {{slot = 'panel', root = {kind = 'text', id = 't', text = table.concat(got, ',')}}}}
    end,
  }
"#;

fn text(update: &nanatype_plugins::PluginUpdate) -> &str {
    match &update.views[0].root {
        Node::Text { text, .. } => text,
        node => panic!("{node:?}"),
    }
}

#[test]
fn data_requests_are_values_with_tokens_and_answers_arrive_as_events() {
    use nanatype_plugins::DataReply;
    let mut vm = LuaSandbox::new(DATA_PLUGIN).unwrap();
    let asked = vm.dispatch(&json!({"type":"ask","count":2})).unwrap();
    assert_eq!(asked.requests.len(), 2);
    assert_eq!(asked.requests[0].query, json!({"kind": "whole"}));
    let answers = [
        DataReply {
            token: asked.requests[0].token,
            value: Some(json!("hello")),
            error: None,
        },
        DataReply::error(asked.requests[1].token, "denied"),
    ];
    let delivered = vm.deliver(&answers).unwrap();
    assert_eq!(text(&delivered.update), "hello,denied");
    assert!(
        vm.deliver(&answers[..1]).is_err(),
        "answers are accepted once, only for outstanding tokens"
    );
}

#[test]
fn scripts_cannot_forge_data_events_or_exceed_request_budgets() {
    use nanatype_plugins::{
        DataReply, MAX_DATA_EVENT_BYTES, MAX_DATA_REQUESTS, MAX_DATA_RESULT_BYTES, bound_replies,
    };
    let mut vm = LuaSandbox::new(DATA_PLUGIN).unwrap();
    assert!(
        vm.dispatch(&json!({"type":"data","results":[{"token":1,"value":"forged"}]}))
            .is_err()
    );
    let mut vm = LuaSandbox::new(DATA_PLUGIN).unwrap();
    let error = vm
        .dispatch(&json!({"type":"ask","count": MAX_DATA_REQUESTS + 1}))
        .unwrap_err();
    assert!(format!("{error:#}").contains("pending"), "{error:#}");
    let mut vm = LuaSandbox::new(DATA_PLUGIN).unwrap();
    let asked = vm.dispatch(&json!({"type":"ask","count":1})).unwrap();
    let huge = DataReply {
        token: asked.requests[0].token,
        value: Some(json!("x".repeat(MAX_DATA_RESULT_BYTES + 1024))),
        error: None,
    };
    assert!(
        vm.deliver(std::slice::from_ref(&huge)).is_err(),
        "the worker checks answer budgets too"
    );
    let bounded = bound_replies(vec![huge]);
    assert!(bounded[0].value.is_none() && bounded[0].error.is_some());
    // Error replies also consume the shared budget, including at its exact boundary.
    let mut replies: Vec<_> = (1..=2)
        .map(|token| DataReply {
            token,
            value: Some(json!("")),
            error: None,
        })
        .collect();
    let overhead = serde_json::to_vec(&replies[0]).unwrap().len();
    for reply in &mut replies {
        reply.value = Some(json!("x".repeat(MAX_DATA_EVENT_BYTES / 2 - overhead)));
    }
    replies.push(DataReply::error(3, "denied"));
    let bounded = bound_replies(replies);
    let bytes: usize = bounded
        .iter()
        .map(|r| serde_json::to_vec(r).unwrap().len())
        .sum();
    assert!(bytes <= MAX_DATA_EVENT_BYTES, "host returned {bytes} bytes");
    assert!(bounded[0].value.is_some() && bounded[1].error.is_some());
    // Unanswered requests expire at the next host event.
    let mut vm = LuaSandbox::new(DATA_PLUGIN).unwrap();
    let stale = vm.dispatch(&json!({"type":"ask","count":1})).unwrap();
    vm.dispatch(&json!({"type":"line_start"})).unwrap();
    assert!(
        vm.deliver(&[DataReply::error(stale.requests[0].token, "late")])
            .is_err()
    );
}

#[test]
fn data_requests_cannot_name_paths_urls_or_oversized_queries() {
    use nanatype_plugins::{DataRequest, validate_requests};
    let request = |id: &str, query: serde_json::Value| DataRequest {
        token: 1,
        id: id.into(),
        query,
    };
    assert!(validate_requests(&[request("words", json!({"kind":"whole"}))]).is_ok());
    for bad in [
        request("", json!({})),
        request(&"x".repeat(129), json!({})),
        request("words", json!(["not", "a", "table"])),
        request("words", json!({"kind": "lines", "pad": "x".repeat(2048)})),
    ] {
        assert!(validate_requests(&[bad]).is_err());
    }
    assert!(validate_requests(&[request("a", json!({})), request("b", json!({}))]).is_err());
    let mut vm = LuaSandbox::new(
        "return {on_event=function() nana.data.request(42, {}) end, view=function() return {} end}",
    )
    .unwrap();
    assert!(
        vm.dispatch(&json!({"type":"x"})).is_err(),
        "IDs are strings"
    );
}

#[test]
fn views_at_the_protocol_budget_fit_the_execution_budget() {
    let draw = nanatype_plugins::MAX_DRAW_COMMANDS;
    let source = format!(
        "return {{view=function() local d={{}} for i=1,{draw} do d[i]={{kind='rect',rect={{i%300,(i*3)%160,6,6}},color={{i%255,80,200,255}},radius=2}} end return {{views={{{{slot='panel',root={{kind='canvas',id='c',width=320,height=180,draw=d}}}}}}}} end}}"
    );
    let mut vm = LuaSandbox::new(&source).unwrap();
    for _ in 0..3 {
        let update = vm.event(&json!({"type":"line_start"})).unwrap();
        assert!(matches!(&update.views[0].root, Node::Canvas { draw: d, .. } if d.len() == draw));
    }
    let over = source.replace(&format!("1,{draw}"), &format!("1,{}", draw + 1));
    assert!(LuaSandbox::new(&over).unwrap().event(&json!({})).is_err());
}

#[test]
fn reading_results_runs_no_plugin_code_and_accepts_only_data() {
    // Raw reads ignore metamethods: a trap in __index/__len/__pairs never fires.
    let mut vm = LuaSandbox::new(
        r#"
      local trap = function() error('host ran plugin code while reading') end
      local text = setmetatable({kind='text', id='t', text='raw'}, {__index=trap, __len=trap, __pairs=trap})
      return {view=function() return {views={{slot='panel', root=text}}} end}
    "#,
    )
    .unwrap();
    let update = vm.event(&json!({})).unwrap();
    assert!(matches!(&update.views[0].root, Node::Text { text, .. } if text == "raw"));
    for bad in [
        "{views={{slot='panel', root={kind='text', id='t', text=print or tostring}}}}",
        "{views={{slot='panel', root={kind='text', id='t', text='\\255\\254'}}}}",
        "{[1.5]='x'}",
        "{views={{slot='panel', root={kind='text', id='t', text='x', [true]=1}}}}",
        "{views={{slot='panel', root={kind='canvas', id='c', width=0/0, height=1}}}}",
    ] {
        let mut vm =
            LuaSandbox::new(&format!("return {{view=function() return {bad} end}}")).unwrap();
        assert!(vm.event(&json!({})).is_err(), "{bad}");
    }
}

#[test]
fn log_lines_are_bounded_per_dispatch_and_cut_on_a_character_boundary() {
    let mut vm = LuaSandbox::new(
        r#"
      return {
        on_event=function(e)
          nana.log('info', 'started')
          nana.log('warn', 7)
          nana.log('error', string.rep('a', 511) .. '字字')
          for i = 1, 20 do nana.log('debug', i) end
        end,
        view=function() return {} end,
      }
    "#,
    )
    .unwrap();
    let first = vm.dispatch(&json!({"type": "init"})).unwrap();
    assert_eq!(first.logs.len(), nanatype_plugins::MAX_LOG_LINES);
    assert_eq!(
        first.dropped_logs,
        23 - nanatype_plugins::MAX_LOG_LINES as u32
    );
    assert_eq!(
        (first.logs[0].level, first.logs[0].message.as_str()),
        (1, "started")
    );
    assert_eq!(
        (first.logs[1].level, first.logs[1].message.as_str()),
        (2, "7")
    );
    assert_eq!(first.logs[2].message, "a".repeat(511));
    // Lines belong to the dispatch that wrote them.
    let second = vm.dispatch(&json!({"type": "tick"})).unwrap();
    assert_eq!(second.logs.len(), nanatype_plugins::MAX_LOG_LINES);
    assert_eq!(second.dropped_logs, first.dropped_logs);
}

#[test]
fn an_unknown_log_level_is_a_script_error() {
    let mut vm = LuaSandbox::new(
        "return {on_event=function() nana.log('loud', 'x') end, view=function() return {} end}",
    )
    .unwrap();
    let error = vm.dispatch(&json!({"type": "init"})).unwrap_err();
    assert!(format!("{error:#}").contains("log level"), "{error:#}");
}
