//! One-shot Windows UAC target; the desktop and engine themselves remain unelevated.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    // Accept only the platform adapter's fixed pipe protocol, never an arbitrary shell/action.
    #[cfg(windows)]
    if let Err(error) = fta_platform::windows::network_setup::helper_main(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }
    // Other OS builds retain headless workspace compatibility without pretending to configure
    // Windows or pulling GUI dependencies into the Linux engine.
    #[cfg(not(windows))]
    {
        eprintln!("This helper is available only on Windows.");
        std::process::exit(1);
    }
}
