#![allow(unsafe_code)]

//! C ABI bindings that expose the runtime to non-Rust hosts.
//!
//! This module intentionally keeps the surface minimal and focuses on the pieces
//! that the current integration requires: runtime handles, callback completion,
//! and observable registration. Additional APIs (computed/reaction) will build
//! on the same patterns.

use crate::core::action;
use crate::core::action::ActionPolicy;
use crate::core::observable::{Atom, ObservableCore};
use crate::core::reaction::{self, ReactionHandle};
use crate::core::runtime::{self, AllowStateChangesGuard};
use crate::internal::shared::Shared;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::slice;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const INVALID_ID: u64 = 0;
const RUNTIME_ID: u64 = 1;

/// Runtime handle exported through the FFI surface.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mobx_runtime_t {
    pub raw: u64,
}

/// Observable handle exported through the FFI surface.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mobx_observable_t {
    pub raw: u64,
}

/// Reaction handle exported through the FFI surface.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mobx_reaction_t {
    pub raw: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mobx_state_guard_t {
    pub raw: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub enum MobxActionPolicy {
    MobxActionNever = 0,
    MobxActionObserved = 1,
    MobxActionAlways = 2,
}

/// Computed handle exported through the FFI surface.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct mobx_computed_t {
    pub raw: u64,
}

impl mobx_runtime_t {
    fn is_valid(&self) -> bool {
        self.raw == RUNTIME_ID
    }
}

impl mobx_observable_t {
    fn is_valid(&self) -> bool {
        self.raw != INVALID_ID
    }
}

impl mobx_reaction_t {
    fn is_valid(&self) -> bool {
        self.raw != INVALID_ID
    }
}

impl mobx_state_guard_t {
    fn is_valid(&self) -> bool {
        self.raw != INVALID_ID
    }
}

impl mobx_computed_t {
    fn is_valid(&self) -> bool {
        self.raw != INVALID_ID
    }
}

type ReadCallback = unsafe extern "C" fn(user_data: *mut c_void) -> MobxValue;
type WriteCallback = unsafe extern "C" fn(user_data: *mut c_void, value: MobxValue);
type ReactionCallback = unsafe extern "C" fn(user_data: *mut c_void);

#[repr(C)]
pub struct mobx_observable_desc {
    pub name: *const c_char,
    pub user_data: *mut c_void,
    pub read: Option<ReadCallback>,
    pub write: Option<WriteCallback>,
}

#[repr(C)]
pub struct mobx_reaction_desc {
    pub name: *const c_char,
    pub user_data: *mut c_void,
    pub run: Option<ReactionCallback>,
}

#[repr(C)]
pub struct mobx_action_desc {
    pub name: *const c_char,
    pub user_data: *mut c_void,
    pub body: Option<ReactionCallback>,
}

type ComputedGetter = ReadCallback;

