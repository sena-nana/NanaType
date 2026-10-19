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
