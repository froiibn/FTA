//! Android owns paths, permissions, discovery and the encrypted key vault; the existing mobile host owns transfers.
use std::ffi::c_char;

/// # Safety
/// `request` must be a live NUL-terminated UTF-8 JSON control message; release the result with `fta_android_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fta_android_call(request: *const c_char) -> *mut c_char {
    // SAFETY: The caller supplies a valid CString; the existing boundary catches panics and owns its response allocation on every platform.
    unsafe { fta_ios::fta_ios_call(request) }
}

/// # Safety
/// `response` must be an outstanding result from this library, released exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fta_android_free(response: *mut c_char) {
    // SAFETY: The caller returns one live allocation, so matching ownership avoids mixing the JVM/C heaps with Rust's response allocation.
    unsafe { fta_ios::fta_ios_free(response) }
}

/// # Safety
/// Inputs must remain valid NUL-terminated UTF-8 native application paths until this function returns.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fta_android_configure(
    container: *const c_char,
    state: *const c_char,
    received: *const c_char,
) -> i32 {
    use std::{ffi::CStr, path::PathBuf};
    let result = std::panic::catch_unwind(|| {
        let parse = |p: *const c_char| -> std::io::Result<PathBuf> {
            if p.is_null() {
                return Err(std::io::Error::other("native path required"));
            }
            // SAFETY: JNI keeps each input CString alive throughout this call; the pointer was checked above.
            Ok(PathBuf::from(
                unsafe { CStr::from_ptr(p) }
                    .to_str()
                    .map_err(std::io::Error::other)?,
            ))
        };
        let root = parse(container)?.canonicalize()?;
        let state = parse(state)?.canonicalize()?;
        let receive = parse(received)?.canonicalize()?;
        if !state.starts_with(&root) || !receive.starts_with(&root) {
            return Err(std::io::Error::other("paths outside application sandbox"));
        }
        fta_platform::android::configure(
            root,
            fta_platform::PlatformPaths {
                config_dir: state.clone(),
                runtime_dir: state.join("run"),
                state_dir: state,
                receive_dir: receive,
            },
        )
    });
    if matches!(result, Ok(Ok(()))) { 0 } else { -1 }
}
