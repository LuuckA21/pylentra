//! Thin Tauri commands. Each delegates to a `do_<name>` that takes the state
//! directly, so the logic is testable without a Tauri runtime.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pylentra_core::error::{ApiError, CoreError};
use pylentra_core::export::{
    self, ExportDone, ExportFile, ImportCounts, ImportPick, ImportPreview,
};
use pylentra_core::known_hosts::KnownHostEntry;
use pylentra_core::model::{AuthMethod, Connection, ForwardRule, Group, Settings, Warning};
use pylentra_core::secrets::SecretKind;
use pylentra_core::snippets::Snippet;
use pylentra_core::ssh::events::{PromptAnswer, SessionEvent};
use pylentra_core::ssh::{SessionId, SessionSink};
use pylentra_core::ssh_config::{ScanResult, parse};
use pylentra_core::store::{Library, atomic_write};
use tauri::http::HeaderMap;
use tauri::ipc::{Channel, InvokeBody, InvokeResponseBody, Request};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FilePath};
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
    let mut store = state.store();
    store.delete_connection(id, state.secrets.as_ref())?;
    // The connection is already gone: a snippet file we cannot write (newer
    // version, disk error) must not fail the delete.
    if let Err(e) = state.snippets().remove_for_connection(id) {
        log::warn!("could not remove the snippets of connection {id}: {e}");
    }
    Ok(())
}

#[tauri::command]
pub fn snippets_get(state: State<'_, AppState>) -> Result<Vec<Snippet>, ApiError> {
    Ok(do_snippets_get(&state))
}

pub fn do_snippets_get(state: &AppState) -> Vec<Snippet> {
    state.snippets().list().to_vec()
}

#[tauri::command]
pub fn snippet_save(state: State<'_, AppState>, snippet: Snippet) -> Result<(), ApiError> {
    do_snippet_save(&state, snippet)
}

pub fn do_snippet_save(state: &AppState, snippet: Snippet) -> Result<(), ApiError> {
    // The store stays locked, so the connection cannot vanish before the write.
    let store = state.store();
    if let Some(id) = snippet.connection_id
        && store.get(id).is_err()
    {
        return Err(CoreError::InvalidInput {
            field: "connection_id".into(),
            reason: "unknown connection".into(),
        }
        .into());
    }
    Ok(state.snippets().upsert(snippet)?)
}

#[tauri::command]
pub fn snippet_delete(state: State<'_, AppState>, id: Uuid) -> Result<(), ApiError> {
    do_snippet_delete(&state, id)
}

pub fn do_snippet_delete(state: &AppState, id: Uuid) -> Result<(), ApiError> {
    Ok(state.snippets().delete(id)?)
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
    let (connect_timeout, keepalive) = settings.session_timeouts();
    state.settings().save(settings)?;
    state.sessions.set_timeouts(connect_timeout, keepalive);
    Ok(())
}

// Async so the file read runs off the main thread.
#[tauri::command(async)]
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

/// Reads a regular file of at most `max` bytes; `Io(NotFound)` passes through.
fn read_limited(path: &Path, max: usize, field: &str) -> Result<Vec<u8>, CoreError> {
    let invalid = |reason: &str| CoreError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    };
    // Checked before opening: a directory errors and a FIFO would block the read.
    if !std::fs::metadata(path)?.is_file() {
        return Err(invalid("not a regular file"));
    }
    let mut bytes = Vec::new();
    // One byte past the limit is enough to reject, without reading the rest.
    std::fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(invalid("file too large"));
    }
    Ok(bytes)
}

/// Reads `path` (never writes it) and marks the candidates already saved.
pub fn do_ssh_config_scan(state: &AppState, path: &Path) -> Result<ScanResult, ApiError> {
    let shown = path.display().to_string();
    let bytes = match read_limited(path, MAX_SSH_CONFIG_BYTES, "ssh_config") {
        Ok(bytes) => bytes,
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(missing_scan(shown));
        }
        Err(e) => return Err(e.into()),
    };
    let mut result = parse(
        &String::from_utf8_lossy(&bytes),
        &state.store().library().connections,
    );
    result.path = shown;
    Ok(result)
}

