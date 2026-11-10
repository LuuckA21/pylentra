//! SFTP listing over the in-process server, which serves a temp folder as "/".
mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pylentra_core::error::CoreError;
use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{SessionEvent, Stage};
use pylentra_core::ssh::sftp::{EntryKind, Listing};
use pylentra_core::ssh::{SessionId, SessionManager, SessionOptions};
use support::{CollectSink, Hostile, TestServer, TestServerConfig};
use tempfile::TempDir;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";

struct Env {
    _dir: TempDir,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
    server: TestServer,
}

/// `trusted: false` leaves the host key unknown, so the session waits at its prompt.
async fn env(sftp_root: Option<&Path>, sftp_hostile: Hostile, trusted: bool) -> Env {
    let dir = TempDir::new().unwrap();
    let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
    let secrets = Arc::new(MemorySecretStore::default());
    let mgr = SessionManager::new(
        secrets.clone(),
        known_hosts.clone(),
        SessionOptions::default(),
    );
    let server = TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        sftp_root: sftp_root.map(Path::to_path_buf),
        sftp_hostile,
        ..Default::default()
    })
    .await;
    if trusted {
        known_hosts
            .learn("127.0.0.1", server.addr.port(), &server.host_key)
            .unwrap();
    }
    Env {
        _dir: dir,
        secrets,
        mgr,
        server,
    }
}

fn conn(env: &Env) -> Connection {
    let id = Uuid::new_v4();
    env.secrets
        .set(id, SecretKind::Password, &PW.to_string().into())
        .unwrap();
    Connection {
        id,
        name: "sftp".into(),
        group_id: None,
        host: "127.0.0.1".into(),
        port: env.server.addr.port(),
        username: "u".into(),
        auth: AuthMethod::Password { save: false },
        jump_via: None,
        forwards: vec![],
        color: None,
        icon: None,
        notes: None,
        extra: Default::default(),
    }
}

async fn open_ready(env: &Env) -> SessionId {
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![conn(env)], 80, 24, sink.clone());
    let ready = SessionEvent::Status {
        stage: Stage::Ready,
    };
    sink.wait_event(|e| *e == ready, T).await;
    id
}

fn names(l: &Listing) -> Vec<&str> {
    let mut n: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
    n.sort();
    n
}

#[tokio::test]
async fn list_home() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a.txt"), "abc").unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let id = open_ready(&env).await;

    let l = env.mgr.sftp_list(id, String::new()).await.unwrap();
    assert_eq!(l.dir, "/");
    assert!(!l.truncated);
    assert_eq!(names(&l), ["a.txt", "d"]);
    let a = l.entries.iter().find(|e| e.name == "a.txt").unwrap();
    assert_eq!((&a.kind, a.size), (&EntryKind::File, Some(3)));
    assert!(a.modified.is_some());
    let d = l.entries.iter().find(|e| e.name == "d").unwrap();
    assert_eq!(d.kind, EntryKind::Dir);

    // The open SFTP channel is reused, and paths are canonicalized.
    let sub = env.mgr.sftp_list(id, "/d/../d/.".into()).await.unwrap();
    assert_eq!((sub.dir.as_str(), sub.entries.len()), ("/d", 0));
    let missing = env.mgr.sftp_list(id, "/nope".into()).await.unwrap_err();
    assert!(
        matches!(&missing, CoreError::NotFound { what } if what == "/nope"),
        "{missing:?}"
    );
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn list_hides_invalid_names() {
    let root = TempDir::new().unwrap();
    let extra = ["../x", "a/b", "CON", "ok"].map(String::from).to_vec();
    let env = env(Some(root.path()), Hostile::Names(extra), true).await;
    let id = open_ready(&env).await;

    let l = env.mgr.sftp_list(id, String::new()).await.unwrap();
    let want: &[&str] = if cfg!(windows) {
        &["ok"]
    } else {
        &["CON", "ok"]
    };
    assert_eq!(names(&l), want);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn list_before_ready_fails() {
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::None, false).await;
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![conn(&env)], 80, 24, sink.clone());
    sink.wait_event(|e| matches!(e, SessionEvent::HostKeyPrompt { .. }), T)
        .await;

    let err = env.mgr.sftp_list(id, String::new()).await.unwrap_err();
    assert!(
        matches!(&err, CoreError::Ssh(m) if m == "session is not ready"),
        "{err:?}"
    );
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn sftp_refused() {
    let env = env(None, Hostile::None, true).await;
    let id = open_ready(&env).await;

    let err = env.mgr.sftp_list(id, String::new()).await.unwrap_err();
    assert!(matches!(err, CoreError::SftpUnavailable), "{err:?}");
    // Not cached: asking again asks the server again.
    let err = env.mgr.sftp_list(id, String::new()).await.unwrap_err();
    assert!(matches!(err, CoreError::SftpUnavailable), "{err:?}");
    env.mgr.close(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn list_marks_links() {
    let root = TempDir::new().unwrap();
    // Points outside the root: listing must not follow it.
    std::os::unix::fs::symlink("/", root.path().join("l")).unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let id = open_ready(&env).await;

    let l = env.mgr.sftp_list(id, String::new()).await.unwrap();
    assert_eq!(names(&l), ["l"]);
    assert_eq!(l.entries[0].kind, EntryKind::Link);
    env.mgr.close(id).await.unwrap();
}
