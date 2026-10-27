//! User-session integration keeps receiving and credentials scoped to the current account.

use crate::PlatformPaths;

/// Native protocol-activation toasts outlive this process and route back through the OS.
pub mod notifications;
// Setup uses a narrow, explicit elevation boundary instead of elevating the desktop or engine.
pub mod network_setup;
use std::{ffi::OsString, io, mem, os::windows::ffi::OsStringExt, path::Path, ptr};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            },
            Cryptography::{
                CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
            },
            GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
        },
        System::{
            Com::CoTaskMemFree,
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
        UI::Shell::{FOLDERID_Downloads, FOLDERID_LocalAppData, SHGetKnownFolderPath},
    },
    core::GUID,
};
use zeroize::Zeroize;

const APP_DIRECTORY: &str = "FTA";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "FTA";
const SDDL_REVISION: u32 = 1;
const PIPE_PREFIX: &str = r"\\.\pipe\fta";
const MAX_PIPE_INSTANCE_LENGTH: usize = 128;

/// Read Known Folders instead of assuming English directory names or a fixed drive.
pub fn paths() -> io::Result<PlatformPaths> {
    let local = known_folder(&FOLDERID_LocalAppData)?.join(APP_DIRECTORY);
    Ok(PlatformPaths {
        config_dir: local.clone(),
        state_dir: local.clone(),
        runtime_dir: local.join("run"),
        receive_dir: known_folder(&FOLDERID_Downloads)?.join(APP_DIRECTORY),
    })
}

fn known_folder(folder: &GUID) -> io::Result<std::path::PathBuf> {
    let mut allocated = ptr::null_mut();
    // SAFETY: folder is a valid GUID and allocated is a writable out pointer.
    let status = unsafe { SHGetKnownFolderPath(folder, 0, ptr::null_mut(), &mut allocated) };
    if status < 0 {
        return Err(io::Error::other(format!(
            "Known Folder lookup failed: HRESULT {status:#x}"
        )));
    }
    if allocated.is_null() {
        return Err(io::Error::other("Known Folder lookup returned no path"));
    }
    // SAFETY: a successful SHGetKnownFolderPath returns a NUL-terminated UTF-16 allocation.
    let path = unsafe { OsString::from_wide(wide_slice(allocated)) };
    // SAFETY: SHGetKnownFolderPath requires its successful allocation to be freed here.
    unsafe { CoTaskMemFree(allocated.cast()) };
    Ok(path.into())
}

/// # Safety
/// `value` must point to a live NUL-terminated UTF-16 allocation throughout the borrow.
unsafe fn wide_slice<'a>(value: *const u16) -> &'a [u16] {
    let mut length = 0;
    // SAFETY: caller guarantees a live NUL-terminated allocation.
    while unsafe { *value.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the scan above establishes length within the caller's allocation.
    unsafe { std::slice::from_raw_parts(value, length) }
}

/// User-scoped encryption deliberately omits CRYPTPROTECT_LOCAL_MACHINE.
pub fn protect_secret(secret: &[u8]) -> io::Result<Vec<u8>> {
    crypt_secret(secret, false)
}

/// A different Windows account cannot decrypt the current user's stored identity.
pub fn unprotect_secret(protected: &[u8]) -> io::Result<Vec<u8>> {
    crypt_secret(protected, true)
}

fn crypt_secret(data: &[u8], decrypt: bool) -> io::Result<Vec<u8>> {
    let length = u32::try_from(data.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "credential exceeds DPAPI size limit",
        )
    })?;
    let input = CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    // SAFETY: DPAPI treats input as read-only, all lengths match live buffers, and optional
    // parameters are null. Its output is copied before zeroizing and freeing below.
    let status = unsafe {
        if decrypt {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if status == 0 {
        return Err(io::Error::last_os_error());
    }
    // A zero-length successful result may have a null pointer; from_raw_parts cannot use it.
    let result = if output.cbData == 0 {
        Vec::new()
    } else if output.pbData.is_null() {
        return Err(io::Error::other("DPAPI returned an invalid allocation"));
    } else {
        // SAFETY: successful DPAPI gives cbData live bytes in its LocalAlloc allocation.
        let buffer =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let result = buffer.to_vec();
        buffer.zeroize();
        result
    };
    if !output.pbData.is_null() {
        // SAFETY: DPAPI documents LocalFree as the matching output deallocator.
        unsafe { LocalFree(output.pbData.cast()) };
    }
    Ok(result)
}

/// Stable user SID names isolate accounts without relying on a mutable display name.
pub fn user_pipe_name(instance: &str) -> io::Result<String> {
    if instance.is_empty()
        || instance.len() > MAX_PIPE_INSTANCE_LENGTH
        || !instance
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid IPC instance name",
        ));
    }
    Ok(format!("{PIPE_PREFIX}-{}-{instance}", current_user_sid()?))
}

