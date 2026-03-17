//! C ABI (stable) native plugin interface.
//!
//! This module provides an alternative to Rust-ABI `cdylib` plugins.
//!
//! Why: Rust does NOT guarantee a stable ABI across compiler versions.
//! Returning `Box<dyn Plugin>` across a dynamic library boundary is therefore
//! inherently version-locked and can become undefined behavior.
//!
//! The C ABI interface keeps the dynamic boundary limited to:
//! - NUL-terminated UTF-8 strings
//! - integers / pointers
//! - JSON payloads (input/output)
//!
//! This allows plugin authors to build `cdylib` plugins with a stable ABI.
//! Note: C ABI plugins still run in-process and are therefore **not sandboxed**.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use libloading::Library;
use serde_json::Value;

use crate::hook_io::hook_io_from_context;
use crate::{hook_names, Hook, HookContext, HookError, HookOutput, Plugin, PluginSystem};

/// C ABI version implemented by this host.
pub const ROCODE_CABI_VERSION_V1: u32 = 1;

/// Flags for [`RocodePluginDescriptorV1::flags`].
pub const ROCODE_CABI_FLAG_THREADSAFE: u32 = 1 << 0;

/// Symbol that a C ABI plugin must export.
pub const ROCODE_CABI_DESCRIPTOR_SYMBOL_V1: &[u8] = b"rocode_plugin_descriptor_v1";

type CreateFnV1 = unsafe extern "C" fn() -> *mut c_void;
type DestroyFnV1 = unsafe extern "C" fn(*mut c_void);
type HookCountFnV1 = unsafe extern "C" fn(*mut c_void) -> usize;
type HookNameFnV1 = unsafe extern "C" fn(*mut c_void, usize) -> *const c_char;
type InvokeHookFnV1 = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut i32,
) -> *mut c_char;
type FreeStringFnV1 = unsafe extern "C" fn(*mut c_void, *mut c_char);

type DescriptorFnV1 = unsafe extern "C" fn() -> *const RocodePluginDescriptorV1;

/// C ABI plugin descriptor (v1).
///
/// The exported `rocode_plugin_descriptor_v1()` must return a valid pointer to a
/// static instance of this struct.
#[repr(C)]
pub struct RocodePluginDescriptorV1 {
    /// Must be [`ROCODE_CABI_VERSION_V1`].
    pub abi_version: u32,
    /// Capability flags (see `ROCODE_CABI_FLAG_*`).
    pub flags: u32,

    /// Plugin name (NUL-terminated UTF-8). Must be valid for the process lifetime.
    pub name: *const c_char,
    /// Plugin version (NUL-terminated UTF-8). Must be valid for the process lifetime.
    pub version: *const c_char,

    /// Create a plugin instance. Returns an opaque pointer (must not be NULL).
    pub create: Option<CreateFnV1>,
    /// Destroy a plugin instance created by `create`.
    pub destroy: Option<DestroyFnV1>,

    /// Return number of supported hook names.
    pub hook_count: Option<HookCountFnV1>,
    /// Return hook name by index (NUL-terminated UTF-8).
    /// The returned pointer must remain valid until `destroy`.
    pub hook_name: Option<HookNameFnV1>,

    /// Invoke a hook. On success, returns a JSON string (allocated by plugin) OR NULL
    /// to indicate "no change" (caller should use the provided output JSON).
    /// On error, writes a non-zero error code to `out_code` and returns an error message
    /// string (allocated by plugin) OR NULL.
    pub invoke_hook: Option<InvokeHookFnV1>,

    /// Free a string returned by `invoke_hook`.
    pub free_string: Option<FreeStringFnV1>,

    /// Reserved for future expansion (must be zeroed).
    pub reserved: [usize; 8],
}

// Safety: this state is accessed via the function pointers provided by the
// plugin itself. We treat `instance` as an opaque handle.
//
// Concurrency rules:
// - If the plugin does NOT set ROCODE_CABI_FLAG_THREADSAFE, rocode serializes
//   all calls via `call_lock`.
// - If the plugin sets ROCODE_CABI_FLAG_THREADSAFE, it asserts that concurrent
//   calls (including free_string) are safe.
//
// Thread-affine plugins are NOT supported by this ABI contract.
unsafe impl Send for CAbiPluginState {}
unsafe impl Sync for CAbiPluginState {}