const MAX_EXPORT_BYTES: usize = 5 * 1024 * 1024;
const MAX_EXPORTED_AT_CHARS: usize = 64;

/// The date part of `exported_at`, or none if it does not look like one.
fn export_file_name(exported_at: &str) -> String {
    match exported_at.get(..10) {
        Some(d) if d.bytes().all(|b| b.is_ascii_digit() || b == b'-') => {
            format!("pylentra-export-{d}.toml")
        }
        _ => "pylentra-export.toml".into(),
    }
}

/// GTK saves a name typed without `.toml` as is, which the Open filter would hide.
fn with_toml(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.with_extension("toml")
    } else {
        path
    }
}

/// A network path (`\\host\share`, `//host/share`) is not checked: on Windows
/// that alone contacts the host (SMB, NTLM) as soon as the preview opens.
fn key_found_or_unchecked(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.starts_with(r"\\") || s.starts_with("//") || path.exists()
}

fn picked_path(picked: FilePath) -> Result<PathBuf, CoreError> {
    picked
        .into_path()
        .map_err(|e| CoreError::Io(std::io::Error::other(e.to_string())))
}

// Async: the dialog blocks, and must not run on the main thread.
// The dialogs open here so no path ever comes from the webview.
#[tauri::command(async)]
pub fn library_export(
    window: WebviewWindow,
    state: State<'_, AppState>,
    exported_at: String,
) -> Result<Option<ExportDone>, ApiError> {
    let Some(picked) = window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("TOML", &["toml"])
        .set_file_name(export_file_name(&exported_at))
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = with_toml(picked_path(picked)?);
    let snippets = do_library_export(&state, &path, &exported_at)?;
    Ok(Some(ExportDone {
        path: path.display().to_string(),
        snippets,
    }))
}

/// `false`: `snippets.toml` could not be read, so the file has no snippets.
pub fn do_library_export(
    state: &AppState,
    path: &Path,
    exported_at: &str,
) -> Result<bool, ApiError> {
    if exported_at.chars().count() > MAX_EXPORTED_AT_CHARS {
        return Err(CoreError::InvalidInput {
            field: "exported_at".into(),
            reason: "too long".into(),
        }
        .into());
    }
    let (text, with_snippets) = {
        let store = state.store();
        let snippets = state.snippets();
        let text = export::export(store.library(), snippets.list(), exported_at)?;
        (text, !snippets.is_unavailable())
    };
    atomic_write(path, &text)?;
    Ok(with_snippets)
}

fn read_export(path: &Path) -> Result<ExportFile, CoreError> {
    let bytes = read_limited(path, MAX_EXPORT_BYTES, "file")?;
    let text = String::from_utf8(bytes).map_err(|e| CoreError::InvalidInput {
        field: "file".into(),
        reason: e.to_string(),
    })?;
    export::parse(&text)
}

/// The file without the global snippets already saved, for preview and import alike.
fn read_import(state: &AppState, path: &Path) -> Result<ExportFile, CoreError> {
    let mut file = read_export(path)?;
    export::drop_saved_globals(&mut file.snippets, state.snippets().list());
    Ok(file)
}

