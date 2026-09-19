//! Durable mapping from Buzz conversation scopes to provider ACP sessions.
//!
//! The provider owns the session history. This store only remembers the opaque
//! ACP `sessionId` needed to ask a capable adapter to load that history after a
//! `buzz-acp` restart.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection, OptionalExtension};

use crate::scope::SessionScope;

const MAX_REVISION_LEN: usize = 128;
const MAX_SESSION_ID_LEN: usize = 4_096;
const MAX_BINDINGS_PER_NAMESPACE: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionStoreError {
    #[error("session store path has no parent directory")]
    MissingParent,
    #[error("invalid session revision: {0}")]
    InvalidRevision(String),
    #[error("invalid ACP session id")]
    InvalidSessionId,
    #[error("session store lock poisoned")]
    LockPoisoned,
    #[error("session store task failed: {0}")]
    Task(String),
    #[error("session store I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session store SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Clone)]
pub(crate) struct SessionStore {
    connection: Arc<Mutex<Connection>>,
    agent_pubkey: Arc<str>,
    relay_url: Arc<str>,
    revision: Arc<str>,
}

impl SessionStore {
    pub(crate) fn open(
        path: &Path,
        agent_pubkey: &str,
        relay_url: &str,
        revision: &str,
    ) -> Result<Self, SessionStoreError> {
        let revision = validate_revision(revision)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or(SessionStoreError::MissingParent)?;
        std::fs::create_dir_all(parent)?;

        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS acp_session_bindings (
                agent_pubkey TEXT NOT NULL,
                relay_url TEXT NOT NULL,
                revision TEXT NOT NULL,
                scope_key TEXT NOT NULL,
                adapter_name TEXT NOT NULL,
                session_id TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (agent_pubkey, relay_url, revision, scope_key)
            ) WITHOUT ROWID;",
        )?;

        // A revision is an explicit compatibility fence. Once this process
        // starts on a new revision, older bindings for the same agent/community
        // can no longer be resumed safely and are removed transactionally.
        let tx = connection.transaction()?;
        tx.execute(
            "DELETE FROM acp_session_bindings
             WHERE agent_pubkey = ?1 AND relay_url = ?2 AND revision <> ?3",
            params![agent_pubkey, relay_url, revision],
        )?;
        tx.commit()?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)?.permissions();
            permissions.set_mode(0o600);
            std::fs::set_permissions(path, permissions)?;
        }

        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            agent_pubkey: Arc::from(agent_pubkey),
            relay_url: Arc::from(relay_url),
            revision: Arc::from(revision),
        })
    }

    pub(crate) async fn lookup(
        &self,
        scope: &SessionScope,
        adapter_name: &str,
    ) -> Result<Option<String>, SessionStoreError> {
        let connection = Arc::clone(&self.connection);
        let agent_pubkey = Arc::clone(&self.agent_pubkey);
        let relay_url = Arc::clone(&self.relay_url);
        let revision = Arc::clone(&self.revision);
        let scope_key = scope_key(scope);
        let adapter_name = adapter_name.to_string();
        run_store_task(move || {
            let connection = connection
                .lock()
                .map_err(|_| SessionStoreError::LockPoisoned)?;
            let session_id = connection
                .query_row(
                    "SELECT session_id FROM acp_session_bindings
                     WHERE agent_pubkey = ?1 AND relay_url = ?2 AND revision = ?3
                       AND scope_key = ?4 AND adapter_name = ?5",
                    params![
                        agent_pubkey.as_ref(),
                        relay_url.as_ref(),
                        revision.as_ref(),
                        scope_key,
                        adapter_name
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            Ok(session_id.filter(|id| !id.is_empty() && id.len() <= MAX_SESSION_ID_LEN))
        })
        .await
    }

    pub(crate) async fn put(
        &self,
        scope: &SessionScope,
        adapter_name: &str,
        session_id: &str,
    ) -> Result<(), SessionStoreError> {
        if session_id.is_empty() || session_id.len() > MAX_SESSION_ID_LEN {
            return Err(SessionStoreError::InvalidSessionId);
        }
        let connection = Arc::clone(&self.connection);
        let agent_pubkey = Arc::clone(&self.agent_pubkey);
        let relay_url = Arc::clone(&self.relay_url);
        let revision = Arc::clone(&self.revision);
        let scope_key = scope_key(scope);
        let adapter_name = adapter_name.to_string();
        let session_id = session_id.to_string();
        run_store_task(move || {
            let mut connection = connection
                .lock()
                .map_err(|_| SessionStoreError::LockPoisoned)?;
            let tx = connection.transaction()?;
            tx.execute(
                "INSERT INTO acp_session_bindings
                    (agent_pubkey, relay_url, revision, scope_key, adapter_name, session_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch())
                 ON CONFLICT(agent_pubkey, relay_url, revision, scope_key) DO UPDATE SET
                    adapter_name = excluded.adapter_name,
                    session_id = excluded.session_id,
                    updated_at = excluded.updated_at",
                params![
                    agent_pubkey.as_ref(),
                    relay_url.as_ref(),
                    revision.as_ref(),
                    scope_key,
                    adapter_name,
                    session_id
                ],
            )?;
            tx.execute(
                "DELETE FROM acp_session_bindings
                 WHERE agent_pubkey = ?1 AND relay_url = ?2 AND revision = ?3
                   AND scope_key IN (
                     SELECT scope_key FROM acp_session_bindings
                     WHERE agent_pubkey = ?1 AND relay_url = ?2 AND revision = ?3
                     ORDER BY updated_at DESC, scope_key DESC
                     LIMIT -1 OFFSET ?4
                   )",
                params![
                    agent_pubkey.as_ref(),
                    relay_url.as_ref(),
                    revision.as_ref(),
                    MAX_BINDINGS_PER_NAMESPACE as i64
                ],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub(crate) async fn remove(&self, scope: &SessionScope) -> Result<(), SessionStoreError> {
        let connection = Arc::clone(&self.connection);
        let agent_pubkey = Arc::clone(&self.agent_pubkey);
        let relay_url = Arc::clone(&self.relay_url);
        let revision = Arc::clone(&self.revision);
        let scope_key = scope_key(scope);
        run_store_task(move || {
            let connection = connection
                .lock()
                .map_err(|_| SessionStoreError::LockPoisoned)?;
            connection.execute(
                "DELETE FROM acp_session_bindings
                 WHERE agent_pubkey = ?1 AND relay_url = ?2 AND revision = ?3 AND scope_key = ?4",
                params![
                    agent_pubkey.as_ref(),
                    relay_url.as_ref(),
                    revision.as_ref(),
                    scope_key
                ],
            )?;
            Ok(())
        })
        .await
    }
}

async fn run_store_task<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, SessionStoreError> + Send + 'static,
) -> Result<T, SessionStoreError> {
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| SessionStoreError::Task(error.to_string()))?
}

fn validate_revision(revision: &str) -> Result<String, SessionStoreError> {
    let revision = revision.trim();
    if revision.is_empty()
        || revision.len() > MAX_REVISION_LEN
        || revision.chars().any(char::is_control)
    {
        return Err(SessionStoreError::InvalidRevision(revision.to_string()));
    }
    Ok(revision.to_string())
}

fn scope_key(scope: &SessionScope) -> String {
    match scope {
        SessionScope::Conversation { channel_id } => format!("channel:{channel_id}"),
        SessionScope::Thread {
            channel_id,
            root_event_id,
        } => format!("thread:{channel_id}:{root_event_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "buzz-acp-session-store-{label}-{}.sqlite3",
            Uuid::new_v4()
        ))
    }

    fn conversation() -> SessionScope {
        SessionScope::Conversation {
            channel_id: Uuid::new_v4(),
        }
    }

    #[tokio::test]
    async fn binding_survives_reopen_and_uses_wal() {
        let path = test_path("reopen");
        let scope = conversation();
        {
            let store = SessionStore::open(&path, "agent-a", "wss://buzz.example", "v1")
                .expect("open store");
            store
                .put(&scope, "codex-acp", "thread-123")
                .await
                .expect("persist binding");
        }

        let store =
            SessionStore::open(&path, "agent-a", "wss://buzz.example", "v1").expect("reopen store");
        assert_eq!(
            store.lookup(&scope, "codex-acp").await.expect("lookup"),
            Some("thread-123".to_string())
        );
        let connection = Connection::open(&path).expect("inspection connection");
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .expect("journal mode");
        assert_eq!(mode.to_ascii_lowercase(), "wal");

        drop(connection);
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[tokio::test]
    async fn revision_and_adapter_fence_stale_bindings() {
        let path = test_path("revision");
        let scope = conversation();
        let first = SessionStore::open(&path, "agent-a", "wss://buzz.example", "v1")
            .expect("open first revision");
        first
            .put(&scope, "codex-acp", "thread-old")
            .await
            .expect("persist old binding");
        assert_eq!(
            first
                .lookup(&scope, "claude-agent-acp")
                .await
                .expect("lookup"),
            None
        );
        drop(first);

        let second = SessionStore::open(&path, "agent-a", "wss://buzz.example", "v2")
            .expect("open second revision");
        assert_eq!(
            second.lookup(&scope, "codex-acp").await.expect("lookup"),
            None
        );

        drop(second);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }
}
