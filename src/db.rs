use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::Utc;
use rusqlite::{Connection, OpenFlags};

use crate::{
    crypto::SecretBox,
    error::{IoContext, Result, invalid},
};

#[derive(Clone)]
pub struct Store {
    path: PathBuf,
    secrets: SecretBox,
}

impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        if root.is_symlink() {
            return Err(invalid("The SOC state directory must not be a symlink"));
        }
        fs::create_dir_all(root).at_path("Cannot create", root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))
                .at_path("Cannot protect", root)?;
        }
        let path = root.join("soc.sqlite3");
        if path.is_symlink() {
            return Err(invalid("The SOC database must not be a symlink"));
        }
        let store = Self {
            path,
            secrets: SecretBox::load(root)?,
        };
        store.with(|connection| {
            connection.execute_batch(
                r"
                CREATE TABLE IF NOT EXISTS accounts (
                    owner TEXT PRIMARY KEY,
                    mailbox TEXT NOT NULL,
                    client_id TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    refresh_token BLOB NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS device_flows (
                    id TEXT PRIMARY KEY,
                    owner TEXT NOT NULL,
                    mailbox TEXT NOT NULL,
                    client_id TEXT NOT NULL,
                    tenant_id TEXT NOT NULL,
                    device_code BLOB NOT NULL,
                    user_code TEXT NOT NULL,
                    verification_uri TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    interval_seconds INTEGER NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS device_flows_owner ON device_flows(owner);
                CREATE TABLE IF NOT EXISTS events (
                    owner TEXT NOT NULL,
                    event_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    document BLOB NOT NULL,
                    since TEXT NOT NULL,
                    state TEXT NOT NULL DEFAULT 'waiting_request',
                    request_id TEXT,
                    request_time TEXT,
                    draft_id TEXT,
                    send_started TEXT,
                    sent_at TEXT,
                    update_id TEXT,
                    update_thread TEXT,
                    update_time TEXT,
                    reply_id TEXT,
                    outcome TEXT,
                    details TEXT NOT NULL DEFAULT '',
                    link TEXT NOT NULL DEFAULT '',
                    error TEXT NOT NULL DEFAULT '',
                    recipient TEXT NOT NULL,
                    subject TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(owner, event_id)
                );
                CREATE INDEX IF NOT EXISTS events_pending ON events(owner, state, since);
                CREATE TABLE IF NOT EXISTS idempotency (
                    owner TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    request_key TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY(owner, operation, request_key)
                );
                CREATE INDEX IF NOT EXISTS idempotency_created ON idempotency(created_at);
                ",
            )?;
            Ok(())
        })?;
        Ok(store)
    }

    pub fn with<T>(&self, operation: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut connection = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        #[cfg(unix)]
        if self.path.is_file() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
                .at_path("Cannot protect", &self.path)?;
        }
        operation(&mut connection)
    }

    pub fn seal(&self, value: &str) -> Result<Vec<u8>> {
        self.secrets.seal(value.as_bytes())
    }

    pub fn open_secret(&self, value: &[u8]) -> Result<String> {
        String::from_utf8(self.secrets.open(value)?)
            .map_err(|_| invalid("The saved Microsoft session is not valid UTF-8"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn ready(&self) -> Result<()> {
        self.with(|connection| {
            connection.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        })
    }

    pub fn claim_once(
        &self,
        owner: &str,
        operation: &str,
        request_key: &str,
        retention: Duration,
    ) -> Result<bool> {
        self.with(|connection| {
            let transaction = connection.transaction()?;
            let retention = chrono::Duration::from_std(retention)
                .map_err(|_| invalid("The idempotency retention is invalid"))?;
            transaction.execute(
                "DELETE FROM idempotency WHERE created_at < ?1",
                [(Utc::now() - retention).to_rfc3339()],
            )?;
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO idempotency (owner,operation,request_key,created_at) VALUES (?1,?2,?3,?4)",
                (owner, operation, request_key, Utc::now().to_rfc3339()),
            )?;
            transaction.commit()?;
            Ok(inserted == 1)
        })
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    #[test]
    fn initialises_private_wal_storage() {
        let root = std::env::temp_dir().join(format!("soc-db-{}", Uuid::new_v4()));
        let store = Store::open(&root).expect("store");
        store
            .with(|connection| {
                let tables: i64 = connection.query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name = 'events'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(tables, 1);
                Ok(())
            })
            .expect("schema");
        assert!(store.path().is_file());
        assert!(store.ready().is_ok());
        assert!(
            store
                .claim_once("owner", "submit", "same", Duration::from_secs(60))
                .expect("first claim")
        );
        assert!(
            !store
                .claim_once("owner", "submit", "same", Duration::from_secs(60))
                .expect("duplicate claim")
        );
        fs::remove_dir_all(root).expect("clean state");
    }
}