// Async: the dialog blocks, and must not run on the main thread.
#[tauri::command(async)]
pub fn library_import_preview(
    window: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<ImportPick>, ApiError> {
    let Some(picked) = window
        .dialog()
        .file()
        .set_parent(&window)
        .add_filter("TOML", &["toml"])
        // shortcut: GTK turns "*" into "*.*", hiding names without a dot; new exports always get `.toml`.
        .add_filter("All files", &["*"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    do_library_import_pick(&state, picked_path(picked)?).map(Some)
}

/// Previews `path` and remembers it for `library_import`.
pub fn do_library_import_pick(state: &AppState, path: PathBuf) -> Result<ImportPick, ApiError> {
    let preview = do_library_import_preview(state, &path);
    // A failed pick forgets the previous file: a confirm only reads the one just shown.
    *state.pending_import() = preview.is_ok().then(|| path.clone());
    Ok(ImportPick {
        path: path.display().to_string(),
        preview: preview?,
    })
}

pub fn do_library_import_preview(state: &AppState, path: &Path) -> Result<ImportPreview, ApiError> {
    let file = read_import(state, path)?;
    // A copy: the key checks may be slow and must not hold the store.
    let library = state.store().library().clone();
    Ok(export::preview(&file, &library, key_found_or_unchecked))
}

// Async so the file read runs off the main thread.
#[tauri::command(async)]
pub fn library_import(
    state: State<'_, AppState>,
    selected: Vec<Uuid>,
) -> Result<ImportCounts, ApiError> {
    do_library_import_pending(&state, &selected)
}

/// Imports the file remembered by the last pick.
pub fn do_library_import_pending(
    state: &AppState,
    selected: &[Uuid],
) -> Result<ImportCounts, ApiError> {
    // Held throughout, so a second confirm waits and then finds nothing.
    let mut pending = state.pending_import();
    let path = pending
        .as_deref()
        .ok_or(CoreError::Io(std::io::ErrorKind::NotFound.into()))?;
    let result = do_library_import(state, path, selected);
    // Once the library is written, a retry would add the connections again.
    if result
        .as_ref()
        .map_or_else(|e| e.params.contains_key("connections_imported"), |_| true)
    {
        *pending = None;
    }
    result
}

/// Rereads the file: `selected` holds its ids, the same the preview showed.
/// The library is written first; if the snippets then fail, the connections
/// stay imported and the snippet error is returned.
pub fn do_library_import(
    state: &AppState,
    path: &Path,
    selected: &[Uuid],
) -> Result<ImportCounts, ApiError> {
    let file = read_import(state, path)?;
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    // Held throughout, so the snippets' connections cannot vanish first.
    let mut store = state.store();
    let plan = export::plan(&file, selected, store.library())?;
    let groups = count(plan.groups.len());
    let connections = count(store.import_bundle(plan.groups, plan.connections)?);
    // Nothing to add must not fail on an unavailable snippets file.
    let snippets = if plan.snippets.is_empty() {
        0
    } else {
        let imported = state.snippets().import(plan.snippets).map_err(|e| {
            let mut e = ApiError::from(e);
            // Tells "snippets failed" apart from "nothing imported".
            e.params
                .insert("connections_imported".into(), connections.to_string());
            e
        })?;
        count(imported)
    };
    Ok(ImportCounts {
        connections,
        groups,
        snippets,
    })
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
    if !matches!(connection.auth, AuthMethod::Auto) {
        return Err(invalid("auth", "must be automatic in a quick connection").into());
    }
    if state.store().get(connection.id).is_ok() {
        return Err(invalid("id", "already in the library").into());
    }
    connection.validate()?;
    Ok(state.sessions.open(vec![connection], cols, rows, sink))
}

/// The bytes are the raw request body and the session id is the `id` header,
/// so a big paste is not sent as a JSON array of numbers.
#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<(), ApiError> {
    let (id, data) = write_args(request.headers(), request.body())?;
    do_session_write(&state, id, data).await
}

fn write_args(headers: &HeaderMap, body: &InvokeBody) -> Result<(SessionId, Vec<u8>), CoreError> {
    let invalid = |field: &str| CoreError::InvalidInput {
        field: field.into(),
        reason: "malformed".into(),
    };
    let id = headers
        .get("id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("id"))?;
    let data = match body {
        InvokeBody::Raw(bytes) => bytes.clone(),
        // The postMessage fallback (custom protocol blocked) sends the bytes as JSON.
        InvokeBody::Json(v) => serde_json::from_value(v.clone()).map_err(|_| invalid("data"))?,
    };
    Ok((id, data))
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
pub fn known_hosts_list(state: State<'_, AppState>) -> Result<Vec<KnownHostEntry>, ApiError> {
    Ok(state.known_hosts.list()?)
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
    use pylentra_core::snippets::SnippetStore;
    use pylentra_core::store::ConnectionStore;
    use secrecy::ExposeSecret;
    use tempfile::TempDir;

    fn state_with(secrets: Arc<dyn SecretStore>) -> (TempDir, AppState) {
        let dir = TempDir::new().unwrap();
        let state = AppState::load(dir.path(), None, secrets).unwrap();
        (dir, state)
    }

    #[test]
    fn settings_save_updates_session_timeouts() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        assert_eq!(state.sessions.options().connect_timeout.as_secs(), 15);
        let new = Settings {
            connect_timeout_secs: 40,
            keepalive_secs: 0,
            ..Settings::default()
        };
        do_settings_save(&state, new).unwrap();
        let o = state.sessions.options();
        assert_eq!(
            (o.connect_timeout.as_secs(), o.keepalive_interval),
            (40, None)
        );
        let bad = Settings {
            connect_timeout_secs: 1,
            ..Settings::default()
        };
        assert!(do_settings_save(&state, bad).is_err());
        assert_eq!(state.sessions.options().connect_timeout.as_secs(), 40);
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
            notes: None,
            extra: Default::default(),
        }
    }

    #[test]
    fn ui_json_with_nulls_still_deserializes() {
        // `extra` is flattened, so serde buffers the whole object first.
        let c: Connection = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "name": "n", "group_id": null, "host": "h",
            "port": 22, "username": "u", "auth": { "type": "Agent" },
            "jump_via": null, "forwards": [], "color": null,
        }))
        .unwrap();
        assert_eq!((c.group_id, c.color), (None, None));
        assert!(c.extra.is_empty());
        let g: Group =
            serde_json::from_value(serde_json::json!({ "id": Uuid::new_v4(), "name": "g" }))
                .unwrap();
        assert!(g.extra.is_empty());
    }

    #[test]
    fn settings_save_from_ui_keeps_extra() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        std::fs::write(
            &path,
            "futuro = \"x\"\nfuturo_nan = nan\nfuturo_arr = [nan]\n[futuro_tab]\nx = inf\n",
        )
        .unwrap();
        let state = AppState::load(d.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        // What the UI does: get, JSON, back, with `futuro_nan` now `null`.
        let json = serde_json::to_string(&do_settings_get(&state)).unwrap();
        assert!(json.contains("\"futuro\":\"x\""), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        do_settings_save(&state, back).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("futuro = \"x\""), "{text}");
        assert!(text.contains("futuro_nan = nan"), "{text}");
        assert!(text.contains("futuro_arr = [nan]"), "{text}");
        assert!(text.contains("x = inf"), "{text}");
    }

    #[test]
    fn nested_null_unknown_keys_are_dropped_from_ui_json() {
        let c: Connection = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "name": "n", "host": "h", "port": 22,
            "username": "u", "auth": { "type": "Agent" }, "forwards": [],
            "x": [null], "y": { "z": null }, "ok": 1,
        }))
        .unwrap();
        assert_eq!(c.extra.keys().collect::<Vec<_>>(), ["ok"]);
        let s: Settings =
            serde_json::from_str(r#"{"x":[null],"y":{"z":null},"font_size":15}"#).unwrap();
        assert!(s.extra.is_empty());
    }

    #[test]
    fn null_unknown_keys_are_dropped_from_ui_json() {
        let c: Connection = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(), "name": "n", "host": "h", "port": 22,
            "username": "u", "auth": { "type": "Agent" }, "forwards": [],
            "futuro": null, "futuro2": 3,
        }))
        .unwrap();
        assert_eq!(c.extra.len(), 1);
        assert!(!c.extra.contains_key("futuro"));
        let s: Settings = serde_json::from_str(r#"{"futuro":null,"font_size":15}"#).unwrap();
        assert!(s.extra.is_empty());
        assert_eq!(s.font_size, 15);
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

    fn snippet(name: &str, connection_id: Option<Uuid>) -> Snippet {
        Snippet {
            id: Uuid::new_v4(),
            name: name.into(),
            text: "ls".into(),
            run: false,
            connection_id,
            extra: Default::default(),
        }
    }

    #[test]
    fn snippet_save_and_get() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let c = conn("a");
        do_connection_save(&state, c.clone(), None, None).unwrap();
        let (global, scoped) = (snippet("g", None), snippet("s", Some(c.id)));
        do_snippet_save(&state, global.clone()).unwrap();
        do_snippet_save(&state, scoped.clone()).unwrap();
        assert_eq!(do_snippets_get(&state), vec![global, scoped]);
    }

    #[test]
    fn snippet_save_rejects_unknown_connection() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let err = do_snippet_save(&state, snippet("s", Some(Uuid::new_v4()))).unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "connection_id");
        assert!(do_snippets_get(&state).is_empty());
    }

    #[test]
    fn connection_delete_removes_its_snippets() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let a = conn("a");
        do_connection_save(&state, a.clone(), None, None).unwrap();
        let global = snippet("g", None);
        do_snippet_save(&state, snippet("s", Some(a.id))).unwrap();
        do_snippet_save(&state, global.clone()).unwrap();
        do_connection_delete(&state, a.id).unwrap();
        assert_eq!(do_snippets_get(&state), vec![global]);
    }

    #[test]
    fn connection_delete_survives_read_only_snippets() {
        let dir = TempDir::new().unwrap();
        let a = conn("a");
        let secrets = Arc::new(MemorySecretStore::default());
        let state = AppState::load(dir.path(), None, secrets.clone()).unwrap();
        do_connection_save(&state, a.clone(), None, None).unwrap();
        drop(state);
        std::fs::write(
            dir.path().join("snippets.toml"),
            format!(
                "version = 9\n[[snippets]]\nid = \"{}\"\nname = \"s\"\ntext = \"ls\"\nconnection_id = \"{}\"\n",
                Uuid::new_v4(),
                a.id
            ),
        )
        .unwrap();
        let state = AppState::load(dir.path(), None, secrets).unwrap();
        do_connection_delete(&state, a.id).unwrap();
        assert!(do_library_get(&state).connections.is_empty());
        assert_eq!(do_snippets_get(&state).len(), 1);
    }

    #[test]
    fn startup_warnings_reports_corrupt_snippets() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("snippets.toml"), "not = [valid").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let warnings = do_startup_warnings(&state);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "config.recovered_corrupt");
        assert_eq!(warnings[0].params["file"], "snippets.toml");
        let backup = std::path::Path::new(&warnings[0].params["backup"]);
        assert!(backup.exists(), "backup {backup:?} should exist");
    }

    #[test]
    fn startup_warnings_reports_newer_snippets_read_only() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("snippets.toml"), "version = 9\n").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let warnings = do_startup_warnings(&state);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "config.read_only");
        assert_eq!(warnings[0].params["file"], "snippets.toml");
        assert_eq!(warnings[0].params["found"], "9");
    }

    #[test]
    fn unreadable_snippets_file_starts_read_only() {
        let dir = TempDir::new().unwrap();
        // A directory where the file should be: reading it fails, not with NotFound.
        std::fs::create_dir(dir.path().join("snippets.toml")).unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let warnings = do_startup_warnings(&state);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "config.unreadable");
        assert_eq!(warnings[0].params["file"], "snippets.toml");
        let reason = &warnings[0].params["reason"];
        assert!(
            !reason.is_empty() && !reason.starts_with("I/O error"),
            "{reason}"
        );
        assert!(do_snippets_get(&state).is_empty());
        let err = do_snippet_save(&state, snippet("a", None)).unwrap_err();
        assert_eq!(err.code, "config.read_only");
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

    #[test]
    fn quick_session_rejects_auth_other_than_auto() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        for auth in [AuthMethod::Agent, AuthMethod::Password { save: false }] {
            let err = quick_open(
                &state,
                Connection {
                    auth,
                    ..quick_conn()
                },
            )
            .unwrap_err();
            assert_eq!(err.code, "config.invalid");
            assert_eq!(err.params["field"], "auth");
        }
    }

    #[test]
    fn quick_session_rejects_an_id_in_the_library() {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let saved = quick_conn();
        do_connection_save(&state, saved.clone(), None, None).unwrap();
        let err = quick_open(&state, saved).unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "id");
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
    fn scan_rejects_a_directory() {
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let err = do_ssh_config_scan(&state, d.path()).unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "ssh_config");
        assert_eq!(err.params["reason"], "not a regular file");
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

    fn group(name: &str) -> Group {
        Group {
            id: Uuid::new_v4(),
            name: name.into(),
            color: None,
            extra: Default::default(),
        }
    }

    fn file_text(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    /// Exports a library of two connections (one in a group) and one snippet.
    fn exported(dir: &TempDir) -> std::path::PathBuf {
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let g = group("Prod");
        do_group_save(&state, g.clone()).unwrap();
        let a = Connection {
            group_id: Some(g.id),
            ..conn("a")
        };
        do_connection_save(&state, a.clone(), None, None).unwrap();
        do_connection_save(&state, conn("b"), None, None).unwrap();
        do_snippet_save(&state, snippet("s", Some(a.id))).unwrap();
        let path = dir.path().join("export.toml");
        assert!(do_library_export(&state, &path, "2026-10-30").unwrap());
        path
    }

    fn all_rows(state: &AppState, path: &Path) -> Vec<Uuid> {
        let p = do_library_import_preview(state, path).unwrap();
        p.rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn library_export_then_import() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let selected = all_rows(&state, &path);
        assert_eq!(selected.len(), 2);
        let counts = do_library_import(&state, &path, &selected).unwrap();
        assert_eq!(
            counts,
            ImportCounts {
                connections: 2,
                groups: 1,
                snippets: 1
            }
        );
        let lib = do_library_get(&state);
        let names: Vec<&str> = lib.connections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(lib.groups.len(), 1);
        assert_eq!(lib.connections[0].group_id, Some(lib.groups[0].id));
        assert_eq!(
            do_snippets_get(&state)[0].connection_id,
            Some(lib.connections[0].id)
        );
    }

    #[test]
    fn library_export_without_available_snippets() {
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        do_connection_save(&state, conn("a"), None, None).unwrap();
        *state.snippets() = SnippetStore::unavailable(d.path().join("snippets.toml"));
        let path = d.path().join("export.toml");
        assert!(!do_library_export(&state, &path, "2026-10-30").unwrap());
        let file = pylentra_core::export::parse(&file_text(&path).unwrap()).unwrap();
        assert_eq!((file.connections.len(), file.snippets.len()), (1, 0));
    }

    #[test]
    fn library_export_with_read_only_snippets_includes_them() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("snippets.toml"),
            format!(
                "version = 9\n[[snippets]]\nid = \"{}\"\nname = \"s\"\ntext = \"ls\"\n",
                Uuid::new_v4()
            ),
        )
        .unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let path = dir.path().join("export.toml");
        assert!(do_library_export(&state, &path, "2026-10-30").unwrap());
        let file = pylentra_core::export::parse(&file_text(&path).unwrap()).unwrap();
        assert_eq!(file.snippets.len(), 1);
    }

    #[test]
    fn library_import_into_read_only_library_changes_nothing() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let dir = TempDir::new().unwrap();
        let (conns, snips) = (
            dir.path().join("connections.toml"),
            dir.path().join("snippets.toml"),
        );
        std::fs::write(&conns, "version = 9\n").unwrap();
        std::fs::write(&snips, "version = 1\n").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let selected = all_rows(&state, &path);
        let err = do_library_import(&state, &path, &selected).unwrap_err();
        assert_eq!(err.code, CoreError::ReadOnly.code());
        assert!(!err.params.contains_key("connections_imported"));
        assert_eq!(file_text(&conns).as_deref(), Some("version = 9\n"));
        assert_eq!(file_text(&snips).as_deref(), Some("version = 1\n"));
    }

    #[test]
    fn library_import_keeps_connections_when_snippets_fail() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("snippets.toml"), "version = 9\n").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let selected = all_rows(&state, &path);
        let err = do_library_import(&state, &path, &selected).unwrap_err();
        assert_eq!(err.code, CoreError::ReadOnly.code());
        assert_eq!(err.params["connections_imported"], "2");
        assert!(do_snippets_get(&state).is_empty());
        let (on_disk, _) = ConnectionStore::load(dir.path().join("connections.toml")).unwrap();
        assert_eq!(on_disk.library().connections.len(), 2);
    }

    #[test]
    fn library_import_pick_remembers_the_file_until_imported() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let err = do_library_import_pending(&state, &[]).unwrap_err();
        assert_eq!(err.code, "io");
        let pick = do_library_import_pick(&state, path.clone()).unwrap();
        assert_eq!(pick.path, path.display().to_string());
        let selected: Vec<Uuid> = pick.preview.rows.iter().map(|r| r.id).collect();
        assert_eq!(
            do_library_import_pending(&state, &selected)
                .unwrap()
                .connections,
            2
        );
        // Imported once: a second confirm must not add the same connections again.
        assert_eq!(
            do_library_import_pending(&state, &selected)
                .unwrap_err()
                .code,
            "io"
        );
        assert_eq!(do_library_get(&state).connections.len(), 2);
    }

    #[test]
    fn library_import_pick_of_a_bad_file_forgets_the_previous_one() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        do_library_import_pick(&state, path).unwrap();
        assert!(do_library_import_pick(&state, files.path().to_path_buf()).is_err());
        assert!(do_library_import_pending(&state, &[]).is_err());
    }

    #[test]
    fn library_import_partly_done_is_not_repeated() {
        let files = TempDir::new().unwrap();
        let path = exported(&files);
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("snippets.toml"), "version = 9\n").unwrap();
        let state =
            AppState::load(dir.path(), None, Arc::new(MemorySecretStore::default())).unwrap();
        let selected = do_library_import_pick(&state, path)
            .unwrap()
            .preview
            .rows
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        do_library_import_pending(&state, &selected).unwrap_err();
        assert_eq!(
            do_library_import_pending(&state, &selected)
                .unwrap_err()
                .code,
            "io"
        );
        assert_eq!(do_library_get(&state).connections.len(), 2);
    }

    #[test]
    fn export_file_name_uses_the_date() {
        assert_eq!(
            export_file_name("2026-10-30T12:00:00Z"),
            "pylentra-export-2026-10-30.toml"
        );
        // Local time from the UI: just after midnight is already the new day.
        assert_eq!(
            export_file_name("2026-10-30T00:30:00+01:00"),
            "pylentra-export-2026-10-30.toml"
        );
        for odd in ["", "2026", "../../etc/x.toml"] {
            assert_eq!(export_file_name(odd), "pylentra-export.toml");
        }
    }

    #[test]
    fn export_path_gets_toml_when_it_has_no_extension() {
        let cases = [
            ("/d/backup", "/d/backup.toml"),
            ("/d/backup.toml", "/d/backup.toml"),
            ("/d/backup.txt", "/d/backup.txt"),
        ];
        for (picked, saved) in cases {
            assert_eq!(with_toml(PathBuf::from(picked)), PathBuf::from(saved));
        }
    }

    #[test]
    fn key_check_skips_network_paths() {
        let d = TempDir::new().unwrap();
        let key = d.path().join("id");
        std::fs::write(&key, "k").unwrap();
        assert!(key_found_or_unchecked(&key));
        assert!(!key_found_or_unchecked(&d.path().join("missing")));
        for unc in [r"\\host\share\id", "//host/share/id"] {
            assert!(key_found_or_unchecked(Path::new(unc)));
        }
    }

    #[test]
    fn library_import_again_skips_saved_global_snippets() {
        let (d, src) = state_with(Arc::new(MemorySecretStore::default()));
        let a = conn("a");
        do_connection_save(&src, a.clone(), None, None).unwrap();
        do_snippet_save(&src, snippet("g", None)).unwrap();
        do_snippet_save(&src, snippet("on a", Some(a.id))).unwrap();
        let path = d.path().join("export.toml");
        do_library_export(&src, &path, "2026-10-30").unwrap();
        let (_d, state) = state_with(Arc::new(MemorySecretStore::default()));
        // The second time the global snippet is already there; the one on
        // `a` follows its new connection.
        for (global, added) in [(1, 2), (0, 1)] {
            let p = do_library_import_preview(&state, &path).unwrap();
            assert_eq!((p.snippets_global, p.snippets_per_connection), (global, 1));
            let selected = all_rows(&state, &path);
            let counts = do_library_import(&state, &path, &selected).unwrap();
            assert_eq!(counts.snippets, added);
        }
        let globals = do_snippets_get(&state)
            .iter()
            .filter(|s| s.connection_id.is_none())
            .count();
        assert_eq!((globals, do_snippets_get(&state).len()), (1, 3));
    }

    #[test]
    fn library_import_without_snippets_ignores_unavailable_store() {
        let (d, src) = state_with(Arc::new(MemorySecretStore::default()));
        do_connection_save(&src, conn("a"), None, None).unwrap();
        let path = d.path().join("export.toml");
        do_library_export(&src, &path, "2026-10-30").unwrap();
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        *state.snippets() = SnippetStore::unavailable(d.path().join("snippets.toml"));
        let selected = all_rows(&state, &path);
        assert_eq!(
            do_library_import(&state, &path, &selected)
                .unwrap()
                .connections,
            1
        );
    }

    #[test]
    fn library_import_reads_only_regular_files_up_to_5_mib() {
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let big = d.path().join("big.toml");
        std::fs::write(&big, vec![b'#'; 5 * 1024 * 1024 + 1]).unwrap();
        for (path, reason) in [(d.path(), "not a regular file"), (&*big, "file too large")] {
            for err in [
                do_library_import_preview(&state, path).unwrap_err(),
                do_library_import(&state, path, &[]).unwrap_err(),
            ] {
                assert_eq!(err.code, "config.invalid");
                assert_eq!(err.params["reason"], reason);
            }
        }
        // At the limit the size check passes and the parse decides.
        std::fs::write(&big, vec![b'#'; 5 * 1024 * 1024]).unwrap();
        let err = do_library_import_preview(&state, &big).unwrap_err();
        assert_eq!(err.params["field"], "format");
    }

    #[test]
    fn library_export_rejects_long_exported_at() {
        let (d, state) = state_with(Arc::new(MemorySecretStore::default()));
        let path = d.path().join("export.toml");
        let err = do_library_export(&state, &path, &"x".repeat(65)).unwrap_err();
        assert_eq!(err.code, "config.invalid");
        assert_eq!(err.params["field"], "exported_at");
        assert!(!path.exists());
        assert!(do_library_export(&state, &path, &"x".repeat(64)).is_ok());
    }

    #[test]
    fn write_args_reads_raw_body_and_id_header() {
        let id = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert("id", id.to_string().parse().unwrap());
        let raw = InvokeBody::Raw(vec![0xc3, 0xa8]);
        assert_eq!(write_args(&headers, &raw).unwrap(), (id, vec![0xc3, 0xa8]));
        let json = InvokeBody::Json(serde_json::json!([1, 2]));
        assert_eq!(write_args(&headers, &json).unwrap(), (id, vec![1, 2]));
        let bad = InvokeBody::Json(serde_json::json!([256]));
        assert!(write_args(&headers, &bad).is_err());
        assert!(write_args(&HeaderMap::new(), &raw).is_err());
    }
}