#[repr(C)]
pub struct mobx_computed_desc {
    pub name: *const c_char,
    pub user_data: *mut c_void,
    pub getter: Option<ComputedGetter>,
    pub setter: Option<WriteCallback>,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct MobxSlice {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MobxValueTag {
    Bool = 0,
    I64 = 1,
    F64 = 2,
    String = 3,
    Json = 4,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union MobxValueData {
    pub bool_value: bool,
    pub int_value: i64,
    pub float_value: f64,
    pub string_value: MobxSlice,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct MobxValue {
    pub tag: MobxValueTag,
    pub data: MobxValueData,
}

impl MobxValue {
    pub fn bool(value: bool) -> Self {
        Self {
            tag: MobxValueTag::Bool,
            data: MobxValueData { bool_value: value },
        }
    }

    pub fn i64(value: i64) -> Self {
        Self {
            tag: MobxValueTag::I64,
            data: MobxValueData { int_value: value },
        }
    }
}

#[derive(Clone, PartialEq)]
enum OwnedValueData {
    Bool(bool),
    I64(i64),
    F64(f64),
    Bytes(Vec<u8>),
}

#[derive(Clone, PartialEq)]
struct OwnedValue {
    tag: MobxValueTag,
    data: OwnedValueData,
}

impl OwnedValue {
    fn from_ffi(value: MobxValue) -> Option<Self> {
        let owned_data = match value.tag {
            MobxValueTag::Bool => OwnedValueData::Bool(unsafe { value.data.bool_value }),
            MobxValueTag::I64 => OwnedValueData::I64(unsafe { value.data.int_value }),
            MobxValueTag::F64 => OwnedValueData::F64(unsafe { value.data.float_value }),
            MobxValueTag::String | MobxValueTag::Json => {
                let slice = unsafe { value.data.string_value };
                if slice.ptr.is_null() && slice.len > 0 {
                    return None;
                }
                let bytes = unsafe { slice::from_raw_parts(slice.ptr, slice.len) };
                OwnedValueData::Bytes(bytes.to_vec())
            }
        };
        Some(Self {
            tag: value.tag,
            data: owned_data,
        })
    }

    fn to_ffi(&self) -> MobxValue {
        match self.tag {
            MobxValueTag::Bool => {
                if let OwnedValueData::Bool(b) = self.data {
                    MobxValue::bool(b)
                } else {
                    MobxValue::bool(false)
                }
            }
            MobxValueTag::I64 => {
                if let OwnedValueData::I64(i) = self.data {
                    MobxValue::i64(i)
                } else {
                    MobxValue::i64(0)
                }
            }
            MobxValueTag::F64 => {
                let value = if let OwnedValueData::F64(f) = self.data {
                    f
                } else {
                    0.0
                };
                MobxValue {
                    tag: MobxValueTag::F64,
                    data: MobxValueData { float_value: value },
                }
            }
            MobxValueTag::String | MobxValueTag::Json => {
                let bytes = match &self.data {
                    OwnedValueData::Bytes(vec) => vec.clone(),
                    _ => Vec::new(),
                };
                let slice = RETURN_BUFFER.with(|buffer| {
                    let mut guard = buffer.borrow_mut();
                    *guard = bytes;
                    MobxSlice {
                        ptr: guard.as_ptr(),
                        len: guard.len(),
                    }
                });
                MobxValue {
                    tag: self.tag,
                    data: MobxValueData {
                        string_value: slice,
                    },
                }
            }
        }
    }
}

thread_local! {
    static RETURN_BUFFER: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

struct RuntimeState {
    refcount: AtomicU64,
}

impl RuntimeState {
    fn instance() -> &'static RuntimeState {
        static STATE: OnceLock<RuntimeState> = OnceLock::new();
        STATE.get_or_init(|| RuntimeState {
            refcount: AtomicU64::new(0),
        })
    }
}

struct ObservableEntry {
    atom: Shared<Atom>,
    user_data: UserData,
    read: ReadCallback,
    write: Option<WriteCallback>,
}

unsafe impl Send for ObservableEntry {}
unsafe impl Sync for ObservableEntry {}

struct ReactionEntry {
    handle: ReactionHandle,
    _context: Arc<CallbackContext>,
}

unsafe impl Send for ReactionEntry {}
unsafe impl Sync for ReactionEntry {}

struct ComputedEntry {
    computed: crate::core::computed::Computed<OwnedValue>,
    setter: Option<WriteCallback>,
    user_data: UserData,
    _context: Arc<GetterContext>,
}

#[derive(Copy, Clone)]
struct UserData(*mut c_void);

unsafe impl Send for UserData {}
unsafe impl Sync for UserData {}

struct CallbackContext {
    user_data: UserData,
    callback: ReactionCallback,
}

unsafe impl Send for CallbackContext {}
unsafe impl Sync for CallbackContext {}

struct GetterContext {
    user_data: UserData,
    getter: ComputedGetter,
}

unsafe impl Send for GetterContext {}
unsafe impl Sync for GetterContext {}

unsafe impl Send for ComputedEntry {}
unsafe impl Sync for ComputedEntry {}

impl ObservableEntry {
    fn new(desc: &mobx_observable_desc, name: String) -> Option<Self> {
        let read = desc.read?;
        Some(Self {
            atom: Atom::new(name, None, None),
            user_data: UserData(desc.user_data),
            read,
            write: desc.write,
        })
    }

    fn read(&self) -> MobxValue {
        self.atom.report_observed();
        unsafe { (self.read)(self.user_data.0) }
    }

    fn write(&self, value: MobxValue) {
        if let Some(cb) = self.write {
            unsafe { cb(self.user_data.0, value) };
        }
        self.atom.report_changed();
    }

    fn notify_changed(&self) {
        self.atom.report_changed();
    }
}

impl ReactionEntry {
    fn new(desc: &mobx_reaction_desc, name: String) -> Option<Self> {
        let callback = desc.run?;
        let context = Arc::new(CallbackContext {
            user_data: UserData(desc.user_data),
            callback,
        });
        let ctx_clone = Arc::clone(&context);
        let handle = reaction::autorun(move || unsafe {
            (ctx_clone.callback)(ctx_clone.user_data.0);
        });
        let _ = name; // placeholder until named reactions are supported
        Some(Self {
            handle,
            _context: context,
        })
    }
}

impl ComputedEntry {
    fn new(desc: &mobx_computed_desc, name: String) -> Option<Self> {
        let getter = desc.getter?;
        let user_data = UserData(desc.user_data);
        let ctx = Arc::new(GetterContext { user_data, getter });
        let ctx_clone = Arc::clone(&ctx);
        let computed = crate::core::computed::Computed::new(
            crate::core::computed::ComputedOptions::new(move || unsafe {
                OwnedValue::from_ffi((ctx_clone.getter)(ctx_clone.user_data.0))
                    .expect("invalid value from getter")
            })
            .name(name),
        );
        Some(Self {
            computed,
            setter: desc.setter,
            user_data,
            _context: ctx,
        })
    }
}

struct ObservableRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, Arc<ObservableEntry>>>,
}

impl ObservableRegistry {
    fn instance() -> &'static ObservableRegistry {
        static REGISTRY: OnceLock<ObservableRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| ObservableRegistry {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        })
    }

    fn insert(&self, entry: ObservableEntry) -> mobx_observable_t {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.entries.lock().expect("registry poisoned");
        guard.insert(id, Arc::new(entry));
        mobx_observable_t { raw: id }
    }

    fn get(&self, handle: mobx_observable_t) -> Option<Arc<ObservableEntry>> {
        if !handle.is_valid() {
            return None;
        }
        let guard = self.entries.lock().expect("registry poisoned");
        guard.get(&handle.raw).cloned()
    }

    fn remove(&self, handle: mobx_observable_t) {
        let mut guard = self.entries.lock().expect("registry poisoned");
        guard.remove(&handle.raw);
    }
}

fn runtime_handle_or_invalid(runtime: mobx_runtime_t) -> mobx_runtime_t {
    if runtime.is_valid() {
        runtime
    } else {
        mobx_runtime_t { raw: INVALID_ID }
    }
}

fn observable_registry() -> &'static ObservableRegistry {
    ObservableRegistry::instance()
}

