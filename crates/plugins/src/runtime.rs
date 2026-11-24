//! Only the application's pinned interpreter is instantiated, never plugin Wasm.
use crate::{
    DataReply, DataRequest, MAX_DATA_EVENT_BYTES, MAX_DATA_REQUESTS, MAX_DATA_RESULT_BYTES,
    MAX_MESSAGE_BYTES, MAX_SCRIPT_BYTES, PluginLog, PluginUpdate,
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Duration,
};
use wasmtime::{
    Caller, Config, Engine, Extern, ExternType, Instance, Linker, Memory, Module, Store,
    StoreLimits, StoreLimitsBuilder, TypedFunc, Val,
};
use wasmtime::{Result, bail, format_err};

const COMPILED_VM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/lua.cwasm"));
const PRELUDE: &str = include_str!("../runtime/prelude.lua");
const BOOTSTRAP: &str = include_str!("../runtime/bootstrap.lua");
const FUEL: u64 = 20_000_000;
/// Reading a result is host work bounded by `MAX_RESULT_VALUES`; it gets its
/// own budget so the plugin's compute limit stays `FUEL`.
const READ_FUEL: u64 = 50_000_000;
const MEMORY: usize = 32 * 1024 * 1024;
const INITIAL_SCRATCH: usize = 4096;

#[derive(Debug)]
struct LuaLongjmp;
impl std::fmt::Display for LuaLongjmp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Lua protected exception")
    }
}
impl std::error::Error for LuaLongjmp {}

/// Raw Lua C API entry points used to push events and read results without
/// per-event Lua parsing or compilation.
struct Api {
    malloc: TypedFunc<i32, i32>,
    free: TypedFunc<i32, ()>,
    kind: TypedFunc<(i32, i32), i32>,
    next: TypedFunc<(i32, i32), i32>,
    rawlen: TypedFunc<(i32, i32), i64>,
    rawgeti: TypedFunc<(i32, i32, i64), i32>,
    isinteger: TypedFunc<(i32, i32), i32>,
    tointegerx: TypedFunc<(i32, i32, i32), i64>,
    tonumberx: TypedFunc<(i32, i32, i32), f64>,
    toboolean: TypedFunc<(i32, i32), i32>,
    tolstring: TypedFunc<(i32, i32, i32), i32>,
    absindex: TypedFunc<(i32, i32), i32>,
    settop: TypedFunc<(i32, i32), ()>,
    checkstack: TypedFunc<(i32, i32), i32>,
    pushvalue: TypedFunc<(i32, i32), ()>,
    pushnil: TypedFunc<i32, ()>,
    pushboolean: TypedFunc<(i32, i32), ()>,
    pushinteger: TypedFunc<(i32, i64), ()>,
    pushnumber: TypedFunc<(i32, f64), ()>,
    pushlstring: TypedFunc<(i32, i32, i32), i32>,
    createtable: TypedFunc<(i32, i32, i32), ()>,
    rawset: TypedFunc<(i32, i32), ()>,
    rawseti: TypedFunc<(i32, i32, i64), ()>,
    pcallk: TypedFunc<(i32, i32, i32, i32, i32, i32), i32>,
}

