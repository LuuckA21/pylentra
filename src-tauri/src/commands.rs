//! Thin Tauri commands. Each delegates to a `do_<name>` that takes the state
//! directly, so the logic is testable without a Tauri runtime.

use std::sync::Arc;

use pylentra_core::error::{ApiError, CoreError};
use pylentra_core::model::{AuthMethod, Connection, ForwardRule, Group, Settings, Warning};
use pylentra_core::secrets::SecretKind;
use pylentra_core::ssh::events::{PromptAnswer, SessionEvent};
use pylentra_core::ssh::{SessionId, SessionSink};
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
    // Sessions read the keychain regardless of the flag, so an unticked
    // "save" must forget the secret. Forgetting is best effort, like
    // delete_connection: a missing keychain must not block the save.
    let (keep_password, keep_passphrase) = match connection.auth {
        AuthMethod::Password { save } => (save, false),
        AuthMethod::Key {
            save_passphrase, ..
        } => (false, save_passphrase),
        AuthMethod::Agent => (false, false),
    };
    for (kind, keep, value) in [
        (SecretKind::Password, keep_password, password),
        (SecretKind::Passphrase, keep_passphrase, passphrase),
    ] {
        if !keep {
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
    fn connection_save_survives_failing_secret_delete() {
        let (_d, state) = state_with(Arc::new(FailingSecretStore));
        let c = Connection {
            auth: AuthMethod::Agent,
            ..conn("a")
        };
        do_connection_save(&state, c.clone(), None, None).unwrap();
        assert_eq!(do_library_get(&state).connections, vec![c]);
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
}