struct ReactionRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, Arc<ReactionEntry>>>,
}

impl ReactionRegistry {
    fn instance() -> &'static ReactionRegistry {
        static REGISTRY: OnceLock<ReactionRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| ReactionRegistry {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        })
    }

    fn insert(&self, entry: ReactionEntry) -> mobx_reaction_t {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.entries.lock().expect("reaction registry poisoned");
        guard.insert(id, Arc::new(entry));
        mobx_reaction_t { raw: id }
    }

    fn get(&self, handle: mobx_reaction_t) -> Option<Arc<ReactionEntry>> {
        if !handle.is_valid() {
            return None;
        }
        let guard = self.entries.lock().expect("reaction registry poisoned");
        guard.get(&handle.raw).cloned()
    }

    fn remove(&self, handle: mobx_reaction_t) -> Option<Arc<ReactionEntry>> {
        let mut guard = self.entries.lock().expect("reaction registry poisoned");
        guard.remove(&handle.raw)
    }
}

fn reaction_registry() -> &'static ReactionRegistry {
    ReactionRegistry::instance()
}

struct StateGuardRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, AllowStateChangesGuard>>,
}

impl StateGuardRegistry {
    fn instance() -> &'static StateGuardRegistry {
        static REGISTRY: OnceLock<StateGuardRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| StateGuardRegistry {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        })
    }

    fn insert(&self, guard: AllowStateChangesGuard) -> mobx_state_guard_t {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut guard_map = self.entries.lock().expect("state guard registry poisoned");
        guard_map.insert(id, guard);
        mobx_state_guard_t { raw: id }
    }

    fn remove(&self, handle: mobx_state_guard_t) -> Option<AllowStateChangesGuard> {
        let mut guard_map = self.entries.lock().expect("state guard registry poisoned");
        guard_map.remove(&handle.raw)
    }
}

