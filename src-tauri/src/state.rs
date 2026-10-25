use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use pylentra_core::error::{ApiError, CoreError};
use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::secrets::SecretStore;
use pylentra_core::snippets::SnippetStore;
use pylentra_core::ssh::{SessionManager, SessionOptions};
use pylentra_core::store::{ConnectionStore, LoadOutcome, SettingsStore};

pub struct AppState {
    pub store: Mutex<ConnectionStore>,
    pub settings: Mutex<SettingsStore>,
    pub snippets: Mutex<SnippetStore>,
    pub sessions: SessionManager,
    pub secrets: Arc<dyn SecretStore>,
    pub known_hosts: Arc<KnownHosts>,
    pub warnings: Vec<ApiError>,
}

impl AppState {
    /// `openssh_known_hosts` is only ever read.
    pub fn load(
        config_dir: &Path,
        openssh_known_hosts: Option<PathBuf>,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self, CoreError> {
        let (store, conn_outcome) = ConnectionStore::load(config_dir.join("connections.toml"))?;
        let (settings, settings_outcome) = SettingsStore::load(config_dir.join("settings.toml"));
        // Snippets are secondary: an unreadable file must not stop the app.
        let snippets_path = config_dir.join("snippets.toml");
        let (snippets, snippets_warning) = match SnippetStore::load(snippets_path.clone()) {
            Ok((s, outcome)) => (s, warning(outcome)),
            Err(e) => {
                log::warn!("cannot read snippets: {e}");
                let w = ApiError {
                    code: "config.unreadable".into(),
                    params: [("reason".into(), e.to_string())].into(),
                };
                (SnippetStore::unavailable(snippets_path), Some(w))
            }
        };
        let known_hosts = Arc::new(KnownHosts::new(
            config_dir.join("known_hosts"),
            openssh_known_hosts,
        ));
        let (connect_timeout, keepalive_interval) = settings.get().session_timeouts();
        let sessions = SessionManager::new(
            secrets.clone(),
            known_hosts.clone(),
            SessionOptions {
                connect_timeout,
                keepalive_interval,
                ..SessionOptions::default()
            },
        );
        let warnings = [conn_outcome, settings_outcome]
            .into_iter()
            .filter_map(warning)
            .chain(snippets_warning.map(|mut w| {
                w.params.insert("file".into(), "snippets.toml".into());
                w
            }))
            .collect();
        Ok(Self {
            store: Mutex::new(store),
            settings: Mutex::new(settings),
            snippets: Mutex::new(snippets),
            sessions,
            secrets,
            known_hosts,
            warnings,
        })
    }

    pub fn store(&self) -> MutexGuard<'_, ConnectionStore> {
        // A poisoned store still holds the last committed library.
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Lock order: `store()` before `snippets()`, never the reverse.
    pub fn snippets(&self) -> MutexGuard<'_, SnippetStore> {
        self.snippets.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings(&self) -> MutexGuard<'_, SettingsStore> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn warning(outcome: LoadOutcome) -> Option<ApiError> {
    let (code, param) = match outcome {
        LoadOutcome::Ok | LoadOutcome::CreatedEmpty => return None,
        LoadOutcome::RecoveredFromCorrupt { backup } => (
            "config.recovered_corrupt",
            ("backup", backup.display().to_string()),
        ),
        LoadOutcome::ReadOnlyNewerVersion { found } => {
            ("config.read_only", ("found", found.to_string()))
        }
    };
    Some(ApiError {
        code: code.into(),
        params: [(param.0.into(), param.1)].into(),
    })
}
