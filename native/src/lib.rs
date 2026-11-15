#![deny(unsafe_op_in_unsafe_fn)]
pub mod core;
pub mod credential;
mod events;
mod github;
mod runtime;
pub use runtime::PluginRuntime;
use serde_json::{json, Value};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::OnceLock,
};
const MAX_INPUT: usize = 2 * 1024 * 1024;
fn runtime() -> &'static PluginRuntime {
    static RUNTIME: OnceLock<PluginRuntime> = OnceLock::new();
    RUNTIME.get_or_init(PluginRuntime::new)
}
#[no_mangle]
pub extern "C" fn otools_plugin_invoke(input: *const u8, len: usize, out: *mut usize) -> *mut u8 {
    if input.is_null() || out.is_null() || len > MAX_INPUT {
        return std::ptr::null_mut();
    }
    let response = catch_unwind(AssertUnwindSafe(|| {
        let bytes = unsafe { std::slice::from_raw_parts(input, len) };
        let request: Value =
            serde_json::from_slice(bytes).map_err(|_| "Invalid request JSON".to_string())?;
        runtime().invoke(request)
    }));
    let response = match response {
        Ok(Ok(data)) => json!({"ok":true,"data":data}),
        Ok(Err(error)) => json!({"ok":false,"error":error}),
        Err(_) => json!({"ok":false,"error":"Plugin operation failed","panic":true}),
    };
    let bytes = serde_json::to_vec(&response)
        .unwrap_or_else(|_| b"{\"ok\":false}".to_vec())
        .into_boxed_slice();
    unsafe {
        *out = bytes.len();
    }
    Box::into_raw(bytes) as *mut u8
}
#[no_mangle]
pub extern "C" fn otools_plugin_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
        }
    }
}
