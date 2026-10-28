//! Unpackaged desktop toasts use a Start-menu AUMID/stub CLSID and OS protocol activation.
//!
//! Microsoft documents this alternative to an in-process COM activator. It intentionally has
//! no reply-input support; its protocol payload is sufficient to reopen a received record.

use crate::notification_payload::{
    can_update_protocol, protocol_toast, validate_app_id, validate_scheme,
};
use std::{
    io,
    path::{Path, PathBuf},
};
use windows::{
    Data::Xml::Dom::XmlDocument,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    Win32::{
        Foundation::PROPERTYKEY,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IPersistFile, STGM_READ,
                StructuredStorage::{
                    InitPropVariantFromCLSID, PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0,
                    PROPVARIANT_0_0_0, PropVariantClear, PropVariantToStringAlloc,
                },
            },
            Variant::VT_LPWSTR,
            WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
        },
        UI::Shell::{
            IShellLinkW, PropertiesSystem::IPropertyStore, SHCNE_ASSOCCHANGED, SHCNF_IDLIST,
            SHChangeNotify, SetCurrentProcessExplicitAppUserModelID, ShellLink,
        },
    },
    core::{GUID, HSTRING, Interface, PCWSTR, PWSTR},
};

// These are Windows SDK property identifiers, not installation-specific paths or credentials.
const APP_USER_MODEL_FMTID: GUID = GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3);
const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_USER_MODEL_FMTID,
    pid: 5,
};
const APP_TOAST_ACTIVATOR: PROPERTYKEY = PROPERTYKEY {
    fmtid: APP_USER_MODEL_FMTID,
    pid: 26,
};

fn native_error(error: windows::core::Error) -> io::Error {
    io::Error::other(error.to_string())
}

// A fresh WinRT apartment avoids depending on the Tauri UI thread's existing COM mode.
fn in_apartment(operation: impl FnOnce() -> io::Result<()> + Send + 'static) -> io::Result<()> {
    std::thread::spawn(move || {
        // SAFETY: this new thread has no previous COM initialization; success is balanced below.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(native_error)?;
        struct Apartment;
        impl Drop for Apartment {
            fn drop(&mut self) {
                // SAFETY: Apartment is created only after successful initialization on this thread.
                unsafe { RoUninitialize() };
            }
        }
        let _apartment = Apartment;
        operation()
    })
    .join()
    .map_err(|_| io::Error::other("native notification thread stopped unexpectedly"))?
}

/// Register per-user routing and a real Start-menu shortcut before submitting any toasts.
/// The caller supplies the installed executable and the Tauri application identifier.
pub fn register(
    executable: &Path,
    app_id: &str,
    display_name: &str,
    scheme: &str,
) -> io::Result<()> {
    validate_app_id(app_id)?;
    validate_scheme(scheme)?;
    if display_name.is_empty()
        || display_name.contains(['\0', '/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid notification application name",
        ));
    }
    // std canonicalize adds a verbatim prefix that IShellLinkW::SetPath rejects even for short paths.
    // Resolve links first, then simplify only when ordinary DOS/UNC syntax preserves the same target.
    let executable = dunce::canonicalize(executable)?;
    if !executable.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "notification executable must be a file",
        ));
    }
    let app_id = app_id.to_string();
    let display_name = display_name.to_string();
    let scheme = scheme.to_string();
    in_apartment(move || {
        register_protocol(&executable, &app_id, &display_name, &scheme)?;
        register_shortcut(&executable, &app_id)?;
        let app_id = HSTRING::from(app_id);
        // SAFETY: app_id is a live UTF-16 string; the API copies the process identity.
        unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id.as_ptr())) }
            .map_err(native_error)?;
        // Windows caches URI handlers. Registration changes must invalidate that cache before clicks.
        // SAFETY: SHCNE_ASSOCCHANGED requires SHCNF_IDLIST and null item pointers, including for protocols.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        Ok(())
    })
}

