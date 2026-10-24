//! Native identity/config contracts are isolated from tests that intentionally use ephemeral keys.

// Only Windows executes this path; cross-compilation is not presented as runtime verification.
#[cfg(windows)]
mod windows {
    use fta_core::config::atomic_write;
    use std::{
        fs, io,
        os::windows::fs::OpenOptionsExt,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    // A failed replacement must preserve the previous config rather than deleting it first.
    #[test]
    fn replacement_failure_preserves_previous_configuration() {
        const FILE_SHARE_READ: u32 = 1;
        const FILE_SHARE_WRITE: u32 = 2;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        atomic_write(&path, b"previous").unwrap();
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .unwrap();
        assert!(atomic_write(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"previous");
        drop(held);
    }

    // Concurrent readers must never observe the remove/rename gap or partially written JSON.
    #[test]
    fn replacing_configuration_has_no_missing_file_window() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        atomic_write(&path, b"old").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reader_stop = stop.clone();
        let reader_path = path.clone();
        let reader = std::thread::spawn(move || -> io::Result<()> {
            while !reader_stop.load(Ordering::Acquire) {
                let bytes = fs::read(&reader_path)?;
                assert!(bytes == b"old" || bytes == b"new");
            }
            Ok(())
        });
        for _ in 0..100 {
            atomic_write(&path, b"new").unwrap();
        }
        stop.store(true, Ordering::Release);
        reader.join().unwrap().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"new");
    }
}