fn current_user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the pseudo process handle is valid and token is a writable handle output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // Ensure every early return closes the process-token handle.
    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: Token exclusively owns a successfully opened handle.
            unsafe { CloseHandle(self.0) };
        }
    }
    let token = Token(token);
    let mut length = 0;
    // SAFETY: a null query buffer with zero size requests the required allocation size.
    unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut length) };
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    // usize storage gives TOKEN_USER the alignment that a Vec<u8> would not guarantee.
    let words = (length as usize).div_ceil(mem::size_of::<usize>());
    let mut storage = vec![0usize; words];
    // SAFETY: storage is correctly aligned and contains at least length writable bytes.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful GetTokenInformation filled a TokenUser record in storage.
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut string_sid = ptr::null_mut();
    // SAFETY: user.User.Sid points inside storage, which stays alive throughout this call.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut string_sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful SID conversion returns a NUL-terminated LocalAlloc string.
    let sid = unsafe { String::from_utf16_lossy(wide_slice(string_sid)) };
    // SAFETY: this allocation belongs to this conversion and must be freed with LocalFree.
    unsafe { LocalFree(string_sid.cast()) };
    Ok(sid)
}

/// Apply a current-user-only DACL at creation; fixing permissions after creation is racy.
/// This function requires a Tokio runtime with I/O enabled.
pub fn create_user_pipe(name: &str, first_instance: bool) -> io::Result<NamedPipeServer> {
    let sid = current_user_sid()?;
    let expected_prefix = format!("{PIPE_PREFIX}-{sid}-");
    // Reuse the same namespace validation for callers that bypass user_pipe_name.
    let instance = name.strip_prefix(&expected_prefix).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "IPC name is not scoped to the current user",
        )
    })?;
    if user_pipe_name(instance)? != name {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid IPC pipe name",
        ));
    }
    let descriptor: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut allocated = ptr::null_mut();
    // SAFETY: descriptor is NUL-terminated, and allocated is a valid descriptor out pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION,
            &mut allocated,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: allocated,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first_instance)
        .reject_remote_clients(true);
    // SAFETY: attributes and its descriptor remain live until synchronous pipe creation
    // returns; the resulting kernel object holds its own security descriptor copy.
    let result = unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    };
    // SAFETY: SDDL conversion returned this LocalAlloc descriptor; pipe creation has finished.
    unsafe { LocalFree(allocated) };
    result
}

/// Protect a directory through an opened handle so path replacement cannot redirect its ACL.
/// The caller creates the directory first; reparse points are explicitly rejected.
pub fn secure_directory(path: &Path) -> io::Result<()> {
    use std::{
        fs::OpenOptions,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
    };
    use windows_sys::Win32::{
        Security::{
            Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
    };
    const READ_CONTROL_ACCESS: u32 = 0x0002_0000;
    const WRITE_DAC_ACCESS: u32 = 0x0004_0000;
    // Excluding FILE_SHARE_DELETE pins this exact directory while its metadata and ACL are checked.
    let directory = OpenOptions::new()
        .access_mode(READ_CONTROL_ACCESS | WRITE_DAC_ACCESS | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private directory must be a real directory, not a reparse point",
        ));
    }
    let sid = current_user_sid()?;
    let sddl: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut descriptor = ptr::null_mut();
    // SAFETY: sddl is terminated and descriptor is a valid writable out pointer.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        // SAFETY: descriptor remains live and all output locations are valid.
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::other("private directory DACL is missing"));
        }
        // SAFETY: the owned file handle is a real directory, and dacl points inside the
        // live descriptor. The API copies the ACL and applies inheritance protection.
        let error = unsafe {
            SetSecurityInfo(
                directory.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        Ok(())
    })();
    // SAFETY: conversion allocated descriptor using LocalAlloc and no call still borrows it.
    unsafe { LocalFree(descriptor) };
    result
}