fn register_protocol(
    executable: &Path,
    app_id: &str,
    display_name: &str,
    scheme: &str,
) -> io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let executable = executable.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "notification executable path must be Unicode",
        )
    })?;
    if executable.contains(['"', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid notification executable path",
        ));
    }
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let expected_command = format!("\"{executable}\" \"%1\"");
    let (protocol, disposition) = root.create_subkey(format!("Software\\Classes\\{scheme}"))?;
    const OWNER_VALUE: &str = "FTAApplicationId";
    if disposition == winreg::enums::REG_OPENED_EXISTING_KEY {
        let owner: Option<String> = protocol.get_value(OWNER_VALUE).ok();
        let existing_command: Option<String> = protocol
            .open_subkey("shell\\open\\command")
            .ok()
            .and_then(|command| command.get_value("").ok());
        if !can_update_protocol(
            owner.as_deref(),
            existing_command.as_deref(),
            app_id,
            &expected_command,
        ) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "activation scheme is already registered by another application",
            ));
        }
    }
    protocol.set_value(OWNER_VALUE, &app_id)?;
    protocol.set_value("", &format!("URL:{display_name}"))?;
    protocol.set_value("URL Protocol", &"")?;
    let (command, _) = protocol.create_subkey("shell\\open\\command")?;
    // Windows launches this executable directly; no cmd.exe or user-controlled shell template is used.
    command.set_value("", &expected_command)?;
    let (identity, _) =
        root.create_subkey(format!("Software\\Classes\\AppUserModelId\\{app_id}"))?;
    identity.set_value("DisplayName", &display_name)?;
    Ok(())
}

fn shortcut_app_id(path: &Path) -> io::Result<String> {
    // SAFETY: this helper runs inside the initialized registration apartment.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(native_error)?;
    let persist: IPersistFile = link.cast().map_err(native_error)?;
    let path = HSTRING::from(path.as_os_str());
    // SAFETY: path is a live terminated UTF-16 string; loading is read-only.
    unsafe { persist.Load(PCWSTR(path.as_ptr()), STGM_READ) }.map_err(native_error)?;
    let properties: IPropertyStore = link.cast().map_err(native_error)?;
    // SAFETY: property store is initialized and the key identifies a valid AUMID property.
    let mut value = unsafe { properties.GetValue(&APP_USER_MODEL_ID) }.map_err(native_error)?;
    // SAFETY: value is the live owned property variant returned by GetValue.
    let text = unsafe { PropVariantToStringAlloc(&value) };
    // SAFETY: the property variant owns the allocation that GetValue returned.
    let cleared = unsafe { PropVariantClear(&mut value) };
    let text = text.map_err(native_error)?;
    // SAFETY: successful conversion returned a NUL-terminated allocated string.
    let result = unsafe { text.to_string() };
    // SAFETY: PropVariantToStringAlloc documents CoTaskMemFree as its matching deallocator.
    unsafe { CoTaskMemFree(Some(text.0.cast())) };
    cleared.map_err(native_error)?;
    result.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn register_shortcut(executable: &Path, app_id: &str) -> io::Result<()> {
    // The shell supplies this path even when the user's Start-menu folder is relocated.
    let programs = super::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Programs)?;
    std::fs::create_dir_all(&programs)?;
    // A unique app-identifier filename avoids overwriting another product's similarly named shortcut.
    let shortcut: PathBuf = programs.join(format!("{app_id}.lnk"));
    if shortcut.exists() && shortcut_app_id(&shortcut)? != app_id {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "notification shortcut belongs to another application",
        ));
    }
    let executable = HSTRING::from(executable.as_os_str());
    let shortcut = HSTRING::from(shortcut.as_os_str());
    let app_id_string = HSTRING::from(app_id);
    let activator = GUID::from_u128(
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, app_id.as_bytes()).as_u128(),
    );
    // SAFETY: COM is initialized on this thread and ShellLink is the registered in-process class.
    let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
        .map_err(native_error)?;
    // Keep the failing shell operation in diagnostics: several APIs return the same E_INVALIDARG.
    // SAFETY: both UTF-16 strings are valid and remain live during synchronous calls.
    unsafe { link.SetPath(PCWSTR(executable.as_ptr())) }
        .map_err(|error| io::Error::other(format!("set notification shortcut target: {error}")))?;
    let properties: IPropertyStore = link.cast().map_err(native_error)?;
    // PROPVARIANT's Drop frees its string; suppress it for this borrowed HSTRING so registration
    // cannot free the HSTRING's interior buffer and corrupt the heap after SetValue copies it.
    let id_variant = std::mem::ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    pwszVal: PWSTR(app_id_string.as_ptr().cast_mut()),
                },
            }),
        },
    });
    // SAFETY: the property key and borrowed string variant remain valid until SetValue returns.
    unsafe { properties.SetValue(&APP_USER_MODEL_ID, &*id_variant) }
        .map_err(|error| io::Error::other(format!("set notification shortcut AUMID: {error}")))?;
    // SAFETY: activator is a valid GUID and the API allocates an owned PROPVARIANT.
    let mut activator_variant =
        unsafe { InitPropVariantFromCLSID(&activator) }.map_err(native_error)?;
    // SAFETY: the owned variant and static property key remain live throughout SetValue.
    let result = unsafe { properties.SetValue(&APP_TOAST_ACTIVATOR, &activator_variant) };
    // SAFETY: this is exactly the owned variant returned by InitPropVariantFromCLSID.
    let cleared = unsafe { PropVariantClear(&mut activator_variant) };
    result.map_err(|error| {
        io::Error::other(format!("set notification shortcut activator: {error}"))
    })?;
    cleared.map_err(native_error)?;
    // SAFETY: the store and link are initialized COM objects in this thread's apartment.
    unsafe { properties.Commit() }.map_err(|error| {
        io::Error::other(format!("commit notification shortcut properties: {error}"))
    })?;
    let persist: IPersistFile = link.cast().map_err(native_error)?;
    // SAFETY: shortcut is a live absolute path to this application's Start-menu link.
    unsafe { persist.Save(PCWSTR(shortcut.as_ptr()), true) }
        .map_err(|error| io::Error::other(format!("save notification shortcut: {error}")))
}

