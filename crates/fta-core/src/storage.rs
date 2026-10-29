//! Durable records and no-clobber commits prevent successful transfers from hiding data loss.
use crate::{
    Error, Result,
    config::{AppPaths, private_dir},
    model::{Device, Transfer, now},
};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_HISTORY_RECORDS: usize = 10_000;
const MAX_HISTORY_MESSAGE_BYTES: u64 = 100 * 1024 * 1024;
const HISTORY_RETENTION_SECONDS: u64 = 30 * 24 * 60 * 60;
const MAX_HISTORY_QUERY: usize = 1000;
const FILE_HASH_BUFFER_BYTES: usize = 64 * 1024;
const MAX_FILENAME_BYTES: usize = 180;
const MAX_FILENAME_ATTEMPTS: usize = 10_000;
const MAX_EVENT_PAGE_ITEMS: usize = 100;
const MAX_EVENT_CONSUMER_NAME_BYTES: usize = 128;

/// SQLite owns transfer origins and pinned receive roots, never mutable UI settings.
pub struct Store {
    db: Mutex<Connection>,
}
impl Store {
    /// WAL plus FULL synchronization makes recorded commit intent durable before file publication.
    pub fn open(paths: &AppPaths) -> Result<Self> {
        let mut db = Connection::open(paths.state_dir.join("history.sqlite3"))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY, data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS transfers(id TEXT PRIMARY KEY, data TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL DEFAULT 0,
                message_bytes INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS transfer_roots(transfer_id TEXT PRIMARY KEY REFERENCES transfers(id) ON DELETE CASCADE,
                root TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                transfer_id TEXT NOT NULL REFERENCES transfers(id) ON DELETE CASCADE,
                kind TEXT NOT NULL, created_at INTEGER NOT NULL, summary TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS event_consumers(consumer TEXT PRIMARY KEY, sequence INTEGER NOT NULL);
            ")?;
        // Early prototypes had JSON-only rows; backfill query metadata without discarding history.
        let columns = {
            let mut query = db.prepare("PRAGMA table_info(transfers)")?;
            query
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (name, definition) in [
            ("state", "TEXT NOT NULL DEFAULT ''"),
            ("created_at", "INTEGER NOT NULL DEFAULT 0"),
            ("message_bytes", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !columns.iter().any(|column| column == name) {
                db.execute_batch(&format!(
                    "ALTER TABLE transfers ADD COLUMN {name} {definition}"
                ))?;
            }
        }
        let old_rows = {
            let mut query = db.prepare("SELECT data FROM transfers WHERE state=''")?;
            query
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let tx = db.transaction()?;
        for row in old_rows {
            let transfer: Transfer = serde_json::from_str(&row)?;
            save_transfer_row(&tx, &transfer)?;
        }
        tx.commit()?;
        Ok(Self { db: Mutex::new(db) })
    }
    /// iOS can relocate an app sandbox during installation. Rebase only its owned receive root,
    /// never arbitrary historical destinations, and keep event/cursor identities unchanged.
    pub fn relocate_receive_root(&self, previous: &Path, current: &Path) -> Result<()> {
        if !previous.is_absolute() || !current.is_absolute() {
            return Err(Error::new(
                "INVALID_PATH",
                "absolute receive roots required",
            ));
        }
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let records = {
            let mut query = tx.prepare("SELECT transfers.data FROM transfers JOIN transfer_roots ON transfers.id=transfer_roots.transfer_id WHERE transfer_roots.root=?1")?;
            query
                .query_map([previous.to_string_lossy().as_ref()], |row| {
                    row.get::<_, String>(0)
                })?
                .map(|row| Ok(serde_json::from_str::<Transfer>(&row?)?))
                .collect::<Result<Vec<_>>>()?
        };
        for mut transfer in records {
            if transfer.direction != "receive" {
                return Err(Error::new("CONFLICT", "sender cannot own a receive root"));
            }
            for item in &mut transfer.items {
                if let Some(path) = &item.path {
                    let relative = Path::new(path)
                        .strip_prefix(previous)
                        .map_err(|_| Error::new("INVALID_PATH", "item outside receive root"))?;
                    if relative.components().count() != 1
                        || !relative
                            .components()
                            .all(|part| matches!(part, std::path::Component::Normal(_)))
                    {
                        return Err(Error::new("INVALID_PATH", "invalid stored receive path"));
                    }
                    item.path = Some(current.join(relative).to_string_lossy().into());
                }
            }
            save_transfer_row(&tx, &transfer)?;
            tx.execute(
                "UPDATE events SET summary=?1 WHERE transfer_id=?2",
                params![
                    transfer_summary(&transfer)?.to_string(),
                    transfer.transfer_id
                ],
            )?;
        }
        tx.execute(
            "UPDATE transfer_roots SET root=?1 WHERE root=?2",
            params![current.to_string_lossy(), previous.to_string_lossy()],
        )?;
        tx.commit()?;
        Ok(())
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| Error::new("DATABASE_ERROR", "database lock poisoned"))
    }
    /// Read registrations without interpreting mutable names as identity.
    pub fn devices(&self) -> Result<Vec<Device>> {
        let db = self.lock()?;
        let mut query = db.prepare("SELECT data FROM devices ORDER BY id")?;
        let rows = query.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    /// A reused device ID cannot silently replace an already trusted authentication key.
    pub fn save_device(&self, device: &Device) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT data FROM devices WHERE id=?1",
                [&device.device_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            let previous: Device = serde_json::from_str(&previous)?;
            if !crate::identity::same_public_key(
                &hex::decode(&previous.certificate).unwrap_or_default(),
                &hex::decode(&device.certificate).unwrap_or_default(),
            ) {
                return Err(Error::new(
                    "KEY_CHANGED",
                    "registered device key changed; remove registration before pairing again",
                ));
            }
        }
        tx.execute("INSERT INTO devices(id,data) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET data=excluded.data",params![device.device_id,serde_json::to_string(device)?])?;
        tx.commit()?;
        Ok(())
    }
    /// Removing trust erases the registration instead of keeping an inactive key alias.
    pub fn unpair(&self, id: &str) -> Result<()> {
        self.lock()?
            .execute("DELETE FROM devices WHERE id=?1", [id])?;
        Ok(())
    }
    /// Prefix selection is accepted only when exactly one device matches.
    pub fn device(&self, id: &str) -> Result<Device> {
        let matches: Vec<_> = self
            .devices()?
            .into_iter()
            .filter(|device| device.device_id.starts_with(id))
            .collect();
        if matches.len() != 1 {
            return Err(Error::new(
                "UNTRUSTED",
                "device ID must identify exactly one registered device",
            ));
        }
        matches
            .into_iter()
            .next()
            .ok_or_else(|| Error::new("UNTRUSTED", "device not registered"))
    }
    /// Initial receiver reservation is atomic with its immutable root and origin metadata.
    pub fn reserve_receive(&self, transfer: &Transfer, root: &Path) -> Result<Transfer> {
        if transfer.direction != "receive" {
            return Err(Error::new(
                "INVALID_REQUEST",
                "only receiver records reserve a receive root",
            ));
        }
        let root = receiving_dir(root)?;
        let root = root
            .to_str()
            .ok_or_else(|| Error::new("INVALID_PATH", "receive directory must be valid Unicode"))?;
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        if let Some(previous) = transfer_row(&tx, &transfer.transfer_id)? {
            ensure_same_origin(&previous, transfer)?;
            tx.commit()?;
            return Ok(previous);
        }
        save_transfer_row(&tx, transfer)?;
        tx.execute(
            "INSERT INTO transfer_roots(transfer_id,root) VALUES(?1,?2)",
            params![transfer.transfer_id, root],
        )?;
        tx.commit()?;
        Ok(transfer.clone())
    }
    /// Later body/commit operations stay in the original folder even if settings change.
    pub fn receive_root(&self, id: &str) -> Result<PathBuf> {
        self.lock()?
            .query_row(
                "SELECT root FROM transfer_roots WHERE transfer_id=?1",
                [id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(PathBuf::from)
            .ok_or_else(|| Error::new("NOT_FOUND", "receive destination was not reserved"))
    }
    /// Origin checks prevent two peers or sender/receiver jobs from overwriting the same ID.
    pub fn save_transfer(&self, transfer: &Transfer) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let previous = transfer_row(&tx, &transfer.transfer_id)?;
        if let Some(previous) = &previous {
            ensure_same_origin(previous, transfer)?;
        }
        save_transfer_row(&tx, transfer)?;
        if terminal_state(&transfer.state)
            && previous
                .as_ref()
                .is_none_or(|previous| previous.state != transfer.state)
        {
            let summary = transfer_summary(transfer)?;
            tx.execute(
                "INSERT INTO events(transfer_id,kind,created_at,summary) VALUES(?1,?2,?3,?4)",
                params![
                    transfer.transfer_id,
                    format!("transfer.{}", transfer.state),
                    now(),
                    serde_json::to_string(&summary)?
                ],
            )?;
        }
        if terminal_state(&transfer.state) {
            prune(&tx)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Byte updates touch only the latest item's counter; completion and another item's state cannot be overwritten.
    pub fn update_progress(&self, id: &str, index: usize, bytes: u64) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let mut transfer =
            transfer_row(&tx, id)?.ok_or_else(|| Error::new("NOT_FOUND", "transfer not found"))?;
        if terminal_state(&transfer.state) || transfer.state == "unknown" {
            return Ok(());
        }
        let item = transfer
            .items
            .get_mut(index)
            .ok_or_else(|| Error::new("INVALID_REQUEST", "progress item does not exist"))?;
        if item.state == "completed" {
            return Ok(());
        }
        let size = item
            .size
            .parse::<u64>()
            .map_err(|_| Error::new("INVALID_REQUEST", "invalid item size"))?;
        let previous = item.bytes_transferred.parse::<u64>().unwrap_or(0);
        item.bytes_transferred = previous.max(bytes.min(size)).to_string();
        save_transfer_row(&tx, &transfer)?;
        tx.commit()?;
        Ok(())
    }
    /// Revocation/network loss durably terminates active work; partial files remain distinguishable from completed files.
    pub fn interrupt_active(&self, peer_id: Option<&str>, reason: &str) -> Result<()> {
        let records = {
            let db = self.lock()?;
            let mut query=db.prepare("SELECT data FROM transfers WHERE state IN ('sending','receiving','verifying','committing')")?;
            query
                .query_map([], |row| row.get::<_, String>(0))?
                .map(|row| Ok(serde_json::from_str::<Transfer>(&row?)?))
                .collect::<Result<Vec<_>>>()?
        };
        for mut transfer in records {
            if peer_id.is_some_and(|id| id != transfer.peer_id) {
                continue;
            }
            let root = self.receive_root(&transfer.transfer_id).ok();
            for (index, item) in transfer.items.iter_mut().enumerate() {
                if item.state == "completed" {
                    continue;
                }
                // A commit in its crash window requires recovery, not a guess that the final file was absent.
                if item.state == "committing" {
                    continue;
                }
                item.state = "failed".into();
                item.error = Some(reason.into());
                if let Some(root) = &root {
                    let path =
                        partials_dir(root)?.join(format!("{}-{index}.part", transfer.transfer_id));
                    match fs::remove_file(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            eprintln!("interrupted partial cleanup failed: {}", error.kind())
                        }
                    }
                }
            }
            transfer.state = if transfer.direction == "send" {
                "unknown".into()
            } else if transfer.items.iter().any(|item| item.state == "committing") {
                "committing".into()
            } else {
                aggregate(&transfer.items)
            };
            transfer.error = Some(reason.into());
            self.save_transfer(&transfer)?;
        }
        Ok(())
    }
    /// Durable summaries are small enough to replay without embedding message bodies or absolute paths.
    pub fn events_after(&self, after: u64, limit: usize) -> Result<serde_json::Value> {
        if limit == 0 || limit > MAX_EVENT_PAGE_ITEMS {
            return Err(Error::new(
                "INVALID_REQUEST",
                "event page size must be 1..100",
            ));
        }
        let db = self.lock()?;
        let mut query=db.prepare("SELECT sequence,transfer_id,kind,created_at,summary FROM events WHERE sequence>?1 ORDER BY sequence LIMIT ?2")?;
        let rows = query.query_map(params![after, limit], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut events = Vec::new();
        let mut next = after;
        for row in rows {
            let (sequence, transfer_id, kind, created_at, summary) = row?;
            next = sequence;
            events.push(serde_json::json!({"sequence":sequence,"transfer_id":transfer_id,"kind":kind,"created_at":created_at,"record":serde_json::from_str::<serde_json::Value>(&summary)?}));
        }
        Ok(serde_json::json!({"events":events,"next_cursor":next}))
    }
    pub fn event_cursor(&self, consumer: &str) -> Result<u64> {
        validate_consumer(consumer)?;
        Ok(self
            .lock()?
            .query_row(
                "SELECT sequence FROM event_consumers WHERE consumer=?1",
                [consumer],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }
    /// Acknowledgements are monotonic and cannot skip into an event sequence that has never existed.
    pub fn acknowledge_events(&self, consumer: &str, sequence: u64) -> Result<()> {
        validate_consumer(consumer)?;
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let high: u64 = tx.query_row(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'),0)",
            [],
            |row| row.get(0),
        )?;
        if sequence > high {
            return Err(Error::new(
                "INVALID_REQUEST",
                "event cursor is ahead of the log",
            ));
        }
        tx.execute("INSERT INTO event_consumers(consumer,sequence) VALUES(?1,?2) ON CONFLICT(consumer) DO UPDATE SET sequence=MAX(sequence,excluded.sequence)",params![consumer,sequence])?;
        tx.commit()?;
        Ok(())
    }
    /// Exact IDs avoid applying a remote result to a similarly named transfer.
    pub fn transfer(&self, id: &str) -> Result<Transfer> {
        transfer_row(&*self.lock()?, id)?
            .ok_or_else(|| Error::new("NOT_FOUND", "transfer record not found"))
    }
    /// UI pagination is bounded independently of recovery, which must inspect all active rows.
    pub fn history(&self, limit: usize) -> Result<Vec<Transfer>> {
        let db = self.lock()?;
        let mut query = db.prepare("SELECT data FROM transfers ORDER BY rowid DESC LIMIT ?1")?;
        let rows = query.query_map([limit.min(MAX_HISTORY_QUERY) as u64], |row| {
            row.get::<_, String>(0)
        })?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    /// Record deletion never implies deletion of the independently owned received file.
    pub fn delete_record(&self, id: &str) -> Result<()> {
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        let record = transfer_row(&tx, id)?
            .ok_or_else(|| Error::new("NOT_FOUND", "transfer record not found"))?;
        if !terminal_state(&record.state) {
            return Err(Error::new(
                "BUSY",
                "active or unconfirmed transfer records cannot be deleted",
            ));
        }
        tx.execute("DELETE FROM transfers WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    /// Recovery scans every interrupted row, including records older than one history page.
    pub fn recover(&self) -> Result<()> {
        let records = {
            let db = self.lock()?;
            let mut query=db.prepare("SELECT data FROM transfers WHERE state IN ('receiving','sending','committing','verifying')")?;
            let rows = query.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|row| Ok(serde_json::from_str::<Transfer>(&row?)?))
                .collect::<Result<Vec<_>>>()?
        };
        for mut transfer in records {
            let root = self.receive_root(&transfer.transfer_id).ok();
            for (index, item) in transfer.items.iter_mut().enumerate() {
                if item.state == "committing"
                    && let (Some(path), Some(expected), Some(root)) =
                        (&item.path, &item.sha256, &root)
                {
                    let partial =
                        partials_dir(root)?.join(format!("{}-{index}.part", transfer.transfer_id));
                    // Intent is written before link allocation. If a partial still exists,
                    // equal contents alone cannot prove an existing destination belongs to us.
                    let published = match fs::symlink_metadata(&partial) {
                        Ok(_) => same_file::is_same_file(&partial, path).unwrap_or(false),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                        Err(_) => false,
                    };
                    if published && hash_file(Path::new(path)).is_ok_and(|hash| &hash == expected) {
                        item.state = "completed".into();
                        item.saved_name = Path::new(path)
                            .file_name()
                            .map(|name| name.to_string_lossy().into());
                    }
                }
                if item.state != "completed" {
                    item.state = "failed".into();
                    item.error = Some("INTERRUPTED".into());
                }
                if let Some(root) = &root {
                    let partial =
                        partials_dir(root)?.join(format!("{}-{index}.part", transfer.transfer_id));
                    match fs::remove_file(partial) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            transfer.state = if transfer.direction == "send" {
                "unknown".into()
            } else {
                aggregate(&transfer.items)
            };
            self.save_transfer(&transfer)?;
        }
        // A prior delete can fail while Windows/antivirus still holds a partial open. Retry after restart.
        let terminal_records = {
            let db = self.lock()?;
            let mut query=db.prepare("SELECT transfers.data FROM transfers JOIN transfer_roots ON transfers.id=transfer_roots.transfer_id WHERE transfers.state IN ('completed','partial','failed','cancelled')")?;
            query
                .query_map([], |row| row.get::<_, String>(0))?
                .map(|row| Ok(serde_json::from_str::<Transfer>(&row?)?))
                .collect::<Result<Vec<_>>>()?
        };
        for transfer in terminal_records {
            let root = self.receive_root(&transfer.transfer_id)?;
            for index in 0..transfer.items.len() {
                let partial = crate::storage::partials_dir(&root)?
                    .join(format!("{}-{index}.part", transfer.transfer_id));
                match fs::remove_file(partial) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => eprintln!("stale partial cleanup failed: {}", error.kind()),
                }
            }
        }
        let mut db = self.lock()?;
        let tx = db.transaction()?;
        prune(&tx)?;
        tx.commit()?;
        Ok(())
    }
}

fn validate_consumer(consumer: &str) -> Result<()> {
    if consumer.is_empty()
        || consumer.len() > MAX_EVENT_CONSUMER_NAME_BYTES
        || !consumer
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(Error::new("INVALID_REQUEST", "invalid event consumer name"));
    }
    Ok(())
}
fn transfer_summary(transfer: &Transfer) -> Result<serde_json::Value> {
    let mut summary = serde_json::to_value(transfer)?;
    if let Some(object) = summary.as_object_mut() {
        object.remove("body");
    }
    if let Some(items) = summary
        .get_mut("items")
        .and_then(serde_json::Value::as_array_mut)
    {
        for item in items {
            if let Some(object) = item.as_object_mut() {
                object.remove("path");
            }
        }
    }
    Ok(summary)
}
fn transfer_row(db: &Connection, id: &str) -> Result<Option<Transfer>> {
    let row: Option<String> = db
        .query_row("SELECT data FROM transfers WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    row.map(|row| serde_json::from_str(&row).map_err(Into::into))
        .transpose()
}
fn ensure_same_origin(previous: &Transfer, next: &Transfer) -> Result<()> {
    if previous.peer_id != next.peer_id
        || previous.direction != next.direction
        || previous.kind != next.kind
        || !previous
            .items
            .iter()
            .map(|item| (&item.name, &item.size))
            .eq(next.items.iter().map(|item| (&item.name, &item.size)))
    {
        return Err(Error::new(
            "CONFLICT",
            "transfer ID belongs to different origin or metadata",
        ));
    }
    Ok(())
}
fn save_transfer_row(db: &Connection, transfer: &Transfer) -> Result<()> {
    let bytes = transfer.body.as_ref().map_or(0, |body| body.len() as u64);
    db.execute("INSERT INTO transfers(id,data,state,created_at,message_bytes) VALUES(?1,?2,?3,?4,?5)
        ON CONFLICT(id) DO UPDATE SET data=excluded.data,state=excluded.state,created_at=excluded.created_at,message_bytes=excluded.message_bytes",
        params![transfer.transfer_id,serde_json::to_string(transfer)?,transfer.state,transfer.created_at,bytes])?;
    Ok(())
}
fn terminal_state(state: &str) -> bool {
    matches!(state, "completed" | "partial" | "failed" | "cancelled")
}
fn prune(db: &Connection) -> Result<()> {
    // Protect in-flight and unknown results; only completed records count against retention limits.
    let candidates = {
        let mut query=db.prepare("SELECT id,created_at,message_bytes FROM transfers WHERE state IN ('completed','partial','failed','cancelled') ORDER BY created_at DESC,rowid DESC")?;
        query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    let cutoff = now().saturating_sub(HISTORY_RETENTION_SECONDS);
    let mut bytes = 0u64;
    for (index, (id, created, message_bytes)) in candidates.into_iter().enumerate() {
        bytes = bytes.saturating_add(message_bytes);
        if created < cutoff || index >= MAX_HISTORY_RECORDS || bytes > MAX_HISTORY_MESSAGE_BYTES {
            db.execute("DELETE FROM transfers WHERE id=?1", [id])?;
        }
    }
    Ok(())
}
pub fn aggregate(items: &[crate::model::Item]) -> String {
    let complete = items.iter().filter(|i| i.state == "completed").count();
    if complete == items.len() && !items.is_empty() {
        "completed"
    } else if complete > 0 {
        "partial"
    } else {
        "failed"
    }
    .into()
}
pub fn hash_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; FILE_HASH_BUFFER_BYTES];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
/// Normalize every remote filename with the strictest supported platform's restrictions.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean: String = base
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    clean = clean.trim_end_matches(['.', ' ']).to_string();
    while clean.len() > MAX_FILENAME_BYTES {
        clean.pop();
    }
    let stem = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(['1', '2', '3', '4', '5', '6', '7', '8', '9']));
    if clean.is_empty() || clean == "." || clean == ".." {
        clean = "file".into();
    }
    if reserved {
        clean.insert(0, '_');
    }
    clean
}
/// iOS exposes Documents in Files, so incomplete bytes belong in private Application Support.
/// Desktop layouts stay unchanged so existing recovery records retain their original meaning.
pub fn partials_dir(root: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        use sha2::Digest;
        let base = fta_platform::ios::paths()?.state_dir.join("partials");
        Ok(base.join(hex::encode(sha2::Sha256::digest(
            root.as_os_str().as_encoded_bytes(),
        ))))
    }
    #[cfg(not(target_os = "ios"))]
    {
        Ok(root.join(".fta-partials"))
    }
}

pub fn receiving_dir(root: &Path) -> Result<PathBuf> {
    if fs::symlink_metadata(root).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(Error::new(
            "INVALID_PATH",
            "receive directory must not be a symlink",
        ));
    }
    fs::create_dir_all(root)?;
    let root = fs::canonicalize(root)?;
    private_dir(&crate::storage::partials_dir(&root)?)?;
    Ok(root)
}
/// Publish atomically without overwriting; no-replace rename supports filesystems without hard links.
pub fn commit_path(
    temp: &Path,
    root: &Path,
    name: &str,
    mut intent: impl FnMut(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let safe = safe_name(name);
    let p = Path::new(&safe);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    for n in 0..MAX_FILENAME_ATTEMPTS {
        let target = root.join(if n == 0 {
            safe.clone()
        } else {
            format!("{stem} ({n}){ext}")
        });
        // Avoid recording a known collision as this transfer's intended publication.
        match fs::symlink_metadata(&target) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        intent(&target)?;
        match fs::hard_link(temp, &target) {
            Ok(()) => {
                // Persist publication before deleting the only temporary directory entry.
                sync_directory(root)?;
                fs::remove_file(temp)?;
                if let Some(parent) = temp.parent() {
                    sync_directory(parent)?;
                }
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => match fta_platform::publish_noreplace(temp, &target) {
                Ok(()) => {
                    sync_directory(root)?;
                    if let Some(parent) = temp.parent() {
                        sync_directory(parent)?;
                    }
                    return Ok(target);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            },
        }
    }
    Err(Error::new(
        "CONFLICT",
        "could not allocate an unused filename",
    ))
}
/// Unix directory fsync persists filename changes; Windows has no portable directory flush API.
fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_cannot_escape_or_use_windows_devices() {
        for (input, expected) in [
            ("../../photo.jpg", "photo.jpg"),
            ("..\\CON.txt", "_CON.txt"),
            ("..", "file"),
            ("nul", "_nul"),
        ] {
            assert_eq!(safe_name(input), expected);
        }
        assert!(!safe_name("a\u{1b}b").contains('\u{1b}'));
    }
    #[test]
    fn collision_commit_preserves_existing_and_normalizes_multibyte() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("tmp");
        fs::write(&tmp, b"new").unwrap();
        fs::write(dir.path().join("a.txt"), b"old").unwrap();
        let saved = commit_path(&tmp, dir.path(), "../a.txt", |_| Ok(())).unwrap();
        assert_eq!(fs::read(dir.path().join("a.txt")).unwrap(), b"old");
        assert_eq!(fs::read(saved).unwrap(), b"new");
        assert!(safe_name(&"한".repeat(100)).len() <= 180);
    }
    fn store_fixture() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            config_dir: dir.path().join("config"),
            state_dir: dir.path().join("state"),
            runtime_dir: dir.path().join("run"),
            default_receive_dir: dir.path().join("inbox"),
        };
        paths.prepare().unwrap();
        let store = Store::open(&paths).unwrap();
        (dir, store)
    }
    fn receiver_record() -> Transfer {
        Transfer {
            transfer_id: uuid::Uuid::new_v4().to_string(),
            peer_id: "sender".into(),
            direction: "receive".into(),
            kind: "files".into(),
            state: "receiving".into(),
            created_at: now(),
            body: None,
            error: None,
            items: vec![crate::model::Item {
                bytes_transferred: "0".into(),
                name: "photo.jpg".into(),
                size: "3".into(),
                state: "pending".into(),
                saved_name: None,
                path: None,
                sha256: None,
                error: None,
            }],
        }
    }
    // Reservation is the boundary: changing user settings must never redirect an in-flight item.
    #[test]
    fn receive_root_is_immutable_and_foreign_origin_cannot_replace_it() {
        let (dir, store) = store_fixture();
        let transfer = receiver_record();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        store.reserve_receive(&transfer, &first).unwrap();
        store.reserve_receive(&transfer, &second).unwrap();
        assert_eq!(
            store.receive_root(&transfer.transfer_id).unwrap(),
            first.canonicalize().unwrap()
        );
        let mut foreign = transfer.clone();
        foreign.peer_id = "other".into();
        assert_eq!(
            store.reserve_receive(&foreign, &second).unwrap_err().code,
            "CONFLICT"
        );
        assert_eq!(store.save_transfer(&foreign).unwrap_err().code, "CONFLICT");
        assert_eq!(
            store.transfer(&transfer.transfer_id).unwrap().peer_id,
            transfer.peer_id
        );
    }
    // Terminal history expires without deleting files; uncertain or in-flight results remain queryable.
    #[test]
    fn retention_protects_active_and_unknown_and_preserves_received_files() {
        let (dir, store) = store_fixture();
        let mut active = receiver_record();
        active.created_at = 0;
        store.save_transfer(&active).unwrap();
        let mut unknown = receiver_record();
        unknown.state = "unknown".into();
        unknown.created_at = 0;
        store.save_transfer(&unknown).unwrap();
        let mut expired = receiver_record();
        expired.state = "completed".into();
        expired.created_at = 0;
        let saved = dir.path().join("saved.jpg");
        fs::write(&saved, b"old").unwrap();
        expired.items[0].path = Some(saved.to_string_lossy().into());
        store.save_transfer(&expired).unwrap();
        assert!(store.transfer(&expired.transfer_id).is_err());
        assert!(store.transfer(&active.transfer_id).is_ok());
        assert!(store.transfer(&unknown.transfer_id).is_ok());
        assert_eq!(fs::read(saved).unwrap(), b"old");
        assert_eq!(
            store.delete_record(&unknown.transfer_id).unwrap_err().code,
            "BUSY"
        );
    }
    // Recovery must not skip an old active row just because a UI page has 1000 newer records.
    #[test]
    fn recovery_scans_beyond_history_page_and_cleans_partials() {
        let (dir, store) = store_fixture();
        let transfer = receiver_record();
        let root = dir.path().join("inbox");
        store.reserve_receive(&transfer, &root).unwrap();
        let partial = root
            .join(".fta-partials")
            .join(format!("{}-0.part", transfer.transfer_id));
        fs::write(&partial, b"bad").unwrap();
        {
            let mut db = store.lock().unwrap();
            let tx = db.transaction().unwrap();
            for _ in 0..=MAX_HISTORY_QUERY {
                let mut newer = receiver_record();
                newer.state = "completed".into();
                save_transfer_row(&tx, &newer).unwrap();
            }
            tx.commit().unwrap();
        }
        store.recover().unwrap();
        assert_eq!(
            store.transfer(&transfer.transfer_id).unwrap().state,
            "failed"
        );
        assert!(!partial.exists());
    }
    // A delete must cascade private destination bookkeeping while leaving the user's file alone.
    #[test]
    fn deleting_record_removes_root_mapping() {
        let (dir, store) = store_fixture();
        let mut transfer = receiver_record();
        store
            .reserve_receive(&transfer, &dir.path().join("inbox"))
            .unwrap();
        transfer.state = "cancelled".into();
        store.save_transfer(&transfer).unwrap();
        store.delete_record(&transfer.transfer_id).unwrap();
        assert!(store.receive_root(&transfer.transfer_id).is_err());
    }
    // Existing equal-content files are not evidence that the interrupted hard link succeeded.
    #[test]
    fn recovery_distinguishes_preexisting_content_from_published_hardlink() {
        let (dir, store) = store_fixture();
        for published in [false, true] {
            let mut transfer = receiver_record();
            let root = dir
                .path()
                .join(if published { "published" } else { "collision" });
            store.reserve_receive(&transfer, &root).unwrap();
            let partial = root
                .join(".fta-partials")
                .join(format!("{}-0.part", transfer.transfer_id));
            let target = root.join("photo.jpg");
            fs::write(&partial, b"same").unwrap();
            if published {
                fs::hard_link(&partial, &target).unwrap();
            } else {
                fs::write(&target, b"same").unwrap();
            }
            transfer.state = "committing".into();
            transfer.items[0].state = "committing".into();
            transfer.items[0].path = Some(target.to_string_lossy().into());
            transfer.items[0].sha256 = Some(hash_file(&partial).unwrap());
            store.save_transfer(&transfer).unwrap();
            store.recover().unwrap();
            assert_eq!(
                store.transfer(&transfer.transfer_id).unwrap().state,
                if published { "completed" } else { "failed" }
            );
            assert_eq!(fs::read(target).unwrap(), b"same");
            assert!(!partial.exists());
        }
    }
    // Relocation must preserve IDs/cursors and must not rewrite another configured destination.
    #[test]
    fn sandbox_relocation_rebases_only_owned_receiver_paths() {
        let (temp, store) = store_fixture();
        let root = temp.path().canonicalize().unwrap();
        let old = root.join("old");
        let new = root.join("new");
        let mut record = receiver_record();
        store.reserve_receive(&record, &old).unwrap();
        record.items[0].path = Some(old.join("file.txt").to_string_lossy().into());
        record.items[0].state = "completed".into();
        record.state = "completed".into();
        store.save_transfer(&record).unwrap();
        let mut other = receiver_record();
        other.transfer_id = uuid::Uuid::new_v4().to_string();
        let other_root = root.join("other");
        store.reserve_receive(&other, &other_root).unwrap();
        let events = store.events_after(0, 100).unwrap();
        store.relocate_receive_root(&old, &new).unwrap();
        assert_eq!(store.receive_root(&record.transfer_id).unwrap(), new);
        assert_eq!(
            store.transfer(&record.transfer_id).unwrap().items[0]
                .path
                .as_deref(),
            new.join("file.txt").to_str()
        );
        assert_eq!(
            store.events_after(0, 100).unwrap()["next_cursor"],
            events["next_cursor"]
        );
        assert_eq!(store.receive_root(&other.transfer_id).unwrap(), other_root);
        assert_eq!(store.event_cursor("ios-notifications-v1").unwrap(), 0);
    }
}