impl Api {
    fn new(instance: &Instance, store: &mut Store<StoreLimits>) -> Result<Self> {
        Ok(Self {
            malloc: instance.get_typed_func(&mut *store, "malloc")?,
            free: instance.get_typed_func(&mut *store, "free")?,
            kind: instance.get_typed_func(&mut *store, "lua_type")?,
            next: instance.get_typed_func(&mut *store, "lua_next")?,
            rawlen: instance.get_typed_func(&mut *store, "lua_rawlen")?,
            rawgeti: instance.get_typed_func(&mut *store, "lua_rawgeti")?,
            isinteger: instance.get_typed_func(&mut *store, "lua_isinteger")?,
            tointegerx: instance.get_typed_func(&mut *store, "lua_tointegerx")?,
            tonumberx: instance.get_typed_func(&mut *store, "lua_tonumberx")?,
            toboolean: instance.get_typed_func(&mut *store, "lua_toboolean")?,
            tolstring: instance.get_typed_func(&mut *store, "lua_tolstring")?,
            absindex: instance.get_typed_func(&mut *store, "lua_absindex")?,
            settop: instance.get_typed_func(&mut *store, "lua_settop")?,
            checkstack: instance.get_typed_func(&mut *store, "lua_checkstack")?,
            pushvalue: instance.get_typed_func(&mut *store, "lua_pushvalue")?,
            pushnil: instance.get_typed_func(&mut *store, "lua_pushnil")?,
            pushboolean: instance.get_typed_func(&mut *store, "lua_pushboolean")?,
            pushinteger: instance.get_typed_func(&mut *store, "lua_pushinteger")?,
            pushnumber: instance.get_typed_func(&mut *store, "lua_pushnumber")?,
            pushlstring: instance.get_typed_func(&mut *store, "lua_pushlstring")?,
            createtable: instance.get_typed_func(&mut *store, "lua_createtable")?,
            rawset: instance.get_typed_func(&mut *store, "lua_rawset")?,
            rawseti: instance.get_typed_func(&mut *store, "lua_rawseti")?,
            pcallk: instance.get_typed_func(&mut *store, "lua_pcallk")?,
        })
    }
}

/// Keys whose tables are always arrays, even when empty.
const ARRAY_KEYS: &[&str] = &[
    "requests", "logs", "views", "commands", "children", "draw", "points", "rect", "color",
];
/// Values one result may contain; the protocol's own limits are far lower.
const MAX_RESULT_VALUES: usize = 200_000;
const MAX_RESULT_DEPTH: usize = 40;

pub struct LuaSandbox {
    store: Store<StoreLimits>,
    instance: Instance,
    memory: Memory,
    api: Api,
    state: i32,
    len_ptr: i32,
    scratch_ptr: i32,
    scratch_cap: usize,
    epoch_gate: Arc<EpochGate>,
    /// Data request tokens awaiting an answer from the host.
    pending: BTreeSet<u32>,
}

/// What one dispatch produced: the view, any data requests and `nana.log` lines.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dispatch {
    pub update: PluginUpdate,
    #[serde(default)]
    pub requests: Vec<DataRequest>,
    #[serde(default)]
    pub logs: Vec<PluginLog>,
    /// Lines past the per-dispatch budget.
    #[serde(default)]
    pub dropped_logs: u32,
}

struct EpochGate {
    active_calls: Mutex<usize>,
    wake: Condvar,
}

struct EpochGuard(Arc<EpochGate>);
impl EpochGuard {
    fn enter(gate: Arc<EpochGate>) -> Self {
        {
            let mut active = gate.active_calls.lock().unwrap_or_else(|e| e.into_inner());
            *active += 1;
            if *active == 1 {
                gate.wake.notify_one();
            }
        }
        Self(gate)
    }
}
impl Drop for EpochGuard {
    fn drop(&mut self) {
        let mut active = self
            .0
            .active_calls
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *active = active.saturating_sub(1);
    }
}

struct Runtime {
    engine: Engine,
    module: Module,
    epoch_gate: Arc<EpochGate>,
}

fn engine() -> Result<&'static Runtime> {
    static ENGINE: OnceLock<std::result::Result<Runtime, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let build = || -> Result<Runtime> {
                let mut config = Config::new();
                config.consume_fuel(true);
                config.epoch_interruption(true);
                let engine = Engine::new(&config)?;
                let module = fixed_module(&engine)?;
                let epoch_gate = Arc::new(EpochGate {
                    active_calls: Mutex::new(0),
                    wake: Condvar::new(),
                });
                let clock = engine.clone();
                let gate = epoch_gate.clone();
                std::thread::Builder::new()
                    .name("plugin-deadline".into())
                    .spawn(move || {
                        loop {
                            let mut active =
                                gate.active_calls.lock().unwrap_or_else(|e| e.into_inner());
                            while *active == 0 {
                                active = gate.wake.wait(active).unwrap_or_else(|e| e.into_inner());
                            }
                            let (next, _) = gate
                                .wake
                                .wait_timeout(active, Duration::from_millis(2))
                                .unwrap_or_else(|e| e.into_inner());
                            let tick = *next > 0;
                            drop(next);
                            if tick {
                                clock.increment_epoch();
                            }
                        }
                    })?;
                Ok(Runtime {
                    engine,
                    module,
                    epoch_gate,
                })
            };
            build().map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| format_err!("{e}"))
}

