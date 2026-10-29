//! Android Context supplies the sandbox; Keystore wraps the TLS key without persisting plaintext.
use crate::PlatformPaths;
use std::{
    ffi::{CString, c_char},
    fs::File,
    io,
    path::PathBuf,
    sync::OnceLock,
};

struct Sandbox {
    root: PathBuf,
    handle: File,
    paths: PlatformPaths,
}
static SANDBOX: OnceLock<Sandbox> = OnceLock::new();
const MAX_KEY_BYTES: usize = 16 * 1024;

// Android SELinux may deny opening system parents even when this app can open its own container.
pub fn configure(root: PathBuf, paths: PlatformPaths) -> io::Result<()> {
    if !root.is_absolute() || root.canonicalize()? != root {
        return Err(io::Error::other("physical app sandbox required"));
    }
    if let Some(old) = SANDBOX.get() {
        return if old.root == root && old.paths == paths {
            Ok(())
        } else {
            Err(io::Error::other("sandbox already configured"))
        };
    }
    let handle = rustix::fs::open(
        &root,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?;
    SANDBOX
        .set(Sandbox {
            root,
            handle: handle.into(),
            paths,
        })
        .map_err(|_| io::Error::other("sandbox initialization raced"))
}
pub fn paths() -> io::Result<PlatformPaths> {
    SANDBOX
        .get()
        .map(|s| s.paths.clone())
        .ok_or_else(|| io::Error::other("Android Context paths required"))
}
pub fn source_anchor() -> io::Result<(PathBuf, File)> {
    let s = SANDBOX
        .get()
        .ok_or_else(|| io::Error::other("Android sandbox required"))?;
    Ok((s.root.clone(), s.handle.try_clone()?))
}

// The small C shim attaches Rust workers to the JVM and delegates only to this application's key vault.
unsafe extern "C" {
    fn fta_android_store_key(account: *const c_char, key: *const u8, length: usize) -> i32;
    fn fta_android_load_key(account: *const c_char, output: *mut u8, capacity: usize) -> isize;
}
pub fn store_identity_key(account: &str, secret: &[u8]) -> io::Result<()> {
    let account = CString::new(account).map_err(io::Error::other)?;
    if secret.is_empty() || secret.len() > MAX_KEY_BYTES {
        return Err(io::Error::other("unsupported identity key size"));
    }
    // SAFETY: Pointers borrow live Rust buffers for the synchronous JNI call; the JVM encrypts before returning.
    if unsafe { fta_android_store_key(account.as_ptr(), secret.as_ptr(), secret.len()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::other("Android Keystore storage failed"))
    }
}
pub fn load_identity_key(account: &str) -> io::Result<Vec<u8>> {
    let account = CString::new(account).map_err(io::Error::other)?;
    let mut result = vec![0; MAX_KEY_BYTES];
    // SAFETY: The shim checks the Java result length before copying into this explicitly bounded allocation.
    let length =
        unsafe { fta_android_load_key(account.as_ptr(), result.as_mut_ptr(), result.len()) };
    if length <= 0 || length as usize > result.len() {
        return Err(io::Error::other("Android Keystore identity unavailable"));
    }
    result.truncate(length as usize);
    Ok(result)
}