/// Publish on filesystems without hard links while refusing to replace any existing filename.
pub fn publish_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    const NO_REPLACEMENT_FLAGS: u32 = 0;
    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        let value: Vec<u16> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "file path contains NUL",
            ));
        }
        Ok(value.into_iter().chain(Some(0)).collect())
    }
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    // SAFETY: both paths are NUL-terminated and live throughout the synchronous API call.
    // Omitting MOVEFILE_REPLACE_EXISTING is the atomic no-clobber contract.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), NO_REPLACEMENT_FLAGS) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// HKCU registration requires no elevation and applies only to the interactive account.
pub fn set_autostart(executable: &Path, enabled: bool) -> io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if enabled {
        // Like notification shortcuts, Explorer's login launcher needs a shell-compatible target.
        let executable = dunce::canonicalize(executable)?;
        let value = executable.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "autostart path must be valid Unicode",
            )
        })?;
        if value.contains(['"', '\0']) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid autostart path",
            ));
        }
        // Quoting the entire executable prevents spaces in installation paths becoming args.
        key.set_value(RUN_VALUE, &format!("\"{value}\""))
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // DPAPI corruption must fail rather than silently generate a replacement identity.
    #[test]
    fn secrets_roundtrip_and_corruption_is_rejected() -> io::Result<()> {
        let secret = b"test identity key";
        let protected = protect_secret(secret)?;
        assert_ne!(protected, secret);
        assert_eq!(unprotect_secret(&protected)?, secret);
        let mut corrupted = protected;
        corrupted[0] ^= 1;
        assert!(unprotect_secret(&corrupted).is_err());
        Ok(())
    }

    // User identity is stable while caller-controlled strings cannot escape the pipe name.
    #[test]
    fn pipe_name_is_stable_and_rejects_path_characters() -> io::Result<()> {
        assert_eq!(user_pipe_name("test")?, user_pipe_name("test")?);
        assert!(user_pipe_name("../other").is_err());
        assert!(user_pipe_name("").is_err());
        Ok(())
    }

    // A same-user client must still connect after the restrictive DACL is applied.
    #[tokio::test]
    async fn private_pipe_accepts_owner_client() -> io::Result<()> {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::windows::named_pipe::ClientOptions,
        };
        let name = user_pipe_name(&format!("test-{}", std::process::id()))?;
        let server = create_user_pipe(&name, true)?;
        let mut client = ClientOptions::new().open(&name)?;
        server.connect().await?;
        let mut server = server;
        client.write_all(b"ok").await?;
        let mut received = [0; 2];
        server.read_exact(&mut received).await?;
        assert_eq!(&received, b"ok");
        assert!(create_user_pipe(&name, true).is_err());
        Ok(())
    }
    // A restrictive ACL must preserve owner access and reject non-directory inputs.
    #[test]
    fn private_directory_stays_accessible_to_its_owner() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("fta-acl-test-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let result = (|| {
            secure_directory(&root)?;
            // Inspect the applied kernel ACL, not just whether the creating account can write.
            let sddl = directory_sddl(&root)?;
            assert!(
                sddl.starts_with("D:P"),
                "DACL must reject inherited permissions: {sddl}"
            );
            assert_eq!(
                sddl.matches('(').count(),
                1,
                "only the owner ACE may remain"
            );
            // Windows may serialize the same SID as an SDDL alias (for example LA).
            // Normalize the single ACE's trustee instead of depending on its spelling.
            let trustee = sddl
                .rsplit_once(';')
                .and_then(|(_, trustee)| trustee.strip_suffix(')'))
                .expect("the single ACE must contain a trustee SID");
            assert_eq!(canonical_sid(trustee)?, current_user_sid()?, "{sddl}");
            let child = root.join("owner.txt");
            std::fs::write(&child, b"private")?;
            assert_eq!(std::fs::read(&child)?, b"private");
            assert!(secure_directory(&child).is_err());
            std::fs::remove_file(child)?;
            Ok(())
        })();
        std::fs::remove_dir(root)?;
        result
    }
    // Well-known aliases and numeric SIDs identify the same trustee on all Windows accounts;
    // normalization must still distinguish different principals rather than skipping the check.
    #[test]
    fn trustee_aliases_preserve_sid_identity() -> io::Result<()> {
        assert_eq!(canonical_sid("SY")?, "S-1-5-18");
        assert_eq!(canonical_sid("BA")?, "S-1-5-32-544");
        assert_eq!(canonical_sid("S-1-5-18")?, "S-1-5-18");
        assert_ne!(canonical_sid("SY")?, canonical_sid("BA")?);
        // LA is machine-relative, so obtain its SID from Windows rather than a fixture account.
        let administrator = canonical_sid("LA")?;
        assert_ne!(administrator, "LA");
        assert_eq!(canonical_sid(&administrator)?, administrator);
        let owner = current_user_sid()?;
        assert_eq!(canonical_sid(&owner)?, owner);
        assert!(canonical_sid("not-a-sid").is_err());
        Ok(())
    }
    fn canonical_sid(value: &str) -> io::Result<String> {
        use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;

        let value: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let mut sid = ptr::null_mut();
        // SAFETY: value is terminated and sid is a valid output pointer. Windows resolves
        // supported SDDL aliases as well as numeric strings without changing any permissions.
        if unsafe { ConvertStringSidToSidW(value.as_ptr(), &mut sid) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut text = ptr::null_mut();
        // SAFETY: sid remains live until this synchronous conversion returns.
        let status = unsafe { ConvertSidToStringSidW(sid, &mut text) };
        let error = io::Error::last_os_error();
        // SAFETY: ConvertStringSidToSidW requires LocalFree for its SID allocation.
        unsafe { LocalFree(sid) };
        if status == 0 {
            return Err(error);
        }
        // SAFETY: successful conversion returns a NUL-terminated UTF-16 allocation.
        let result = unsafe { String::from_utf16_lossy(wide_slice(text)) };
        // SAFETY: the converted string belongs to this call and uses LocalFree.
        unsafe { LocalFree(text.cast()) };
        Ok(result)
    }
    fn directory_sddl(path: &Path) -> io::Result<String> {
        use std::{
            fs::OpenOptions,
            os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        };
        use windows_sys::Win32::{
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo,
                    SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION,
            },
            Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS,
        };
        const READ_CONTROL_ACCESS: u32 = 0x0002_0000;
        let directory = OpenOptions::new()
            .access_mode(READ_CONTROL_ACCESS)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let mut descriptor = ptr::null_mut();
        // SAFETY: directory owns a valid handle and descriptor is a writable out pointer.
        let error = unsafe {
            GetSecurityInfo(
                directory.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let mut text = ptr::null_mut();
        // SAFETY: descriptor is the live result of GetSecurityInfo; text is an output pointer.
        let status = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION,
                DACL_SECURITY_INFORMATION,
                &mut text,
                ptr::null_mut(),
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: GetSecurityInfo allocates this descriptor with LocalAlloc.
        unsafe { LocalFree(descriptor) };
        if status == 0 {
            return Err(error);
        }
        // SAFETY: successful conversion returns a NUL-terminated UTF-16 allocation.
        let result = unsafe { String::from_utf16_lossy(wide_slice(text)) };
        // SAFETY: the converted string must be released through LocalFree.
        unsafe { LocalFree(text.cast()) };
        Ok(result)
    }
    // Native publication must also work without hard-link support and preserve an occupied target.
    #[test]
    fn publish_rename_never_clobbers_an_existing_destination() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("fta-publish-test-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let source = root.join("partial");
        let destination = root.join("received");
        let result = (|| {
            std::fs::write(&source, b"new")?;
            std::fs::write(&destination, b"old")?;
            assert!(publish_noreplace(&source, &destination).is_err());
            assert_eq!(std::fs::read(&destination)?, b"old");
            assert_eq!(std::fs::read(&source)?, b"new");
            std::fs::remove_file(&destination)?;
            publish_noreplace(&source, &destination)?;
            assert!(!source.exists());
            assert_eq!(std::fs::read(&destination)?, b"new");
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }
}