#[allow(unsafe_code)]
fn fixed_module(engine: &Engine) -> Result<Module> {
    // SAFETY: bytes are embedded directly from this crate's build.rs, which
    // verifies the pinned Wasm SHA-256 and compiles it for TARGET using the
    // locked Wasmtime version and identical fuel/epoch configuration. No
    // filesystem cache, package data, plugin input or IPC bytes reach this API.
    unsafe { Module::deserialize(engine, COMPILED_VM) }
}

impl LuaSandbox {
    pub fn new(source: &str) -> Result<Self> {
        Self::create(source, true)
    }
    /// Compile with the pinned interpreter without executing the supplied chunk.
    pub fn check_source(source: &str) -> Result<()> {
        Self::create(source, false).map(|_| ())
    }
    fn create(source: &str, execute: bool) -> Result<Self> {
        if source.len() > MAX_SCRIPT_BYTES || source.as_bytes().first() == Some(&0x1b) {
            bail!("only bounded Lua source is accepted");
        }
        let runtime = engine()?;
        let _epoch = EpochGuard::enter(runtime.epoch_gate.clone());
        let limits = StoreLimitsBuilder::new()
            .memory_size(MEMORY)
            .instances(1)
            .tables(1)
            .memories(1)
            .build();
        let mut store = Store::new(&runtime.engine, limits);
        store.limiter(|s| s);
        store.set_fuel(FUEL * 5)?;
        store.set_epoch_deadline(250);
        let mut linker = Linker::new(&runtime.engine);
        for import in runtime.module.imports() {
            let ExternType::Func(ty) = import.ty() else {
                bail!("unexpected interpreter import");
            };
            let name = import.name().to_owned();
            // This is not a WASI linker: filesystem/process imports always trap.
            linker.func_new(
                import.module(),
                import.name(),
                ty,
                move |mut caller: Caller<'_, StoreLimits>, params, results| {
                    match name.as_str() {
                        "invoke_vii" => {
                            let saved = caller
                                .get_export("stackSave")
                                .and_then(Extern::into_func)
                                .ok_or_else(|| format_err!("missing stack save"))?
                                .typed::<(), i32>(&caller)?
                                .call(&mut caller, ())?;
                            let table = caller
                                .get_export("__indirect_function_table")
                                .and_then(Extern::into_table)
                                .ok_or_else(|| format_err!("missing function table"))?;
                            let function = table
                                .get(&mut caller, params[0].i32().unwrap() as u64)
                                .and_then(|v| v.as_func().flatten().copied())
                                .ok_or_else(|| format_err!("invalid callback"))?;
                            if let Err(error) = function.call(&mut caller, &params[1..], &mut []) {
                                // Match the pinned Emscripten invoke_vii contract.
                                // Fuel, deadline, denied imports and native traps
                                // never become catchable Lua exceptions.
                                if !error.is::<LuaLongjmp>() {
                                    return Err(error);
                                }
                                caller
                                    .get_export("stackRestore")
                                    .and_then(Extern::into_func)
                                    .ok_or_else(|| format_err!("missing stack restore"))?
                                    .typed::<i32, ()>(&caller)?
                                    .call(&mut caller, saved)?;
                                caller
                                    .get_export("setThrew")
                                    .and_then(Extern::into_func)
                                    .ok_or_else(|| format_err!("missing exception bridge"))?
                                    .typed::<(i32, i32), ()>(&caller)?
                                    .call(&mut caller, (1, 0))?;
                            }
                        }
                        "_emscripten_throw_longjmp" => {
                            return Err(wasmtime::Error::new(LuaLongjmp));
                        }
                        "emscripten_resize_heap" => {
                            results[0] = Val::I32(0);
                        }
                        "environ_sizes_get" => {
                            let memory = caller
                                .get_export("memory")
                                .and_then(Extern::into_memory)
                                .ok_or_else(|| format_err!("missing memory"))?;
                            for p in params {
                                memory.write(&mut caller, p.i32().unwrap() as usize, &[0; 4])?;
                            }
                            results[0] = Val::I32(0);
                        }
                        "environ_get" => {
                            results[0] = Val::I32(0);
                        }
                        "emscripten_date_now" => {
                            results[0] = Val::F64(0f64.to_bits());
                        }
                        "_emscripten_get_now_is_monotonic" => {
                            results[0] = Val::I32(1);
                        }
                        _ => bail!("denied interpreter import: {name}"),
                    }
                    Ok(())
                },
            )?;
        }
        let instance = linker.instantiate(&mut store, &runtime.module)?;
        instance
            .get_typed_func::<(), ()>(&mut store, "emscripten_stack_init")?
            .call(&mut store, ())?;
        instance
            .get_typed_func::<(), ()>(&mut store, "__wasm_call_ctors")?
            .call(&mut store, ())?;
        let state = instance
            .get_typed_func::<(), i32>(&mut store, "luaL_newstate")?
            .call(&mut store, ())?;
        if state == 0 {
            bail!("Lua allocation failed");
        }
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| format_err!("missing interpreter memory"))?;
        let api = Api::new(&instance, &mut store)?;
        let len_ptr = api.malloc.call(&mut store, 4)?;
        let scratch_ptr = api.malloc.call(&mut store, INITIAL_SCRATCH as i32)?;
        if len_ptr == 0 || scratch_ptr == 0 {
            bail!("interpreter memory exhausted");
        }
        let mut vm = Self {
            store,
            instance,
            memory,
            api,
            state,
            len_ptr,
            scratch_ptr,
            scratch_cap: INITIAL_SCRATCH,
            epoch_gate: runtime.epoch_gate.clone(),
            pending: BTreeSet::new(),
        };
        for (name, global) in [
            ("luaopen_base", "_G"),
            ("luaopen_table", "table"),
            ("luaopen_string", "string"),
            ("luaopen_math", "math"),
            ("luaopen_utf8", "utf8"),
        ] {
            vm.instance
                .get_typed_func::<i32, i32>(&mut vm.store, name)?
                .call(&mut vm.store, state)?;
            let pointer = vm.allocate(global.as_bytes(), true)?;
            vm.instance
                .get_typed_func::<(i32, i32), ()>(&mut vm.store, "lua_setglobal")?
                .call(&mut vm.store, (state, pointer))?;
            vm.free(pointer)?;
        }
        vm.eval("io=nil; os=nil; package=nil; debug=nil; require=nil; dofile=nil; loadfile=nil; load=nil; print=nil; warn=nil; collectgarbage=nil; string.dump=nil; return ''")?;
        vm.eval(&format!("{PRELUDE}\nreturn ''"))?;
        let script = format!("local plugin=(function()\n{source}\nend)()\n{BOOTSTRAP}");
        if execute {
            // Leaves the precompiled `__nana_dispatch` closure at stack index 1.
            vm.call(&script)?;
        } else {
            vm.load(source)?;
        }
        Ok(vm)
    }
    fn allocate(&mut self, bytes: &[u8], zero: bool) -> Result<i32> {
        let len = bytes.len() + usize::from(zero);
        let p = self.api.malloc.call(&mut self.store, len as i32)?;
        if p == 0 {
            bail!("interpreter memory exhausted");
        }
        self.memory.write(&mut self.store, p as usize, bytes)?;
        if zero {
            self.memory
                .write(&mut self.store, p as usize + bytes.len(), &[0])?;
        }
        Ok(p)
    }
    fn free(&mut self, p: i32) -> Result<()> {
        self.api.free.call(&mut self.store, p)?;
        Ok(())
    }
    /// Load `source`, leaving its compiled chunk on the Lua stack.
    fn load(&mut self, source: &str) -> Result<()> {
        self.store.set_fuel(FUEL)?;
        self.store.set_epoch_deadline(25);
        self.api.settop.call(&mut self.store, (self.state, 0))?;
        let pointer = self.allocate(source.as_bytes(), false)?;
        let name = self.allocate(b"plugin", true)?;
        let mode = self.allocate(b"t", true)?;
        let status = self
            .instance
            .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut self.store, "luaL_loadbufferx")?
            .call(
                &mut self.store,
                (self.state, pointer, source.len() as i32, name, mode),
            )?;
        self.free(pointer)?;
        self.free(name)?;
        self.free(mode)?;
        if status != 0 {
            let detail = self.error_detail()?;
            bail!("invalid Lua source: {detail}");
        }
        Ok(())
    }
    fn call(&mut self, source: &str) -> Result<()> {
        self.load(source)?;
        let status = self
            .api
            .pcallk
            .call(&mut self.store, (self.state, 0, 1, 0, 0, 0))?;
        if status != 0 {
            let detail = self.error_detail()?;
            bail!("Lua execution failed: {detail}");
        }
        Ok(())
    }
    fn eval(&mut self, source: &str) -> Result<String> {
        self.call(source)?;
        let p = self
            .api
            .tolstring
            .call(&mut self.store, (self.state, -1, self.len_ptr))?;
        if p == 0 {
            return Ok(String::new());
        }
        let mut n = [0; 4];
        self.memory
            .read(&self.store, self.len_ptr as usize, &mut n)?;
        let n = u32::from_le_bytes(n) as usize;
        if n > MAX_MESSAGE_BYTES {
            bail!("plugin result too large");
        }
        let mut bytes = vec![0; n];
        self.memory.read(&self.store, p as usize, &mut bytes)?;
        Ok(String::from_utf8(bytes)?)
    }
    fn error_detail(&mut self) -> Result<String> {
        let p = self
            .api
            .tolstring
            .call(&mut self.store, (self.state, -1, self.len_ptr))?;
        let mut bytes = [0; 4];
        self.memory
            .read(&self.store, self.len_ptr as usize, &mut bytes)?;
        let count = (u32::from_le_bytes(bytes) as usize).min(4096);
        let mut detail = vec![0; count];
        if p != 0 {
            self.memory.read(&self.store, p as usize, &mut detail)?;
        }
        Ok(String::from_utf8_lossy(&detail).into_owned())
    }
    fn push_str(&mut self, s: &str) -> Result<()> {
        let bytes = s.as_bytes();
        if bytes.is_empty() {
            self.api
                .pushlstring
                .call(&mut self.store, (self.state, self.scratch_ptr, 0))?;
            return Ok(());
        }
        if bytes.len() > self.scratch_cap {
            if bytes.len() > MAX_MESSAGE_BYTES {
                bail!("event too large");
            }
            let new_cap = bytes.len().next_power_of_two().min(MAX_MESSAGE_BYTES);
            let new_ptr = self.api.malloc.call(&mut self.store, new_cap as i32)?;
            if new_ptr == 0 {
                bail!("interpreter memory exhausted");
            }
            self.api.free.call(&mut self.store, self.scratch_ptr)?;
            self.scratch_ptr = new_ptr;
            self.scratch_cap = new_cap;
        }
        self.memory
            .write(&mut self.store, self.scratch_ptr as usize, bytes)?;
        self.api.pushlstring.call(
            &mut self.store,
            (self.state, self.scratch_ptr, bytes.len() as i32),
        )?;
        Ok(())
    }
    fn push_value(
        &mut self,
        value: &serde_json::Value,
        depth: usize,
        bytes: &mut usize,
    ) -> Result<()> {
        use serde_json::Value;
        if depth >= MAX_RESULT_DEPTH {
            bail!("event nesting limit");
        }
        match value {
            Value::Null => {
                *bytes += 4;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                self.api.pushnil.call(&mut self.store, self.state)?;
            }
            Value::Bool(b) => {
                *bytes += 5;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                self.api
                    .pushboolean
                    .call(&mut self.store, (self.state, i32::from(*b)))?;
            }
            Value::Number(n) => {
                *bytes += 8;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                if let Some(i) = n.as_i64() {
                    self.api
                        .pushinteger
                        .call(&mut self.store, (self.state, i))?;
                } else if let Some(u) = n.as_u64() {
                    if let Ok(i) = i64::try_from(u) {
                        self.api
                            .pushinteger
                            .call(&mut self.store, (self.state, i))?;
                    } else {
                        self.api
                            .pushnumber
                            .call(&mut self.store, (self.state, u as f64))?;
                    }
                } else if let Some(f) = n.as_f64() {
                    self.api.pushnumber.call(&mut self.store, (self.state, f))?;
                }
            }
            Value::String(s) => {
                *bytes += s.len() + 2;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                self.push_str(s)?;
            }
            Value::Array(a) => {
                *bytes += 2;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                if self.api.checkstack.call(&mut self.store, (self.state, 4))? == 0 {
                    bail!("Lua stack exhausted");
                }
                self.api.createtable.call(
                    &mut self.store,
                    (self.state, a.len().min(i32::MAX as usize) as i32, 0),
                )?;
                for (i, item) in a.iter().enumerate() {
                    *bytes += 1;
                    self.push_value(item, depth + 1, bytes)?;
                    self.api
                        .rawseti
                        .call(&mut self.store, (self.state, -2, (i + 1) as i64))?;
                }
            }
            Value::Object(o) => {
                *bytes += 2;
                if *bytes > MAX_MESSAGE_BYTES {
                    bail!("event too large");
                }
                if self.api.checkstack.call(&mut self.store, (self.state, 4))? == 0 {
                    bail!("Lua stack exhausted");
                }
                self.api.createtable.call(
                    &mut self.store,
                    (self.state, 0, o.len().min(i32::MAX as usize) as i32),
                )?;
                for (k, v) in o {
                    *bytes += k.len() + 3;
                    if *bytes > MAX_MESSAGE_BYTES {
                        bail!("event too large");
                    }
                    self.push_str(k)?;
                    self.push_value(v, depth + 1, bytes)?;
                    self.api.rawset.call(&mut self.store, (self.state, -3))?;
                }
            }
        }
        Ok(())
    }
    /// Dispatch a host event and return the view; data requests are dropped.
    pub fn event(&mut self, event: &serde_json::Value) -> Result<PluginUpdate> {
        self.dispatch(event).map(|d| d.update)
    }
    /// Dispatch a host event. `data` events only come from [`Self::deliver`].
    pub fn dispatch(&mut self, event: &serde_json::Value) -> Result<Dispatch> {
        if event.get("type").and_then(|t| t.as_str()) == Some("data") {
            bail!("data events are delivered by the host only");
        }
        self.pending.clear();
        self.run(event)
    }
    /// Deliver answers to outstanding requests as a `data` event.
    pub fn deliver(&mut self, results: &[DataReply]) -> Result<Dispatch> {
        let mut total = 0;
        for reply in results {
            if !self.pending.remove(&reply.token) {
                bail!("data answer for an unknown request");
            }
            let len = serde_json::to_vec(reply)?.len();
            total += len;
            if len > MAX_DATA_RESULT_BYTES + 256 || total > MAX_DATA_EVENT_BYTES {
                bail!("data answers exceed budget");
            }
        }
        self.run(&serde_json::json!({"type": "data", "results": results}))
    }
    fn run(&mut self, event: &serde_json::Value) -> Result<Dispatch> {
        let _epoch = EpochGuard::enter(self.epoch_gate.clone());
        self.store.set_fuel(FUEL)?;
        self.store.set_epoch_deadline(25);
        // Stack slot 1 holds the precompiled `__nana_dispatch` closure.
        self.api.settop.call(&mut self.store, (self.state, 1))?;
        self.api.pushvalue.call(&mut self.store, (self.state, 1))?;
        let mut bytes = 0usize;
        self.push_value(event, 0, &mut bytes)?;
        let status = self
            .api
            .pcallk
            .call(&mut self.store, (self.state, 1, 1, 0, 0, 0))?;
        if status != 0 {
            let detail = self.error_detail()?;
            self.api.settop.call(&mut self.store, (self.state, 1))?;
            bail!("Lua execution failed: {detail}");
        }
        self.store.set_fuel(READ_FUEL)?;
        // Host work bounded by value count, not plugin time: a wider window.
        self.store.set_epoch_deadline(250);
        let mut walk = Walk {
            values: 0,
            bytes: 0,
            length: self.len_ptr,
        };
        let value = self.read(-1, 0, None, &mut walk);
        self.api.settop.call(&mut self.store, (self.state, 1))?;
        let dispatch: Dispatch = serde_json::from_value(value?)?;
        dispatch.update.validate().map_err(|e| format_err!(e))?;
        crate::validate_requests(&dispatch.requests).map_err(|e| format_err!(e))?;
        crate::validate_logs(&dispatch.logs).map_err(|e| format_err!(e))?;
        self.pending
            .extend(dispatch.requests.iter().map(|r| r.token));
        if self.pending.len() > MAX_DATA_REQUESTS {
            bail!("too many pending data requests");
        }
        Ok(dispatch)
    }
}