#[derive(Debug)]
struct CAbiPluginState {
    instance: *mut c_void,
    destroy: DestroyFnV1,
    invoke_hook: InvokeHookFnV1,
    free_string: FreeStringFnV1,
    /// Serialize calls unless plugin declares itself threadsafe.
    call_lock: Option<Mutex<()>>,
}

impl Drop for CAbiPluginState {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.instance);
        }
    }
}

struct CAbiPlugin {
    name: String,
    version: String,
    hooks: Vec<String>,
    state: Arc<CAbiPluginState>,
}

impl CAbiPlugin {
    fn new(name: String, version: String, hooks: Vec<String>, state: Arc<CAbiPluginState>) -> Self {
        Self {
            name,
            version,
            hooks,
            state,
        }
    }

    fn invoke_blocking(
        state: &CAbiPluginState,
        hook: &str,
        input: Value,
        output: Value,
    ) -> Result<Value, HookError> {
        let _guard = if let Some(lock) = &state.call_lock {
            Some(
                lock.lock()
                    .map_err(|_| HookError::ExecutionError("cabi plugin mutex poisoned".into()))?,
            )
        } else {
            None
        };

        let hook_c = CString::new(hook)
            .map_err(|_| HookError::ExecutionError("hook name contains NUL".into()))?;
        let input_json = serde_json::to_string(&input).map_err(|e| {
            HookError::ExecutionError(format!("failed to serialize cabi hook input: {e}"))
        })?;
        let output_json = serde_json::to_string(&output).map_err(|e| {
            HookError::ExecutionError(format!("failed to serialize cabi hook output: {e}"))
        })?;
        let input_c = CString::new(input_json)
            .map_err(|_| HookError::ExecutionError("hook input JSON contains NUL".into()))?;
        let output_c = CString::new(output_json)
            .map_err(|_| HookError::ExecutionError("hook output JSON contains NUL".into()))?;

        let mut code: i32 = 0;
        let ptr = unsafe {
            (state.invoke_hook)(
                state.instance,
                hook_c.as_ptr(),
                input_c.as_ptr(),
                output_c.as_ptr(),
                &mut code as *mut i32,
            )
        };

        // Helper: convert a plugin-allocated string and free it.
        let take_string = |ptr: *mut c_char| -> Result<String, HookError> {
            if ptr.is_null() {
                return Ok(String::new());
            }

            let cstr = unsafe { CStr::from_ptr(ptr) };
            let bytes = cstr.to_bytes();
            let result = std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .map_err(|e| {
                    HookError::ExecutionError(format!("cabi returned non-utf8 string: {e}"))
                });

            // Always free the plugin-allocated string, even if decoding fails.
            unsafe { (state.free_string)(state.instance, ptr) };
            result
        };

        if code != 0 {
            // ptr is an optional error message.
            let msg = take_string(ptr).unwrap_or_else(|_| String::new());
            let msg = if msg.trim().is_empty() {
                format!("cabi hook failed (code={code})")
            } else {
                format!("cabi hook failed (code={code}): {msg}")
            };
            return Err(HookError::ExecutionError(msg));
        }

        // Success path.
        if ptr.is_null() {
            // "no change": keep seeded output.
            return Ok(output);
        }
        let s = take_string(ptr)?;
        if s.trim().is_empty() {
            // Empty string is treated as "no change".
            return Ok(output);
        }
        serde_json::from_str::<Value>(&s)
            .map_err(|e| HookError::ExecutionError(format!("cabi returned invalid JSON: {e}")))
    }
}

impl Plugin for CAbiPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn register_hooks<'a>(
        &'a self,
        system: &'a PluginSystem,
    ) -> Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            for hook_name in &self.hooks {
                let Some(event) = hook_names::hook_name_to_event(hook_name.as_str()) else {
                    tracing::debug!(
                        plugin = self.name.as_str(),
                        hook = hook_name.as_str(),
                        "skipping unsupported cabi hook"
                    );
                    continue;
                };

                let hook_id = format!("cabi:{}:{}", self.name, hook_name);
                let plugin_name = self.name.clone();
                let hook_name_owned = hook_name.clone();
                let state = Arc::clone(&self.state);

                // Avoid duplicates.
                let _ = system.remove(&event, &hook_id).await;

                system
                    .register(Hook::new(&hook_id, event, move |context: HookContext| {
                        let plugin_name = plugin_name.clone();
                        let hook_name_owned = hook_name_owned.clone();
                        let state = Arc::clone(&state);
                        async move {
                            let (input, output) = hook_io_from_context(&context);
                            let hook_name_for_call = hook_name_owned.clone();
                            let result = tokio::task::spawn_blocking(move || {
                                CAbiPlugin::invoke_blocking(
                                    state.as_ref(),
                                    &hook_name_for_call,
                                    input,
                                    output,
                                )
                            })
                            .await;

                            match result {
                                Ok(Ok(value)) => Ok(HookOutput::with_payload(value)),
                                Ok(Err(e)) => Err(e),
                                Err(join_err) => Err(HookError::ExecutionError(format!(
                                    "cabi plugin `{}` hook `{}` join error: {}",
                                    plugin_name, hook_name_owned, join_err
                                ))),
                            }
                        }
                    }))
                    .await;
            }
        })
    }
}