/// Submit a protocol-activation toast whose click is handled by Windows after this process exits.
pub fn show(
    app_id: &str,
    scheme: &str,
    transfer_id: &str,
    title: &str,
    body: &str,
) -> io::Result<()> {
    validate_app_id(app_id)?;
    let payload = protocol_toast(scheme, transfer_id, title, body)?;
    let app_id = app_id.to_string();
    in_apartment(move || {
        let xml = XmlDocument::new().map_err(native_error)?;
        xml.LoadXml(&HSTRING::from(payload)).map_err(native_error)?;
        let toast = ToastNotification::CreateToastNotification(&xml).map_err(native_error)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
            .map_err(native_error)?
            .Show(&toast)
            .map_err(native_error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests register only a unique test namespace and remove every registry/shortcut artifact.
    struct TestRegistration {
        app_id: String,
        scheme: String,
        foreign_fixture: bool,
    }
    impl Drop for TestRegistration {
        fn drop(&mut self) {
            use winreg::{RegKey, enums::HKEY_CURRENT_USER};
            let root = RegKey::predef(HKEY_CURRENT_USER);
            let owner: Option<String> = root
                .open_subkey(format!("Software\\Classes\\{}", self.scheme))
                .ok()
                .and_then(|key| key.get_value("FTAApplicationId").ok());
            if !self.foreign_fixture && owner.as_deref() != Some(self.app_id.as_str()) {
                return;
            }
            let _ = root.delete_subkey_all(format!("Software\\Classes\\{}", self.scheme));
            let _ = root.delete_subkey_all(format!(
                "Software\\Classes\\AppUserModelId\\{}",
                self.app_id
            ));
            if let Ok(programs) =
                super::super::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Programs)
            {
                let _ = std::fs::remove_file(programs.join(format!("{}.lnk", self.app_id)));
            }
        }
    }
    fn fixture() -> TestRegistration {
        let seed = format!("{}-{:?}", std::process::id(), std::time::SystemTime::now());
        let suffix = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes())
            .simple()
            .to_string();
        TestRegistration {
            app_id: format!("FTA.Notification.Tests.{suffix}"),
            scheme: format!("fta-test-{}", &suffix[..16]),
            foreign_fixture: false,
        }
    }

    // Native Windows CI verifies actual property-store persistence and idempotent ownership checks.
    #[test]
    fn registration_creates_owned_shortcut_and_can_be_repeated() -> io::Result<()> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let fixture = fixture();
        let executable = std::env::current_exe()?;
        register(
            &executable,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )?;
        register(
            &executable,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )?;
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!("Software\\Classes\\{}", fixture.scheme))?;
        assert_eq!(
            key.get_value::<String, _>("FTAApplicationId")?,
            fixture.app_id
        );
        // The OS protocol launcher must receive the same shell-compatible path as the saved link.
        let command: String = key.open_subkey("shell\\open\\command")?.get_value("")?;
        assert!(!command.contains(r"\\?\"));
        let programs =
            super::super::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Programs)?;
        assert!(programs.join(format!("{}.lnk", fixture.app_id)).is_file());
        Ok(())
    }

    // A reinstall may delete the old executable while leaving the owned notification shortcut behind.
    // Re-registration must move both the URI command and native shortcut to the new installation.
    #[test]
    fn registration_repairs_an_owned_shortcut_after_installation_moves() -> io::Result<()> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let fixture = fixture();
        let directory = tempfile::tempdir()?;
        let old_executable = directory.path().join("old installation").join("fta.exe");
        let new_executable = directory.path().join("new installation").join("fta.exe");
        for path in [&old_executable, &new_executable] {
            std::fs::create_dir_all(path.parent().expect("test installation parent"))?;
            std::fs::write(path, b"notification registration fixture; never executed")?;
        }
        register(
            &old_executable,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )?;
        std::fs::remove_file(&old_executable)?;
        register(
            &new_executable,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )?;
        let expected = dunce::canonicalize(&new_executable)?;
        let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(
            "Software\\Classes\\{}\\shell\\open\\command",
            fixture.scheme
        ))?;
        assert_eq!(
            key.get_value::<String, _>("")?,
            format!("\"{}\" \"%1\"", expected.display())
        );
        let app_id = fixture.app_id.clone();
        in_apartment(move || {
            let programs =
                super::super::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Programs)?;
            let shortcut = HSTRING::from(programs.join(format!("{app_id}.lnk")).as_os_str());
            // SAFETY: this test worker owns an initialized COM apartment and a live shortcut path.
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
                    .map_err(native_error)?;
            let persist: IPersistFile = link.cast().map_err(native_error)?;
            // SAFETY: shortcut owns a live UTF-16 path; loading the test-owned shortcut is read-only.
            unsafe { persist.Load(PCWSTR(shortcut.as_ptr()), STGM_READ) }.map_err(native_error)?;
            const MAX_TARGET_UNITS: usize = 32_768;
            let mut target = vec![0_u16; MAX_TARGET_UNITS];
            // SAFETY: the writable UTF-16 buffer is live; no find-data structure is requested.
            unsafe { link.GetPath(&mut target, std::ptr::null_mut(), 0) }.map_err(native_error)?;
            let end = target
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(target.len());
            let actual = String::from_utf16(&target[..end]).map_err(io::Error::other)?;
            assert!(same_file::is_same_file(actual, expected)?);
            Ok(())
        })
    }

    // NSIS creates an unowned handler before first launch. Canonical path aliases must identify
    // our executable, while extra arguments and explicit foreign ownership must still be rejected.
    #[test]
    fn installer_protocol_alias_is_adopted_without_relaxing_ownership() -> io::Result<()> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let fixture = fixture();
        let executable = std::env::current_exe()?;
        let alias = executable.canonicalize()?;
        let command = format!("\"{}\" \"%1\"", alias.display());
        let expected = format!("\"{}\" \"%1\"", dunce::canonicalize(&executable)?.display());
        assert!(can_update_protocol(
            None,
            Some(&command),
            &fixture.app_id,
            &expected
        ));
        assert!(!can_update_protocol(
            None,
            Some(&format!("{command} --unexpected")),
            &fixture.app_id,
            &expected,
        ));
        assert!(!can_update_protocol(
            Some("another.application"),
            Some(&command),
            &fixture.app_id,
            &expected,
        ));
        let root = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = root.create_subkey(format!("Software\\Classes\\{}", fixture.scheme))?;
        key.create_subkey("shell\\open\\command")?
            .0
            .set_value("", &command)?;
        register(
            &executable,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )?;
        assert_eq!(
            key.get_value::<String, _>("FTAApplicationId")?,
            fixture.app_id
        );
        Ok(())
    }

    // Sharing a scheme does not authorize overwriting another program's registration.
    #[test]
    fn foreign_protocol_is_preserved_without_creating_a_shortcut() -> io::Result<()> {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let mut fixture = fixture();
        let (key, disposition) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!("Software\\Classes\\{}", fixture.scheme))?;
        if disposition != winreg::enums::REG_CREATED_NEW_KEY {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "test namespace was already occupied",
            ));
        }
        fixture.foreign_fixture = true;
        key.set_value("FTAApplicationId", &"another.application")?;
        let (command, _) = key.create_subkey("shell\\open\\command")?;
        command.set_value("", &"other.exe")?;
        let error = register(
            &std::env::current_exe()?,
            &fixture.app_id,
            "FTA notification test",
            &fixture.scheme,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(command.get_value::<String, _>("")?, "other.exe");
        Ok(())
    }
}