struct Walk {
    values: usize,
    bytes: usize,
    /// Scratch `size_t` for string lengths.
    length: i32,
}

impl LuaSandbox {
    /// Convert a Lua value to JSON through raw C API reads: no Lua code,
    /// metamethods or plugin functions run while the host reads a result.
    fn read(
        &mut self,
        index: i32,
        depth: usize,
        key: Option<&str>,
        walk: &mut Walk,
    ) -> Result<serde_json::Value> {
        use serde_json::Value;
        walk.values += 1;
        if depth >= MAX_RESULT_DEPTH || walk.values > MAX_RESULT_VALUES {
            bail!("response nesting or size limit");
        }
        let (store, api, state) = (&mut self.store, &self.api, self.state);
        let index = api.absindex.call(&mut *store, (state, index))?;
        // Approximate encoded size: separators plus the value itself.
        walk.bytes += 1;
        Ok(match api.kind.call(&mut *store, (state, index))? {
            0 => {
                walk.bytes += 4;
                Value::Null
            }
            1 => {
                walk.bytes += 5;
                Value::Bool(api.toboolean.call(&mut *store, (state, index))? != 0)
            }
            3 => {
                if api.isinteger.call(&mut *store, (state, index))? != 0 {
                    let n = api.tointegerx.call(&mut *store, (state, index, 0))?;
                    walk.bytes += n.checked_ilog10().map_or(1, |d| d as usize + 2);
                    Value::from(n)
                } else {
                    walk.bytes += 24;
                    let n = api.tonumberx.call(&mut *store, (state, index, 0))?;
                    Value::Number(
                        serde_json::Number::from_f64(n)
                            .ok_or_else(|| format_err!("non-finite number in result"))?,
                    )
                }
            }
            4 => Value::String(self.string(index, walk)?),
            5 => {
                let len = self.api.rawlen.call(&mut self.store, (state, index))?;
                if len > 0 || key.is_some_and(|k| ARRAY_KEYS.contains(&k)) {
                    let mut items = Vec::new();
                    for i in 1..=len {
                        self.api.rawgeti.call(&mut self.store, (state, index, i))?;
                        let item = self.read(-1, depth + 1, None, walk);
                        self.api.settop.call(&mut self.store, (state, -2))?;
                        items.push(item?);
                    }
                    Value::Array(items)
                } else {
                    let mut object = serde_json::Map::new();
                    self.api.pushnil.call(&mut self.store, state)?;
                    while self.api.next.call(&mut self.store, (state, index))? != 0 {
                        // A non-string key is rejected before it could be converted in place.
                        if self.api.kind.call(&mut self.store, (state, -2))? != 4 {
                            bail!("object keys must be strings");
                        }
                        let name = self.string(-2, walk)?;
                        walk.bytes += 1;
                        let value = self.read(-1, depth + 1, Some(&name), walk);
                        self.api.settop.call(&mut self.store, (state, -2))?;
                        object.insert(name, value?);
                    }
                    Value::Object(object)
                }
            }
            _ => bail!("only data values may cross the host boundary"),
        })
    }
    fn string(&mut self, index: i32, walk: &mut Walk) -> Result<String> {
        let p = self
            .api
            .tolstring
            .call(&mut self.store, (self.state, index, walk.length))?;
        let mut n = [0; 4];
        self.memory
            .read(&self.store, walk.length as usize, &mut n)?;
        let n = u32::from_le_bytes(n) as usize;
        walk.bytes += n + 2;
        if walk.bytes > MAX_MESSAGE_BYTES {
            bail!("plugin result too large");
        }
        let mut bytes = vec![0; n];
        self.memory.read(&self.store, p as usize, &mut bytes)?;
        String::from_utf8(bytes).map_err(|_| format_err!("strings must be UTF-8"))
    }
}