fn cstr_to_string(ptr: *const c_char, field: &str) -> anyhow::Result<String> {
    if ptr.is_null() {
        return Err(anyhow::anyhow!("cabi descriptor field `{}` is NULL", field));
    }
    Ok(unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| anyhow::anyhow!("cabi descriptor field `{}` is not utf-8: {}", field, e))?
        .to_string())
}

/// Try to load a C ABI plugin from an already-open `libloading::Library`.
///
/// Returns `Ok(None)` if the library does not export the v1 descriptor symbol.
/// Returns `Ok(Some(plugin))` if loaded successfully.
/// Returns `Err(_)` if the symbol exists but the plugin is invalid.
pub(crate) unsafe fn try_load_from_library(
    library: &Library,
    path: &Path,
) -> anyhow::Result<Option<Arc<dyn Plugin>>> {
    let descriptor_fn = match library.get::<DescriptorFnV1>(ROCODE_CABI_DESCRIPTOR_SYMBOL_V1) {
        Ok(sym) => sym,
        Err(_) => return Ok(None),
    };

    let desc_ptr = descriptor_fn();
    if desc_ptr.is_null() {
        return Err(anyhow::anyhow!(
            "cabi plugin {:?} returned NULL descriptor",
            path
        ));
    }
    let desc = &*desc_ptr;

    if desc.abi_version != ROCODE_CABI_VERSION_V1 {
        return Err(anyhow::anyhow!(
            "cabi plugin {:?} abi_version mismatch: expected {}, got {}",
            path,
            ROCODE_CABI_VERSION_V1,
            desc.abi_version
        ));
    }

    let name = cstr_to_string(desc.name, "name")?;
    let version = cstr_to_string(desc.version, "version")?;

    let create = desc
        .create
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing create()", path))?;
    let destroy = desc
        .destroy
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing destroy()", path))?;
    let hook_count = desc
        .hook_count
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing hook_count()", path))?;
    let hook_name = desc
        .hook_name
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing hook_name()", path))?;
    let invoke_hook = desc
        .invoke_hook
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing invoke_hook()", path))?;
    let free_string = desc
        .free_string
        .ok_or_else(|| anyhow::anyhow!("cabi plugin {:?} missing free_string()", path))?;

    let instance = create();
    if instance.is_null() {
        return Err(anyhow::anyhow!(
            "cabi plugin {:?} create() returned NULL",
            path
        ));
    }

    // Collect hook names once at load time.
    let count = hook_count(instance);
    let mut hooks = Vec::with_capacity(count);
    for i in 0..count {
        let ptr = hook_name(instance, i);
        if ptr.is_null() {
            tracing::warn!(
                plugin = name.as_str(),
                index = i,
                "cabi hook_name returned NULL; skipping"
            );
            continue;
        }
        let hook = CStr::from_ptr(ptr)
            .to_str()
            .map_err(|e| anyhow::anyhow!("cabi hook name is not utf-8: {}", e))?
            .to_string();
        hooks.push(hook);
    }
    hooks.sort();
    hooks.dedup();

    let threadsafe = (desc.flags & ROCODE_CABI_FLAG_THREADSAFE) != 0;
    let state = Arc::new(CAbiPluginState {
        instance,
        destroy,
        invoke_hook,
        free_string,
        call_lock: if threadsafe {
            None
        } else {
            Some(Mutex::new(()))
        },
    });

    tracing::info!(
        plugin_name = name.as_str(),
        plugin_version = version.as_str(),
        hooks = hooks.len(),
        path = %path.display(),
        "loaded cabi plugin"
    );

    Ok(Some(
        Arc::new(CAbiPlugin::new(name, version, hooks, state)) as Arc<dyn Plugin>
    ))
}
