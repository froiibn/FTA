//! Persisted event positions must survive restarts without replaying private message bodies.
use fta_core::{
    config::AppPaths,
    model::{Transfer, now},
    storage::Store,
};
use serde_json::json;

fn paths(root: &std::path::Path) -> AppPaths {
    AppPaths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
        runtime_dir: root.join("runtime"),
        default_receive_dir: root.join("inbox"),
    }
}
fn transfer(id: &str, state: &str) -> Transfer {
    serde_json::from_value(json!({"transfer_id":id,"peer_id":"peer","direction":"receive","kind":"files","state":state,"created_at":now(),"body":"private message","items":[{"name":"file.txt","size":"1000","state":"pending","path":"/private/path","bytes_transferred":"0"}]})).unwrap()
}
#[test]
fn completion_events_and_acknowledgements_survive_reopen_without_duplicate_events() {
    let d = tempfile::tempdir().unwrap();
    let paths = paths(d.path());
    paths.prepare().unwrap();
    let store = Store::open(&paths).unwrap();
    let record = transfer("one", "completed");
    store.save_transfer(&record).unwrap();
    store.save_transfer(&record).unwrap();
    let page = store.events_after(0, 100).unwrap();
    let events = page["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "transfer.completed");
    assert!(events[0]["record"].get("body").is_none());
    assert!(events[0]["record"]["items"][0].get("path").is_none());
    let sequence = events[0]["sequence"].as_u64().unwrap();
    store
        .acknowledge_events("desktop-notifications", sequence)
        .unwrap();
    drop(store);
    let store = Store::open(&paths).unwrap();
    assert_eq!(
        store.event_cursor("desktop-notifications").unwrap(),
        sequence
    );
    store
        .acknowledge_events("desktop-notifications", 0)
        .unwrap();
    assert_eq!(
        store.event_cursor("desktop-notifications").unwrap(),
        sequence
    );
    assert!(
        store.events_after(sequence, 100).unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .acknowledge_events("desktop-notifications", sequence + 1)
            .is_err()
    );
    store.save_transfer(&transfer("two", "partial")).unwrap();
    assert_eq!(
        store.events_after(sequence, 1).unwrap()["events"][0]["transfer_id"],
        "two"
    );
}
#[test]
fn byte_progress_is_monotonic_bounded_and_cannot_reopen_terminal_records() {
    let d = tempfile::tempdir().unwrap();
    let paths = paths(d.path());
    paths.prepare().unwrap();
    let store = Store::open(&paths).unwrap();
    let mut record = transfer("progress", "receiving");
    store.save_transfer(&record).unwrap();
    store.update_progress("progress", 0, 500).unwrap();
    store.update_progress("progress", 0, 100).unwrap();
    assert_eq!(
        store.transfer("progress").unwrap().items[0].bytes_transferred,
        "500"
    );
    store.update_progress("progress", 0, 2000).unwrap();
    assert_eq!(
        store.transfer("progress").unwrap().items[0].bytes_transferred,
        "1000"
    );
    assert!(
        store.events_after(0, 100).unwrap()["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    record.state = "completed".into();
    record.items[0].state = "completed".into();
    record.items[0].bytes_transferred = "1000".into();
    store.save_transfer(&record).unwrap();
    store.update_progress("progress", 0, 1).unwrap();
    assert_eq!(store.transfer("progress").unwrap().state, "completed");
    assert_eq!(
        store.transfer("progress").unwrap().items[0].bytes_transferred,
        "1000"
    );
}
#[test]
fn atomic_rename_publishes_without_hard_links_and_refuses_existing_destinations() {
    let d = tempfile::tempdir().unwrap();
    let source = d.path().join("temp");
    let target = d.path().join("final");
    std::fs::write(&source, b"new").unwrap();
    std::fs::write(&target, b"old").unwrap();
    assert!(fta_platform::publish_noreplace(&source, &target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    assert_eq!(std::fs::read(&source).unwrap(), b"new");
    std::fs::remove_file(&target).unwrap();
    fta_platform::publish_noreplace(&source, &target).unwrap();
    assert_eq!(std::fs::read(target).unwrap(), b"new");
    assert!(!source.exists());
}

#[test]
fn restart_retries_failed_partial_cleanup_without_deleting_received_files() {
    let d = tempfile::tempdir().unwrap();
    let paths = paths(d.path());
    paths.prepare().unwrap();
    let store = Store::open(&paths).unwrap();
    let mut record = transfer("cleanup", "receiving");
    let root = paths.default_receive_dir.clone();
    store.reserve_receive(&record, &root).unwrap();
    let partial = root.join(".fta-partials/cleanup-0.part");
    let final_file = root.join("kept.txt");
    std::fs::write(&partial, b"incomplete").unwrap();
    std::fs::write(&final_file, b"keep").unwrap();
    record.state = "failed".into();
    store.save_transfer(&record).unwrap();
    store.recover().unwrap();
    assert!(!partial.exists());
    assert_eq!(std::fs::read(final_file).unwrap(), b"keep");
}
