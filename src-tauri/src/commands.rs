//! Thin Tauri commands. Each delegates to a `do_<name>` that takes the state
//! directly, so the logic is testable without a Tauri runtime.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use pylentra_core::error::{ApiError, CoreError};
use pylentra_core::model::{AuthMethod, Connection, ForwardRule, Group, Settings, Warning};
use pylentra_core::secrets::SecretKind;
use pylentra_core::ssh::events::{PromptAnswer, SessionEvent};
use pylentra_core::ssh::{SessionId, SessionSink};
use pylentra_core::ssh_config::{ScanResult, parse};
use pylentra_core::store::Library;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::sink::ChannelSink;
use crate::state::AppState;

#[tauri::command]
pub fn library_get(state: State<'_, AppState>) -> Result<Library, ApiError> {
    Ok(do_library_get(&state))
}

pub fn do_library_get(state: &AppState) -> Library {
    state.store().library().clone()
}

#[tauri::command]
pub fn startup_warnings(state: State<'_, AppState>) -> Result<Vec<ApiError>, ApiError> {
    Ok(do_startup_warnings(&state))
}

pub fn do_startup_warnings(state: &AppState) -> Vec<ApiError> {
    state.warnings.clone()
}

// Off the main thread: the keychain may block.
#[tauri::command(async)]
pub fn connection_save(
    state: State<'_, AppState>,
    connection: Connection,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<Vec<Warning>, ApiError> {
    do_connection_save(&state, connection, password, passphrase)
}

/// Validates first, so a rejected connection never leaves a secret behind,
/// and upserts last, so a secret that cannot be stored saves nothing.
pub fn do_connection_save(
    state: &AppState,
    connection: Connection,
    password: Option<String>,
    passphrase: Option<String>,
) -> Result<Vec<Warning>, ApiError> {
    // Held throughout, so the checked library is the one we write to.
    let mut store = state.store();
    store.check_connection(&connection)?;
    // Sessions read the keychain regardless of the flag, so unticking "save"
    // or switching auth type must forget the secret. Only on that change:
    // a prompt's "save" writes the keychain without setting the flag, and a
    // later rename must not lose it. Forgetting is best effort, like
    // delete_connection: a missing keychain must not block the save.
    let keeps = |auth: &AuthMethod| match auth {
        AuthMethod::Password { save } => (*save, false),
        AuthMethod::Key {
            save_passphrase, ..
        } => (false, *save_passphrase),
        AuthMethod::Agent | AuthMethod::Auto => (false, false),
    };
    let (keep_password, keep_passphrase) = keeps(&connection.auth);
    let old = store.get(connection.id).ok().map(|c| {
        (
            keeps(&c.auth),
            std::mem::discriminant(&c.auth) != std::mem::discriminant(&connection.auth),
        )
    });
    for (kind, keep, value) in [
        (SecretKind::Password, keep_password, password),
        (SecretKind::Passphrase, keep_passphrase, passphrase),
    ] {
        let forget = match old {
            Some(((old_pw, old_pp), auth_changed)) => {
                let was_kept = if kind == SecretKind::Password {
                    old_pw
                } else {
                    old_pp
                };
                was_kept || auth_changed
            }
            None => false,
        };
        if !keep {
            if !forget {
                continue;
            }
            if let Err(e) = state.secrets.delete(connection.id, kind) {
                log::warn!(
                    "could not delete {kind:?} of connection {}: {e}",
                    connection.id
                );
            }
        } else if let Some(value) = value {
            state.secrets.set(connection.id, kind, &value.into())?;
        }
    }
    Ok(store.upsert_connection(connection)?)
}

#[tauri::command]
pub fn connection_dependents(state: State<'_, AppState>, id: Uuid) -> Result<Vec<Uuid>, ApiError> {
    Ok(do_connection_dependents(&state, id))
}

pub fn do_connection_dependents(state: &AppState, id: Uuid) -> Vec<Uuid> {
    state.store().dependents(id)
}

// Off the main thread: the keychain may block.
#[tauri::command(async)]
pub fn connection_delete(state: State<'_, AppState>, id: Uuid) -> Result<(), ApiError> {
    do_connection_delete(&state, id)
}

pub fn do_connection_delete(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    Ok(state
        .store()
        .delete_connection(id, state.secrets.as_ref())?)
}

#[tauri::command]
pub fn group_save(state: State<'_, AppState>, group: Group) -> Result<(), ApiError> {
    do_group_save(&state, group)
}

pub fn do_group_save(state: &AppState, group: Group) -> Result<(), ApiError> {
    Ok(state.store().upsert_group(group)?)
}

#[tauri::command]
pub fn group_delete(state: State<'_, AppState>, id: Uuid) -> Result<(), ApiError> {
    do_group_delete(&state, id)
}

pub fn do_group_delete(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    Ok(state.store().delete_group(id)?)
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Result<Settings, ApiError> {
    Ok(do_settings_get(&state))
}

pub fn do_settings_get(state: &AppState) -> Settings {
    state.settings().get().clone()
}

#[tauri::command]
pub fn settings_save(state: State<'_, AppState>, settings: Settings) -> Result<(), ApiError> {
    do_settings_save(&state, settings)
}

pub fn do_settings_save(state: &AppState, settings: Settings) -> Result<(), ApiError> {
    Ok(state.settings().save(settings)?)
}

#[tauri::command]
pub fn ssh_config_scan(state: State<'_, AppState>) -> Result<ScanResult, ApiError> {
    match std::env::home_dir() {
        Some(home) => do_ssh_config_scan(&state, &home.join(".ssh").join("config")),
        None => Ok(missing_scan(String::new())),
    }
}

fn missing_scan(path: String) -> ScanResult {
    ScanResult {
        path,
        missing: true,
        ..parse("", &[])
    }
}

const MAX_SSH_CONFIG_BYTES: usize = 1024 * 1024;

/// Reads `path` (never writes it) and marks the candidates already saved.
pub fn do_ssh_config_scan(state: &AppState, path: &Path) -> Result<ScanResult, ApiError> {
    let shown = path.display().to_string();
    let mut bytes = Vec::new();
    // One byte past the limit is enough to reject, without reading the rest.
    let read = std::fs::File::open(path).and_then(|f| {
        f.take(MAX_SSH_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    match read {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(missing_scan(shown));
        }
        Err(e) => return Err(CoreError::from(e).into()),
    }
    if bytes.len() > MAX_SSH_CONFIG_BYTES {
        return Err(CoreError::InvalidInput {
            field: "ssh_config".into(),
            reason: "file too large".into(),
        }
        .into());
    }
    let mut result = parse(
        &String::from_utf8_lossy(&bytes),
        &state.store().library().connections,
    );
    result.path = shown;
    Ok(result)
}

#[tauri::command]
pub fn connections_import(
    state: State<'_, AppState>,
    connections: Vec<Connection>,
    group_id: Option<Uuid>,
) -> Result<usize, ApiError> {
    do_connections_import(&state, connections, group_id)
}

pub fn do_connections_import(
    state: &AppState,
    connections: Vec<Connection>,
    group_id: Option<Uuid>,
) -> Result<usize, ApiError> {
    Ok(state.store().import(connections, group_id)?)
}

// Async so the session task is spawned on Tauri's tokio runtime.
#[tauri::command]
pub async fn session_open(
    state: State<'_, AppState>,
    connection_id: Uuid,
    cols: u16,
    rows: u16,
    data: Channel<InvokeResponseBody>,
    events: Channel<SessionEvent>,
) -> Result<SessionId, ApiError> {
    do_session_open(
        &state,
        connection_id,
        cols,
        rows,
        Arc::new(ChannelSink { data, events }),
    )
}

/// Must run inside a tokio runtime.
pub fn do_session_open(
    state: &AppState,
    connection_id: Uuid,
    cols: u16,
    rows: u16,
    sink: Arc<dyn SessionSink>,
) -> Result<SessionId, ApiError> {
    let chain = state.store().jump_chain(connection_id)?;
    Ok(state.sessions.open(chain, cols, rows, sink))
}

// Async so the session task is spawned on Tauri's tokio runtime.
#[tauri::command]
pub async fn session_open_quick(
    state: State<'_, AppState>,
    connection: Connection,
    cols: u16,
    rows: u16,
    data: Channel<InvokeResponseBody>,
    events: Channel<SessionEvent>,
) -> Result<SessionId, ApiError> {
    do_session_open_quick(
        &state,
        connection,
        cols,
        rows,
        Arc::new(ChannelSink { data, events }),
    )
}

/// Opens a session for a connection the UI built, without saving it. It is a
/// single hop, so a jump host or forwards (which need the store) are refused.
/// Must run inside a tokio runtime.
pub fn do_session_open_quick(
    state: &AppState,
    connection: Connection,
    cols: u16,
    rows: u16,
    sink: Arc<dyn SessionSink>,
) -> Result<SessionId, ApiError> {
    let invalid = |field: &str, reason: &str| CoreError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    };
    if connection.jump_via.is_some() {
        return Err(invalid("jump_via", "not allowed in a quick connection").into());
    }
    if !connection.forwards.is_empty() {
        return Err(invalid("forwards", "not allowed in a quick connection").into());
    }
    connection.validate()?;
    Ok(state.sessions.open(vec![connection], cols, rows, sink))
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    id: SessionId,
    data: Vec<u8>,
) -> Result<(), ApiError> {
    do_session_write(&state, id, data).await
}

pub async fn do_session_write(
    state: &AppState,
    id: SessionId,
    data: Vec<u8>,
) -> Result<(), ApiError> {
    Ok(state.sessions.write(id, data).await?)
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    id: SessionId,
    cols: u16,
    rows: u16,
) -> Result<(), ApiError> {
    do_session_resize(&state, id, cols, rows).await
}

pub async fn do_session_resize(
    state: &AppState,
    id: SessionId,
    cols: u16,
    rows: u16,
) -> Result<(), ApiError> {
    Ok(state.sessions.resize(id, cols, rows).await?)
}

#[tauri::command]
pub fn session_answer(
    state: State<'_, AppState>,
    id: SessionId,
    answer: PromptAnswer,
) -> Result<(), ApiError> {
    do_session_answer(&state, id, answer)
}

pub fn do_session_answer(
    state: &AppState,
    id: SessionId,
    answer: PromptAnswer,
) -> Result<(), ApiError> {
    Ok(state.sessions.answer(id, answer)?)
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, id: SessionId) -> Result<(), ApiError> {
    do_session_close(&state, id).await
}

pub async fn do_session_close(state: &AppState, id: SessionId) -> Result<(), ApiError> {
    Ok(state.sessions.close(id).await?)
}

#[tauri::command]
pub async fn forward_start(
    state: State<'_, AppState>,
    session_id: SessionId,
    rule: ForwardRule,
) -> Result<u32, ApiError> {
    do_forward_start(&state, session_id, rule).await
}

/// The core trusts its caller, so the rule is validated here, at the IPC boundary.
pub async fn do_forward_start(
    state: &AppState,
    session_id: SessionId,
    rule: ForwardRule,
) -> Result<u32, ApiError> {
    rule.validate("rule")?;
    Ok(state.sessions.forward_start(session_id, rule).await?)
}

#[tauri::command]
pub async fn forward_stop(
    state: State<'_, AppState>,
    session_id: SessionId,
    forward_id: u32,
) -> Result<(), ApiError> {
    do_forward_stop(&state, session_id, forward_id).await
}

pub async fn do_forward_stop(
    state: &AppState,
    session_id: SessionId,
    forward_id: u32,
) -> Result<(), ApiError> {
    Ok(state.sessions.forward_stop(session_id, forward_id).await?)
}

#[tauri::command]
pub fn known_host_forget(
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<u32, ApiError> {
    do_known_host_forget(&state, &host, port)
}

pub fn do_known_host_forget(state: &AppState, host: &str, port: u16) -> Result<u32, ApiError> {
    let removed = state.known_hosts.forget(host, port)?;
    Ok(u32::try_from(removed).unwrap_or(u32::MAX))
}

#[tauri::command]
pub fn log_dir(app: AppHandle) -> Result<String, ApiError> {
    let dir = app
        .path()
        .app_log_dir()
        .map_err(|e| CoreError::Io(std::io::Error::other(e)))?;
    Ok(dir.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pylentra_core::secrets::{FailingSecretStore, MemorySecretStore, SecretStore};
    use secrecy::ExposeSecret;
    use tempfile::TempDir;

    fn state_with(secrets: Arc<dyn SecretStore>) -> (TempDir, AppState) {
        let dir = TempDir::new().unwrap();
        let state = AppState::load(dir.path(), None, secrets).unwrap();
        (dir, state)
    }

    fn conn(name: &str) -> Connection {
        Connection {
            id: Uuid::new_v4(),
            name: name.into(),
            group_id: None,
            host: "example.com".into(),
            port: 22,
            username: "luca".into(),
            auth: AuthMethod::Password { save: true },
            jump_via: None,
            forwards: vec![],
            color: None,
            icon: None,
        }
    }

    fn secret(state: &AppState, id: Uuid, kind: SecretKind) -> Option<String> {
        state
            .secrets
            .get(id, kind)
            .unwrap()
            .map(|s| s.expose_secret().to_owned())
    }

    #[test]
    fn connection_save_stores_secrets_before_upsert() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let c = conn("a");
        let warnings = do_connection_save(&state, c.clone(), Some("pw".into()), None).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            secret(&state, c.id, SecretKind::Password).as_deref(),
            Some("pw")
        );
        assert_eq!(do_library_get(&state).connections, vec![c]);
    }

    #[test]
    fn connection_save_invalid_does_not_store_secret() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let empty_name = conn(" ");
        let unknown_jump = Connection {
            jump_via: Some(Uuid::new_v4()),
            ..conn("b")
        };
        for c in [empty_name, unknown_jump] {
            let err = do_connection_save(&state, c.clone(), Some("pw".into()), None).unwrap_err();
            assert_eq!(err.code, "config.invalid");
            assert_eq!(secret(&state, c.id, SecretKind::Password), None);
        }
        assert!(do_library_get(&state).connections.is_empty());
    }

    #[test]
    fn connection_save_forgets_secrets_whose_save_is_unticked() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let c = conn("a");
        do_connection_save(&state, c.clone(), Some("pw".into()), Some("pp".into())).unwrap();
        // Password kept while ticked; the passphrase has no use under Password auth.
        assert_eq!(
            secret(&state, c.id, SecretKind::Password).as_deref(),
            Some("pw")
        );
        assert_eq!(secret(&state, c.id, SecretKind::Passphrase), None);

        let key = Connection {
            auth: AuthMethod::Key {
                path: "/k".into(),
                save_passphrase: true,
            },
            ..c.clone()
        };
        do_connection_save(&state, key.clone(), None, Some("pp".into())).unwrap();
        assert_eq!(secret(&state, c.id, SecretKind::Password), None);
        assert_eq!(
            secret(&state, c.id, SecretKind::Passphrase).as_deref(),
            Some("pp")
        );

        let unticked = Connection {
            auth: AuthMethod::Key {
                path: "/k".into(),
                save_passphrase: false,
            },
            ..c.clone()
        };
        do_connection_save(&state, unticked, None, None).unwrap();
        assert_eq!(secret(&state, c.id, SecretKind::Passphrase), None);
    }

    #[test]
    fn connection_save_keeps_secret_saved_from_a_prompt() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let c = Connection {
            auth: AuthMethod::Password { save: false },
            ..conn("a")
        };
        do_connection_save(&state, c.clone(), None, None).unwrap();
        // A ticked "save" in the connect prompt writes the keychain, not the flag.
        state
            .secrets
            .set(c.id, SecretKind::Password, &"pw".to_string().into())
            .unwrap();
        let renamed = Connection {
            name: "b".into(),
            ..c.clone()
        };
        do_connection_save(&state, renamed, None, None).unwrap();
        assert_eq!(
            secret(&state, c.id, SecretKind::Password).as_deref(),
            Some("pw")
        );
    }

    #[test]
    fn connection_save_survives_failing_secret_delete() {
        let (_d, state) = state_with(Arc::new(FailingSecretStore));
        let c = conn("a");
        do_connection_save(&state, c.clone(), None, None).unwrap();
        // Switching auth type forgets the old secret; the failing delete must not block.
        let agent = Connection {
            auth: AuthMethod::Agent,
            ..c
        };
        do_connection_save(&state, agent.clone(), None, None).unwrap();
        assert_eq!(do_library_get(&state).connections, vec![agent]);
    }

    #[test]
    fn connection_save_secret_failure_does_not_upsert() {
        let (_d, state) = state_with(Arc::new(FailingSecretStore));
        let err = do_connection_save(&state, conn("a"), Some("pw".into()), None).unwrap_err();
        assert_eq!(err.code, "secret.unavailable");
        assert!(do_library_get(&state).connections.is_empty());
    }

    #[test]
    fn startup_warnings_reports_corrupt_backup() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("connections.toml"), "not = [valid").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let warnings = do_startup_warnings(&state);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "config.recovered_corrupt");
        let backup = std::path::Path::new(&warnings[0].params["backup"]);
        assert!(backup.exists(), "backup {backup:?} should exist");
    }

    #[test]
    fn startup_warnings_reports_newer_version_read_only() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("connections.toml"), "version = 7\n").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let warnings = do_startup_warnings(&state);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "config.read_only");
        assert_eq!(warnings[0].params["found"], "7");
    }

    #[test]
    fn forward_start_rejects_invalid_rule_before_the_core() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let rule = ForwardRule::Local {
            bind_host: "localhost".into(),
            bind_port: 8080,
            dest_host: "db".into(),
            dest_port: 5432,
            auto_start: false,
        };
        let err = tauri::async_runtime::block_on(do_forward_start(&state, Uuid::new_v4(), rule))
            .unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "rule.bind_host");
    }

    struct NullSink;
    impl SessionSink for NullSink {
        fn data(&self, _: Vec<u8>) {}
        fn event(&self, _: SessionEvent) {}
    }

    fn quick_open(state: &AppState, c: Connection) -> Result<SessionId, ApiError> {
        // Nothing listens on port 1, so the background connect fails fast.
        tauri::async_runtime::block_on(async {
            do_session_open_quick(state, c, 80, 24, Arc::new(NullSink))
        })
    }

    fn quick_conn() -> Connection {
        Connection {
            host: "127.0.0.1".into(),
            port: 1,
            auth: AuthMethod::Auto,
            ..conn("quick")
        }
    }

    #[test]
    fn quick_session_is_not_saved() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        quick_open(&state, quick_conn()).unwrap();
        assert!(do_library_get(&state).connections.is_empty());
    }

    #[test]
    fn quick_session_rejects_jump_and_forwards() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let jump = Connection {
            jump_via: Some(Uuid::new_v4()),
            ..quick_conn()
        };
        let forwards = Connection {
            forwards: vec![ForwardRule::Local {
                bind_host: "127.0.0.1".into(),
                bind_port: 8080,
                dest_host: "db".into(),
                dest_port: 5432,
                auto_start: false,
            }],
            ..quick_conn()
        };
        for (c, field) in [(jump, "jump_via"), (forwards, "forwards")] {
            let err = quick_open(&state, c).unwrap_err();
            assert_eq!(err.code, "config.invalid");
            assert_eq!(err.params["field"], field);
        }
    }

    #[test]
    fn quick_session_validates_fields() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let err = quick_open(
            &state,
            Connection {
                host: " ".into(),
                ..quick_conn()
            },
        )
        .unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "host");
    }
    fn scan(state: &AppState, bytes: &[u8]) -> Result<ScanResult, ApiError> {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, bytes).unwrap();
        do_ssh_config_scan(state, &path)
    }

    #[test]
    fn scan_missing_file() {
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let path = d.path().join("nope");
        let r = do_ssh_config_scan(&state, &path).unwrap();
        assert!(r.missing && r.candidates.is_empty());
        assert_eq!(r.path, path.display().to_string());
    }

    #[test]
    fn scan_rejects_files_over_1_mib() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let err = scan(&state, &vec![b'#'; 1024 * 1024 + 1]).unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert!(scan(&state, &vec![b'#'; 1024 * 1024]).is_ok());
    }

    #[test]
    fn scan_reads_non_utf8_bytes() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let r = scan(&state, b"Host a\n HostName h\xff\n").unwrap();
        assert_eq!(r.candidates.len(), 1);
        assert!(!r.missing && !r.path.is_empty());
    }

    #[test]
    fn scan_marks_existing_names() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        do_connection_save(&state, conn("A"), None, None).unwrap();
        let r = scan(&state, b"Host a b\n HostName h\n").unwrap();
        let exists: Vec<bool> = r.candidates.iter().map(|c| c.exists).collect();
        assert_eq!(exists, [true, false]);
    }

    #[test]
    fn connections_import_adds_all_and_reports_count() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        assert_eq!(
            do_connections_import(&state, vec![conn("a"), conn("b")], None).unwrap(),
            2
        );
        assert_eq!(do_library_get(&state).connections.len(), 2);
    }
}
