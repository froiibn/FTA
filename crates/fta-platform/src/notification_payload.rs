//! Notification activation carries only a validated transfer ID, never an executable or remote URL.

use std::io;

const MAX_SCHEME_LENGTH: usize = 32;
const MAX_APP_ID_LENGTH: usize = 128;
const MAX_TITLE_CHARS: usize = 128;
const MAX_BODY_CHARS: usize = 512;

/// A dedicated scheme must not replace a system browser, file, or mail handler.
pub fn validate_scheme(scheme: &str) -> io::Result<()> {
    let mut chars = scheme.chars();
    if scheme.is_empty()
        || scheme.len() > MAX_SCHEME_LENGTH
        || !chars
            .next()
            .is_some_and(|character| character.is_ascii_lowercase())
        || !chars.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '+' | '-' | '.')
        })
        || ["http", "https", "file", "mailto", "shell", "ms-settings"].contains(&scheme)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid application activation scheme",
        ));
    }
    Ok(())
}

/// AUMID also forms a registry subkey, so path separators and empty identifiers are rejected.
pub fn validate_app_id(app_id: &str) -> io::Result<()> {
    if app_id.is_empty()
        || app_id.len() > MAX_APP_ID_LENGTH
        || !app_id.bytes().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, b'.' | b'-' | b'_')
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid application user model ID",
        ));
    }
    Ok(())
}

/// Canonical UUID formatting prevents markup, arguments, and path traversal entering activation data.
pub fn activation_uri(scheme: &str, transfer_id: &str) -> io::Result<String> {
    validate_scheme(scheme)?;
    let transfer_id = uuid::Uuid::parse_str(transfer_id).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid notification transfer ID",
        )
    })?;
    Ok(format!("{scheme}://inbox/{transfer_id}"))
}

/// Protocol activation is intentional: an in-process callback cannot handle a cold-start click.
pub fn protocol_toast(
    scheme: &str,
    transfer_id: &str,
    title: &str,
    body: &str,
) -> io::Result<String> {
    let uri = activation_uri(scheme, transfer_id)?;
    let title = escape_xml(title, MAX_TITLE_CHARS);
    let body = escape_xml(body, MAX_BODY_CHARS);
    Ok(format!(
        "<toast activationType=\"protocol\" launch=\"{uri}\"><visual><binding template=\"ToastGeneric\"><text>{title}</text><text>{body}</text></binding></visual></toast>"
    ))
}

/// Existing third-party handlers are never replaced merely because they chose the same scheme.
#[cfg(any(windows, test))]
pub(crate) fn can_update_protocol(
    owner: Option<&str>,
    command: Option<&str>,
    app_id: &str,
    expected_command: &str,
) -> bool {
    match owner {
        Some(owner) => owner == app_id,
        None => command == Some(expected_command),
    }
}

fn escape_xml(text: &str, limit: usize) -> String {
    let mut escaped = String::new();
    for character in text
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t' | '\r'))
        .take(limit)
    {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            character => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    const TRANSFER_ID: &str = "79a59d1e-6c92-47a7-bc1b-cd09ae82791c";

    // The OS launch data must describe the exact local record and contain no user-controlled markup.
    #[test]
    fn protocol_payload_escapes_text_and_uses_only_canonical_transfer_id() {
        let xml = protocol_toast("fta", TRANSFER_ID, "<Photo & Video>", "\"열기\"\0").unwrap();
        assert!(xml.contains(&format!(
            "activationType=\"protocol\" launch=\"fta://inbox/{TRANSFER_ID}\""
        )));
        assert!(xml.contains("&lt;Photo &amp; Video&gt;"));
        assert!(xml.contains("&quot;열기&quot;"));
        assert!(!xml.contains('\0'));
        assert!(protocol_toast("fta", "../run.exe", "title", "body").is_err());
    }

    // Native protocol registration is restricted to the application's own scheme and registry key.
    #[test]
    fn reserved_schemes_and_registry_paths_are_rejected() {
        for scheme in ["http", "https", "file", "fta\\evil", "fta\"", "", "1fta"] {
            assert!(validate_scheme(scheme).is_err());
        }
        for app_id in ["", "FTA\\shell", "FTA/other", "FTA\0"] {
            assert!(validate_app_id(app_id).is_err());
        }
        assert!(validate_app_id("app.fta.local-transfer").is_ok());
    }
    // Upgrades may change the executable path only when our explicit app ownership marker matches.
    #[test]
    fn protocol_collision_preserves_another_app_handler() {
        assert!(!can_update_protocol(
            Some("another.app"),
            Some("other.exe"),
            "fta.app",
            "fta.exe"
        ));
        assert!(!can_update_protocol(
            None,
            Some("other.exe"),
            "fta.app",
            "fta.exe"
        ));
        assert!(can_update_protocol(
            Some("fta.app"),
            Some("old-fta.exe"),
            "fta.app",
            "fta.exe"
        ));
        assert!(can_update_protocol(
            None,
            Some("fta.exe"),
            "fta.app",
            "fta.exe"
        ));
    }
}