fn state_guard_registry() -> &'static StateGuardRegistry {
    StateGuardRegistry::instance()
}

struct ComputedRegistry {
    next_id: AtomicU64,
    entries: Mutex<HashMap<u64, Arc<ComputedEntry>>>,
}

impl ComputedRegistry {
    fn instance() -> &'static ComputedRegistry {
        static REGISTRY: OnceLock<ComputedRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| ComputedRegistry {
            next_id: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        })
    }

    fn insert(&self, entry: ComputedEntry) -> mobx_computed_t {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut guard = self.entries.lock().expect("computed registry poisoned");
        guard.insert(id, Arc::new(entry));
        mobx_computed_t { raw: id }
    }

    fn get(&self, handle: mobx_computed_t) -> Option<Arc<ComputedEntry>> {
        if !handle.is_valid() {
            return None;
        }
        let guard = self.entries.lock().expect("computed registry poisoned");
        guard.get(&handle.raw).cloned()
    }

    fn remove(&self, handle: mobx_computed_t) {
        let mut guard = self.entries.lock().expect("computed registry poisoned");
        guard.remove(&handle.raw);
    }
}

fn computed_registry() -> &'static ComputedRegistry {
    ComputedRegistry::instance()
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_runtime_create() -> mobx_runtime_t {
    let state = RuntimeState::instance();
    state.refcount.fetch_add(1, Ordering::Relaxed);
    mobx_runtime_t { raw: RUNTIME_ID }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_runtime_retain(runtime: mobx_runtime_t) {
    if runtime.is_valid() {
        let state = RuntimeState::instance();
        state.refcount.fetch_add(1, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_runtime_release(runtime: mobx_runtime_t) {
    if runtime.is_valid() {
        let state = RuntimeState::instance();
        state.refcount.fetch_sub(1, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_runtime_callback_complete(runtime: mobx_runtime_t) {
    if runtime_handle_or_invalid(runtime).is_valid() {
        reaction::run_pending_reactions();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_observable_register(
    runtime: mobx_runtime_t,
    desc: *const mobx_observable_desc,
) -> mobx_observable_t {
    if !runtime.is_valid() || desc.is_null() {
        return mobx_observable_t { raw: INVALID_ID };
    }

    let desc = unsafe { &*desc };
    let name = unsafe { c_string(desc.name) }.unwrap_or_else(|| "ffi::observable".to_string());

    let Some(entry) = ObservableEntry::new(desc, name) else {
        return mobx_observable_t { raw: INVALID_ID };
    };

    observable_registry().insert(entry)
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_observable_get(handle: mobx_observable_t) -> MobxValue {
    if let Some(entry) = observable_registry().get(handle) {
        entry.read()
    } else {
        MobxValue::bool(false)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_observable_set(handle: mobx_observable_t, value: MobxValue) {
    if let Some(entry) = observable_registry().get(handle) {
        entry.write(value);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_observable_report_changed(handle: mobx_observable_t) {
    if let Some(entry) = observable_registry().get(handle) {
        entry.notify_changed();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_observable_dispose(handle: mobx_observable_t) {
    observable_registry().remove(handle);
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_set_enforce_actions(policy: MobxActionPolicy) {
    let rust_policy = match policy {
        MobxActionPolicy::MobxActionNever => ActionPolicy::Never,
        MobxActionPolicy::MobxActionObserved => ActionPolicy::Observed,
        MobxActionPolicy::MobxActionAlways => ActionPolicy::Always,
    };
    action::set_enforce_actions(rust_policy);
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_allow_state_changes(allow: bool) -> mobx_state_guard_t {
    let guard = runtime::allow_state_changes_guard(allow);
    state_guard_registry().insert(guard)
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_allow_state_changes_end(handle: mobx_state_guard_t) {
    if handle.is_valid() {
        state_guard_registry().remove(handle);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_computed_register(
    runtime: mobx_runtime_t,
    desc: *const mobx_computed_desc,
) -> mobx_computed_t {
    if !runtime.is_valid() || desc.is_null() {
        return mobx_computed_t { raw: INVALID_ID };
    }
    let desc = unsafe { &*desc };
    let name = unsafe { c_string(desc.name) }.unwrap_or_else(|| "ffi::computed".to_string());
    let Some(entry) = ComputedEntry::new(desc, name) else {
        return mobx_computed_t { raw: INVALID_ID };
    };
    computed_registry().insert(entry)
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_computed_get(handle: mobx_computed_t) -> MobxValue {
    if let Some(entry) = computed_registry().get(handle) {
        let value = entry.computed.get();
        value.to_ffi()
    } else {
        MobxValue::bool(false)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_computed_set(handle: mobx_computed_t, value: MobxValue) {
    if let Some(entry) = computed_registry().get(handle) {
        if let Some(setter) = entry.setter {
            unsafe { setter(entry.user_data.0, value) };
        } else if let OwnedValueData::Bool(..) | OwnedValueData::I64(..) | OwnedValueData::F64(..) =
            entry.computed.get().data
        {
            // no-op when setter missing
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_computed_dispose(handle: mobx_computed_t) {
    computed_registry().remove(handle);
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_autorun(
    runtime: mobx_runtime_t,
    desc: *const mobx_reaction_desc,
) -> mobx_reaction_t {
    if !runtime.is_valid() || desc.is_null() {
        return mobx_reaction_t { raw: INVALID_ID };
    }
    let desc = unsafe { &*desc };
    let name = unsafe { c_string(desc.name) }.unwrap_or_else(|| "ffi::reaction".to_string());
    let Some(entry) = ReactionEntry::new(desc, name) else {
        return mobx_reaction_t { raw: INVALID_ID };
    };
    reaction_registry().insert(entry)
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_reaction_schedule(handle: mobx_reaction_t) {
    if let Some(entry) = reaction_registry().get(handle) {
        entry.handle.schedule();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_reaction_dispose(handle: mobx_reaction_t) {
    if let Some(entry) = reaction_registry().remove(handle) {
        entry.handle.dispose();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn mobx_run_in_action(runtime: mobx_runtime_t, desc: *const mobx_action_desc) {
    if !runtime.is_valid() || desc.is_null() {
        return;
    }
    let desc = unsafe { &*desc };
    let Some(body) = desc.body else {
        return;
    };
    let name = unsafe { c_string(desc.name) }.unwrap_or_else(|| "ffi::action".to_string());
    let user_data = UserData(desc.user_data);
    action::action(name, || unsafe {
        body(user_data.0);
    });
}

unsafe fn c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(|s| s.to_string())
}

#[cfg(all(test, feature = "ffi"))]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

    struct HostField {
        runtime: mobx_runtime_t,
        value: AtomicI64,
    }

    unsafe extern "C" fn read_cb(user_data: *mut c_void) -> MobxValue {
        let field = unsafe { &*(user_data as *const HostField) };
        mobx_runtime_callback_complete(field.runtime);
        MobxValue::i64(field.value.load(Ordering::Relaxed))
    }

    unsafe extern "C" fn write_cb(user_data: *mut c_void, value: MobxValue) {
        let field = unsafe { &*(user_data as *const HostField) };
        let new_value = unsafe { value.data.int_value };
        field.value.store(new_value, Ordering::Relaxed);
        mobx_runtime_callback_complete(field.runtime);
    }

    struct ReactionHost {
        runtime: mobx_runtime_t,
        runs: Arc<AtomicUsize>,
        observable: mobx_observable_t,
    }

    unsafe extern "C" fn reaction_cb(user_data: *mut c_void) {
        let host = unsafe { &*(user_data as *const ReactionHost) };
        let _ = mobx_observable_get(host.observable);
        host.runs.fetch_add(1, Ordering::Relaxed);
        mobx_runtime_callback_complete(host.runtime);
    }

    struct ActionHost {
        runtime: mobx_runtime_t,
        counter: Arc<AtomicUsize>,
    }

    unsafe extern "C" fn action_cb(user_data: *mut c_void) {
        let host = unsafe { &*(user_data as *const ActionHost) };
        host.counter.fetch_add(1, Ordering::Relaxed);
        mobx_runtime_callback_complete(host.runtime);
    }

    struct ComputedHost {
        runtime: mobx_runtime_t,
        observable: mobx_observable_t,
    }

    unsafe extern "C" fn computed_getter(user_data: *mut c_void) -> MobxValue {
        let host = unsafe { &*(user_data as *const ComputedHost) };
        let base = mobx_observable_get(host.observable);
        mobx_runtime_callback_complete(host.runtime);
        unsafe { MobxValue::i64(base.data.int_value * 2) }
    }

    #[test]
    fn observable_register_get_set() {
        let runtime = mobx_runtime_create();
        let field = Box::new(HostField {
            runtime,
            value: AtomicI64::new(1),
        });
        let user_data = Box::into_raw(field) as *mut c_void;
        let name = CString::new("counter").unwrap();

        let desc = mobx_observable_desc {
            name: name.as_ptr(),
            user_data,
            read: Some(read_cb),
            write: Some(write_cb),
        };

        let handle = mobx_observable_register(runtime, &desc);
        assert!(handle.is_valid());

        let current = mobx_observable_get(handle);
        unsafe {
            assert_eq!(current.data.int_value, 1);
        }

        mobx_observable_set(handle, MobxValue::i64(42));
        let updated = mobx_observable_get(handle);
        unsafe {
            assert_eq!(updated.data.int_value, 42);
        }

        mobx_observable_dispose(handle);
        unsafe {
            drop(Box::from_raw(user_data as *mut HostField));
        }
        mobx_runtime_release(runtime);
    }

    #[test]
    fn autorun_reacts_to_changes_and_disposes() {
        let runtime = mobx_runtime_create();
        let field = Box::new(HostField {
            runtime,
            value: AtomicI64::new(0),
        });
        let user_data = Box::into_raw(field) as *mut c_void;
        let name = CString::new("counter").unwrap();
        let desc = mobx_observable_desc {
            name: name.as_ptr(),
            user_data,
            read: Some(read_cb),
            write: Some(write_cb),
        };
        let observable = mobx_observable_register(runtime, &desc);
        assert!(observable.is_valid());

        let reaction_runs = Arc::new(AtomicUsize::new(0));
        let reaction_host = Box::new(ReactionHost {
            runtime,
            runs: reaction_runs.clone(),
            observable,
        });
        let reaction_user_data = Box::into_raw(reaction_host) as *mut c_void;
        let reaction_name = CString::new("autorun").unwrap();
        let reaction_desc = mobx_reaction_desc {
            name: reaction_name.as_ptr(),
            user_data: reaction_user_data,
            run: Some(reaction_cb),
        };
        let reaction_handle = mobx_autorun(runtime, &reaction_desc);
        assert!(reaction_handle.is_valid());
        assert_eq!(reaction_runs.load(Ordering::Relaxed), 1);

        mobx_observable_set(observable, MobxValue::i64(5));
        mobx_runtime_callback_complete(runtime);
        assert_eq!(reaction_runs.load(Ordering::Relaxed), 2);

        mobx_reaction_dispose(reaction_handle);
        mobx_observable_set(observable, MobxValue::i64(10));
        mobx_runtime_callback_complete(runtime);
        assert_eq!(reaction_runs.load(Ordering::Relaxed), 2);

        unsafe {
            drop(Box::from_raw(reaction_user_data as *mut ReactionHost));
            drop(Box::from_raw(user_data as *mut HostField));
        }
        mobx_observable_dispose(observable);
        mobx_runtime_release(runtime);
    }

    #[test]
    fn run_in_action_executes_callback() {
        let runtime = mobx_runtime_create();
        let counter = Arc::new(AtomicUsize::new(0));
        let host = Box::new(ActionHost {
            runtime,
            counter: counter.clone(),
        });
        let user_data = Box::into_raw(host) as *mut c_void;
        let name = CString::new("increment").unwrap();
        let desc = mobx_action_desc {
            name: name.as_ptr(),
            user_data,
            body: Some(action_cb),
        };
        mobx_run_in_action(runtime, &desc);
        mobx_runtime_callback_complete(runtime);
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        unsafe {
            drop(Box::from_raw(user_data as *mut ActionHost));
        }
        mobx_runtime_release(runtime);
    }

    #[test]
    fn computed_tracks_observable_changes() {
        let runtime = mobx_runtime_create();
        let field = Box::new(HostField {
            runtime,
            value: AtomicI64::new(2),
        });
        let user_data = Box::into_raw(field) as *mut c_void;
        let name = CString::new("base").unwrap();
        let desc = mobx_observable_desc {
            name: name.as_ptr(),
            user_data,
            read: Some(read_cb),
            write: Some(write_cb),
        };
        let observable = mobx_observable_register(runtime, &desc);

        let computed_host = Box::new(ComputedHost {
            runtime,
            observable,
        });
        let computed_user_data = Box::into_raw(computed_host) as *mut c_void;
        let computed_name = CString::new("double").unwrap();
        let computed_desc = mobx_computed_desc {
            name: computed_name.as_ptr(),
            user_data: computed_user_data,
            getter: Some(computed_getter),
            setter: None,
        };
        let computed_handle = mobx_computed_register(runtime, &computed_desc);
        assert!(computed_handle.is_valid());

        let value = mobx_computed_get(computed_handle);
        unsafe {
            assert_eq!(value.data.int_value, 4);
        }

        mobx_observable_set(observable, MobxValue::i64(5));
        mobx_runtime_callback_complete(runtime);
        let updated = mobx_computed_get(computed_handle);
        unsafe {
            assert_eq!(updated.data.int_value, 10);
        }

        mobx_computed_dispose(computed_handle);
        mobx_observable_dispose(observable);
        unsafe {
            drop(Box::from_raw(user_data as *mut HostField));
            drop(Box::from_raw(computed_user_data as *mut ComputedHost));
        }
        mobx_runtime_release(runtime);
    }
}
