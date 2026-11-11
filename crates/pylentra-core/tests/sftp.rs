//! SFTP listing and transfers over the in-process server, which serves a temp folder as "/".
mod support;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use pylentra_core::error::CoreError;
use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection};
use pylentra_core::secrets::{MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{
    ConflictChoice, PromptAnswer, SessionEvent, SkipReason, Skipped, Stage, TransferState,
};
use pylentra_core::ssh::sftp::{Direction, EntryKind, Listing};
use pylentra_core::ssh::{CLOSE_TIMEOUT, SessionId, SessionManager, SessionOptions};
use support::{CollectSink, Hostile, TestServer, TestServerConfig};
use tempfile::TempDir;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";
/// What a close or cancel may take: the session's own deadline, plus room for a loaded machine.
const BUDGET: Duration = CLOSE_TIMEOUT.saturating_add(Duration::from_secs(1));

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
async fn duplicate_names_appear_once() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("top")).unwrap();
    let dup = ["ok", "ok"].map(String::from).to_vec();
    let env = env(Some(root.path()), Hostile::Names(dup), true).await;
    let id = open_ready(&env).await;

    let l = env.mgr.sftp_list(id, "/top".into()).await.unwrap();
    assert_eq!(names(&l), ["ok"]);
    let plan = env.mgr.sftp_scan(id, vec!["/top".into()]).await.unwrap();
    let files: Vec<_> = plan.files.iter().map(|f| f.rel.as_str()).collect();
    assert_eq!(files, ["top/ok"]);
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

#[tokio::test]
async fn stuck_sftp_never_blocks_terminal() {
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::NeverAnswer, true).await;
    // Bounds the SFTP handshake as well as the connection.
    env.mgr.set_timeouts(Duration::from_secs(2), None);
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![conn(&env)], 80, 24, sink.clone());
    let ready = SessionEvent::Status {
        stage: Stage::Ready,
    };
    sink.wait_event(|e| *e == ready, T).await;

    // `join!` polls in order, so the list command is queued before the write.
    let typed = async {
        env.mgr.write(id, b"hi".to_vec()).await.unwrap();
        // Well before the SFTP handshake gives up.
        sink.wait_data_len(2, Duration::from_millis(1000)).await;
    };
    let (listed, ()) = tokio::join!(env.mgr.sftp_list(id, String::new()), typed);
    let err = listed.unwrap_err();
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

#[tokio::test]
async fn remote_tree() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("a/b")).unwrap();
    fs::write(root.path().join("a/b/c.txt"), "abc").unwrap();
    fs::write(root.path().join("a/x.txt"), "xy").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let id = open_ready(&env).await;

    let plan = env.mgr.sftp_scan(id, vec!["/a/".into()]).await.unwrap();
    assert_eq!(plan.dirs, ["a", "a/b"]);
    let mut files: Vec<_> = plan.files.iter().map(|f| f.rel.as_str()).collect();
    files.sort();
    assert_eq!(files, ["a/b/c.txt", "a/x.txt"]);
    assert_eq!((plan.total_bytes, plan.skipped.len()), (5, 0));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn remote_endless_dirs_stop() {
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::EndlessDirs, true).await;
    let id = open_ready(&env).await;

    fs::create_dir(root.path().join("top")).unwrap();
    let err = env
        .mgr
        .sftp_scan(id, vec!["/top".into()])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CoreError::SftpLimit { what } if what == "depth"),
        "{err:?}"
    );
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn remote_bad_names_skipped() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("top")).unwrap();
    let env = env(Some(root.path()), Hostile::Names(vec!["../x".into()]), true).await;
    let id = open_ready(&env).await;

    let plan = env.mgr.sftp_scan(id, vec!["/top".into()]).await.unwrap();
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].path, "top/../x");
    assert_eq!(plan.skipped[0].reason, SkipReason::InvalidName);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn endless_dots_end_the_listing() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("top")).unwrap();
    let env = env(Some(root.path()), Hostile::EndlessDots, true).await;
    let id = open_ready(&env).await;

    let l = env.mgr.sftp_list(id, "/top".into()).await.unwrap();
    assert!(l.truncated && l.entries.is_empty());
    let err = env
        .mgr
        .sftp_scan(id, vec!["/top".into()])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, CoreError::SftpLimit { what } if what == "entries"),
        "{err:?}"
    );
    env.mgr.close(id).await.unwrap();
}

/// The transfer events of `transfer_id`, in order.
fn transfer(sink: &CollectSink, transfer_id: u32) -> Vec<TransferState> {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            SessionEvent::Transfer {
                transfer_id: t,
                state,
            } if *t == transfer_id => Some(state.clone()),
            _ => None,
        })
        .collect()
}

fn is_end(s: &TransferState) -> bool {
    matches!(
        s,
        TransferState::Finished { .. }
            | TransferState::Failed { .. }
            | TransferState::Cancelled { .. }
    )
}

async fn wait_end(sink: &CollectSink, transfer_id: u32) -> TransferState {
    let ev = sink
        .wait_event(
            |e| matches!(e, SessionEvent::Transfer { transfer_id: t, state } if *t == transfer_id && is_end(state)),
            T,
        )
        .await;
    match ev {
        SessionEvent::Transfer { state, .. } => state,
        _ => unreachable!(),
    }
}

async fn open_with_sink(env: &Env) -> (SessionId, Arc<CollectSink>) {
    let sink = Arc::new(CollectSink::default());
    let id = env.mgr.open(vec![conn(env)], 80, 24, sink.clone());
    let ready = SessionEvent::Status {
        stage: Stage::Ready,
    };
    sink.wait_event(|e| *e == ready, T).await;
    (id, sink)
}

/// Every name under `dir`, recursively, relative and sorted.
fn tree(dir: &Path) -> Vec<String> {
    let mut out = vec![];
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if e.file_type().unwrap().is_dir() {
            out.push(format!("{name}/"));
            out.extend(tree(&e.path()).into_iter().map(|n| format!("{name}/{n}")));
        } else {
            out.push(name);
        }
    }
    out.sort();
    out
}

fn conflicts(sink: &CollectSink, transfer_id: u32) -> Vec<String> {
    transfer(sink, transfer_id)
        .into_iter()
        .filter_map(|s| match s {
            TransferState::Conflict { name } => Some(name),
            _ => None,
        })
        .collect()
}

async fn wait_conflicts(sink: &CollectSink, transfer_id: u32, n: usize) -> Vec<String> {
    tokio::time::timeout(T, async {
        loop {
            let c = conflicts(sink, transfer_id);
            if c.len() >= n {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("conflict question")
}

fn answer(env: &Env, id: SessionId, choice: ConflictChoice, all: bool) {
    env.mgr
        .answer(id, PromptAnswer::Conflict { choice, all })
        .unwrap();
}

#[tokio::test]
async fn download_file_and_tree() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f.txt"), "hello").unwrap();
    fs::create_dir_all(root.path().join("d/e")).unwrap();
    fs::write(root.path().join("d/x"), "xx").unwrap();
    fs::write(root.path().join("d/e/y"), "yyy").unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
    fs::File::options()
        .write(true)
        .open(root.path().join("d/e/y"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/f.txt".into(), "/d".into()], local.path().into())
        .await
        .unwrap();
    assert_eq!(t, 1);
    let end = wait_end(&sink, t).await;
    assert_eq!(
        end,
        TransferState::Finished {
            files: 3,
            skipped: vec![]
        }
    );
    assert_eq!(tree(local.path()), ["d/", "d/e/", "d/e/y", "d/x", "f.txt"]);
    assert_eq!(fs::read(local.path().join("f.txt")).unwrap(), b"hello");
    assert_eq!(fs::read(local.path().join("d/e/y")).unwrap(), b"yyy");
    let m = fs::metadata(local.path().join("d/e/y")).unwrap();
    assert_eq!(m.modified().unwrap(), old);
    let states = transfer(&sink, t);
    assert!(
        matches!(
            &states[0],
            TransferState::Queued {
                direction: Direction::Down,
                ..
            }
        ),
        "{states:?}"
    );
    assert!(
        states
            .iter()
            .any(|s| matches!(s, TransferState::Running { .. })),
        "{states:?}"
    );
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_half_reads() {
    let root = TempDir::new().unwrap();
    let mut body = vec![0u8; 1 << 20];
    rand::fill(&mut body[..]);
    fs::write(root.path().join("r"), &body).unwrap();
    let env = env(Some(root.path()), Hostile::HalfReads, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/r".into()], local.path().into())
        .await
        .unwrap();
    let end = wait_end(&sink, t).await;
    assert!(
        matches!(end, TransferState::Finished { files: 1, .. }),
        "{end:?}"
    );
    assert!(fs::read(local.path().join("r")).unwrap() == body);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_cancel_leaves_nothing() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("big"), vec![7u8; 8 << 20]).unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/big".into()], local.path().into())
        .await
        .unwrap();
    sink.wait_event(
        |e| {
            matches!(
                e,
                SessionEvent::Transfer {
                    state: TransferState::Running { .. },
                    ..
                }
            )
        },
        T,
    )
    .await;
    env.mgr.sftp_cancel(id, t).await.unwrap();
    assert_eq!(
        wait_end(&sink, t).await,
        TransferState::Cancelled { files: 0 }
    );
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
    // Gone from the queue: cancelling again finds nothing.
    let err = env.mgr.sftp_cancel(id, t).await.unwrap_err();
    assert!(matches!(err, CoreError::NotFound { .. }), "{err:?}");
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_conflict_choices() {
    let root = TempDir::new().unwrap();
    let local = TempDir::new().unwrap();
    for n in ["1", "2", "3"] {
        fs::write(root.path().join(n), "new").unwrap();
        fs::write(local.path().join(n), "old").unwrap();
    }
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = ["/1", "/2", "/3"].map(String::from).to_vec();
    let t = env
        .mgr
        .sftp_download(id, items, local.path().into())
        .await
        .unwrap();
    assert_eq!(wait_conflicts(&sink, t, 1).await, ["1"]);
    // A transfer waiting on the user never blocks a listing.
    assert_eq!(
        names(&env.mgr.sftp_list(id, String::new()).await.unwrap()),
        ["1", "2", "3"]
    );
    answer(&env, id, ConflictChoice::Skip, false);
    assert_eq!(wait_conflicts(&sink, t, 2).await, ["1", "2"]);
    answer(&env, id, ConflictChoice::Overwrite, true);
    let end = wait_end(&sink, t).await;
    assert_eq!(
        end,
        TransferState::Finished {
            files: 2,
            skipped: vec![Skipped {
                path: "1".into(),
                reason: SkipReason::Exists
            }]
        }
    );
    assert_eq!(conflicts(&sink, t).len(), 2);
    let read = |n: &str| fs::read_to_string(local.path().join(n)).unwrap();
    assert_eq!(
        (read("1"), read("2"), read("3")),
        ("old".into(), "new".into(), "new".into())
    );
    assert_eq!(tree(local.path()), ["1", "2", "3"]);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_conflict_cancel() {
    let root = TempDir::new().unwrap();
    let local = TempDir::new().unwrap();
    fs::write(root.path().join("1"), "new").unwrap();
    fs::write(root.path().join("2"), "new").unwrap();
    fs::write(local.path().join("2"), "old").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = ["/1", "/2"].map(String::from).to_vec();
    let t = env
        .mgr
        .sftp_download(id, items, local.path().into())
        .await
        .unwrap();
    wait_conflicts(&sink, t, 1).await;
    answer(&env, id, ConflictChoice::Cancel, false);
    assert_eq!(
        wait_end(&sink, t).await,
        TransferState::Cancelled { files: 1 }
    );
    let read = |n: &str| fs::read_to_string(local.path().join(n)).unwrap();
    assert_eq!((read("1"), read("2")), ("new".into(), "old".into()));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_queue_order() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("a"), vec![1u8; 1 << 20]).unwrap();
    fs::write(root.path().join("b"), "b").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let first = env
        .mgr
        .sftp_download(id, vec!["/a".into()], local.path().into())
        .await
        .unwrap();
    let second = env
        .mgr
        .sftp_download(id, vec!["/b".into()], local.path().into())
        .await
        .unwrap();
    assert_eq!((first, second), (1, 2));
    wait_end(&sink, second).await;
    let events = sink.events.lock().unwrap().clone();
    let pos = |pred: &dyn Fn(u32, &TransferState) -> bool| {
        events
            .iter()
            .position(|e| matches!(e, SessionEvent::Transfer { transfer_id, state } if pred(*transfer_id, state)))
            .unwrap()
    };
    let first_done = pos(&|t, s| t == first && matches!(s, TransferState::Finished { .. }));
    let second_runs = pos(&|t, s| t == second && matches!(s, TransferState::Running { .. }));
    assert!(first_done < second_runs, "{events:?}");
    env.mgr.close(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn download_into_symlinked_dir_skips() {
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("d")).unwrap();
    fs::write(root.path().join("d/f"), "f").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), local.path().join("d")).unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/d".into()], local.path().into())
        .await
        .unwrap();
    let end = wait_end(&sink, t).await;
    assert_eq!(
        end,
        TransferState::Finished {
            files: 0,
            skipped: vec![Skipped {
                path: "d".into(),
                reason: SkipReason::Symlink
            }]
        }
    );
    assert!(tree(elsewhere.path()).is_empty());
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_size_changed() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), vec![3u8; 100_000]).unwrap();
    let env = env(Some(root.path()), Hostile::ShortFile, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/f".into()], local.path().into())
        .await
        .unwrap();
    let end = wait_end(&sink, t).await;
    let TransferState::Finished { files: 0, skipped } = &end else {
        panic!("{end:?}");
    };
    assert_eq!(skipped.len(), 1);
    assert!(
        matches!(&skipped[0].reason, SkipReason::Error { error } if error.code == "not_found"),
        "{end:?}"
    );
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn close_during_conflict() {
    let root = TempDir::new().unwrap();
    let local = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "new").unwrap();
    fs::write(local.path().join("f"), "old").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let t = env
        .mgr
        .sftp_download(id, vec!["/f".into()], local.path().into())
        .await
        .unwrap();
    wait_conflicts(&sink, t, 1).await;
    // The pending question is dropped, so closing does not wait for the user.
    let started = std::time::Instant::now();
    env.mgr.close(id).await.unwrap();
    assert!(started.elapsed() < BUDGET);
    assert_eq!(
        transfer(&sink, t).last(),
        Some(&TransferState::Cancelled { files: 0 })
    );
    assert_eq!(tree(local.path()), ["f"]);
}

#[tokio::test]
async fn download_failed_read_skips_only_that_file() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("bad"), "x").unwrap();
    fs::write(root.path().join("good"), "g").unwrap();
    fs::write(root.path().join("next"), "n").unwrap();
    let env = env(Some(root.path()), Hostile::FailRead("bad".into()), true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let items = ["/bad", "/good"].map(String::from).to_vec();
    let first = env
        .mgr
        .sftp_download(id, items, local.path().into())
        .await
        .unwrap();
    let second = env
        .mgr
        .sftp_download(id, vec!["/next".into()], local.path().into())
        .await
        .unwrap();
    let end = wait_end(&sink, first).await;
    let TransferState::Finished { files: 1, skipped } = &end else {
        panic!("{end:?}");
    };
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].path, "bad");
    assert!(
        matches!(&skipped[0].reason, SkipReason::Error { error } if error.code == "ssh.protocol"),
        "{end:?}"
    );
    let end = wait_end(&sink, second).await;
    assert!(
        matches!(end, TransferState::Finished { files: 1, .. }),
        "{end:?}"
    );
    assert_eq!(tree(local.path()), ["good", "next"]);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn download_denied_item_skips_only_that_item() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("bad"), "x").unwrap();
    fs::write(root.path().join("good"), "g").unwrap();
    let env = env(Some(root.path()), Hostile::DenyLstat("bad".into()), true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let items = ["/bad", "/good"].map(String::from).to_vec();
    let t = env
        .mgr
        .sftp_download(id, items, local.path().into())
        .await
        .unwrap();
    let end = wait_end(&sink, t).await;
    let TransferState::Finished { files: 1, skipped } = &end else {
        panic!("{end:?}");
    };
    assert_eq!(skipped.len(), 1, "{end:?}");
    assert_eq!(skipped[0].path, "bad");
    assert!(
        matches!(&skipped[0].reason, SkipReason::Error { error } if error.code == "sftp.permission_denied"),
        "{end:?}"
    );
    assert_eq!(tree(local.path()), ["good"]);
    env.mgr.close(id).await.unwrap();
}

/// Waits until the transfer has made its temp file, so it is inside the copy.
async fn wait_temp(dir: &Path) {
    tokio::time::timeout(T, async {
        while !tree(dir).iter().any(|n| n.ends_with(".pylentra-tmp")) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("temp file");
}

#[tokio::test]
async fn cancel_wakes_a_stalled_read() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "data").unwrap();
    let env = env(Some(root.path()), Hostile::StallReads, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/f".into()], local.path().into())
        .await
        .unwrap();
    wait_temp(local.path()).await;
    let started = std::time::Instant::now();
    env.mgr.sftp_cancel(id, t).await.unwrap();
    assert_eq!(
        wait_end(&sink, t).await,
        TransferState::Cancelled { files: 0 }
    );
    assert!(started.elapsed() < BUDGET);
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn close_during_stalled_read_cleans_up() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "data").unwrap();
    let env = env(Some(root.path()), Hostile::StallReads, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/f".into()], local.path().into())
        .await
        .unwrap();
    wait_temp(local.path()).await;
    let started = std::time::Instant::now();
    env.mgr.close(id).await.unwrap();
    assert!(started.elapsed() < BUDGET);
    // Cleaned up by the transfer itself, before the session reported `Closed`.
    let events = sink.events.lock().unwrap().clone();
    let cancelled = events.iter().position(|e| {
        *e == SessionEvent::Transfer {
            transfer_id: t,
            state: TransferState::Cancelled { files: 0 },
        }
    });
    let closed = events
        .iter()
        .position(|e| matches!(e, SessionEvent::Closed { .. }));
    assert!(
        matches!((cancelled, closed), (Some(a), Some(b)) if a < b),
        "{events:?}"
    );
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
}

#[tokio::test]
async fn close_with_stalled_listing_leaves_no_temp() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "data").unwrap();
    let env = env(Some(root.path()), Hostile::StallReads, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    env.mgr
        .sftp_download(id, vec!["/f".into()], local.path().into())
        .await
        .unwrap();
    wait_temp(local.path()).await;
    let started = std::time::Instant::now();
    // `join!` polls in order: the listing is queued, and stuck on the server, before the close.
    let (listed, closed) = tokio::join!(env.mgr.sftp_list(id, String::new()), env.mgr.close(id));
    closed.unwrap();
    assert!(listed.is_err());
    // The SFTP task is aborted at its budget, mid-copy: the temp file goes with it.
    assert!(started.elapsed() < BUDGET);
    sink.wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await;
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
}

#[tokio::test]
async fn close_during_transfer() {
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("big"), vec![7u8; 8 << 20]).unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;
    let local = TempDir::new().unwrap();

    let t = env
        .mgr
        .sftp_download(id, vec!["/big".into()], local.path().into())
        .await
        .unwrap();
    wait_running(&sink, t).await;
    env.mgr.close(id).await.unwrap();
    sink.wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await;
    assert_eq!(
        transfer(&sink, t).last(),
        Some(&TransferState::Cancelled { files: 0 })
    );
    assert!(tree(local.path()).is_empty(), "{:?}", tree(local.path()));
}

async fn wait_running(sink: &CollectSink, transfer_id: u32) {
    sink.wait_event(
        |e| {
            matches!(
                e,
                SessionEvent::Transfer {
                    transfer_id: t,
                    state: TransferState::Running { .. },
                } if *t == transfer_id
            )
        },
        T,
    )
    .await;
}

fn set_mtime(p: &Path, t: SystemTime) {
    fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[tokio::test]
async fn upload_file_and_tree() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("f.txt"), "hello").unwrap();
    fs::create_dir_all(local.path().join("d/e")).unwrap();
    fs::write(local.path().join("d/x"), "xx").unwrap();
    fs::write(local.path().join("d/e/y"), "yyy").unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
    set_mtime(&local.path().join("d/e/y"), old);
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("up")).unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("f.txt"), local.path().join("d")];
    let t = env.mgr.sftp_upload(id, items, "/up".into()).await.unwrap();
    let end = wait_end(&sink, t).await;
    assert_eq!(
        end,
        TransferState::Finished {
            files: 3,
            skipped: vec![]
        }
    );
    let up = root.path().join("up");
    assert_eq!(tree(&up), ["d/", "d/e/", "d/e/y", "d/x", "f.txt"]);
    // Before any read, which would move the access time.
    let m = fs::metadata(up.join("d/e/y")).unwrap();
    assert_eq!((m.modified().unwrap(), m.accessed().unwrap()), (old, old));
    assert_eq!(fs::read(up.join("f.txt")).unwrap(), b"hello");
    assert_eq!(fs::read(up.join("d/e/y")).unwrap(), b"yyy");
    let states = transfer(&sink, t);
    assert!(
        matches!(
            &states[0],
            TransferState::Queued {
                direction: Direction::Up,
                ..
            }
        ),
        "{states:?}"
    );
    env.mgr.close(id).await.unwrap();
}

/// A bigger file over an existing one, so several blocks are in flight.
async fn overwrite(hostile: Hostile) {
    let local = TempDir::new().unwrap();
    let mut body = vec![0u8; 300_000];
    rand::fill(&mut body[..]);
    fs::write(local.path().join("f"), &body).unwrap();
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "old").unwrap();
    let env = env(Some(root.path()), hostile, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("f")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    assert_eq!(wait_conflicts(&sink, t, 1).await, ["f"]);
    answer(&env, id, ConflictChoice::Overwrite, false);
    let end = wait_end(&sink, t).await;
    assert!(
        matches!(end, TransferState::Finished { files: 1, .. }),
        "{end:?}"
    );
    assert!(fs::read(root.path().join("f")).unwrap() == body);
    assert_eq!(tree(root.path()), ["f"]);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn upload_overwrite_atomic() {
    overwrite(Hostile::None).await;
}

#[tokio::test]
async fn upload_without_posix_rename() {
    overwrite(Hostile::NoPosixRename).await;
}

#[tokio::test]
async fn upload_conflict_choices() {
    let root = TempDir::new().unwrap();
    let local = TempDir::new().unwrap();
    for n in ["1", "2", "3"] {
        fs::write(local.path().join(n), "new").unwrap();
        fs::write(root.path().join(n), "old").unwrap();
    }
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = ["1", "2", "3"].map(|n| local.path().join(n)).to_vec();
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    assert_eq!(wait_conflicts(&sink, t, 1).await, ["1"]);
    answer(&env, id, ConflictChoice::Skip, false);
    assert_eq!(wait_conflicts(&sink, t, 2).await, ["1", "2"]);
    answer(&env, id, ConflictChoice::Overwrite, true);
    let end = wait_end(&sink, t).await;
    assert_eq!(
        end,
        TransferState::Finished {
            files: 2,
            skipped: vec![Skipped {
                path: "1".into(),
                reason: SkipReason::Exists
            }]
        }
    );
    assert_eq!(conflicts(&sink, t).len(), 2);
    let read = |n: &str| fs::read_to_string(root.path().join(n)).unwrap();
    assert_eq!(
        (read("1"), read("2"), read("3")),
        ("old".into(), "new".into(), "new".into())
    );
    assert_eq!(tree(root.path()), ["1", "2", "3"]);
    env.mgr.close(id).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn upload_never_through_remote_links() {
    let root = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), root.path().join("d")).unwrap();
    fs::write(elsewhere.path().join("target"), "keep").unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join("target"), root.path().join("f")).unwrap();
    let local = TempDir::new().unwrap();
    fs::create_dir(local.path().join("d")).unwrap();
    fs::write(local.path().join("d/x"), "x").unwrap();
    fs::write(local.path().join("f"), "f").unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("d"), local.path().join("f")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    let TransferState::Finished { files: 0, skipped } = wait_end(&sink, t).await else {
        panic!("{:?}", transfer(&sink, t));
    };
    let mut skipped: Vec<_> = skipped.into_iter().map(|s| (s.path, s.reason)).collect();
    skipped.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        skipped,
        [
            ("d".into(), SkipReason::Symlink),
            ("f".into(), SkipReason::Symlink)
        ]
    );
    assert!(conflicts(&sink, t).is_empty());
    assert_eq!(tree(elsewhere.path()), ["target"]);
    assert_eq!(
        fs::read_to_string(elsewhere.path().join("target")).unwrap(),
        "keep"
    );
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn upload_cancel_leaves_nothing() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("big"), vec![7u8; 8 << 20]).unwrap();
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("big")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    wait_running(&sink, t).await;
    env.mgr.sftp_cancel(id, t).await.unwrap();
    assert_eq!(
        wait_end(&sink, t).await,
        TransferState::Cancelled { files: 0 }
    );
    // Removed on the server before `Cancelled` was sent.
    assert!(tree(root.path()).is_empty(), "{:?}", tree(root.path()));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn close_during_upload_cleans_server() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("big"), vec![7u8; 8 << 20]).unwrap();
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::None, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("big")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    wait_running(&sink, t).await;
    let started = std::time::Instant::now();
    env.mgr.close(id).await.unwrap();
    assert!(started.elapsed() < BUDGET);
    assert_eq!(
        transfer(&sink, t).last(),
        Some(&TransferState::Cancelled { files: 0 })
    );
    assert!(tree(root.path()).is_empty(), "{:?}", tree(root.path()));
}

#[tokio::test]
async fn connection_drop_during_upload() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("f"), vec![1u8; 1 << 20]).unwrap();
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "old").unwrap();
    let env = env(Some(root.path()), Hostile::DropAfterWrite(64 * 1024), true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("f")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    wait_conflicts(&sink, t, 1).await;
    answer(&env, id, ConflictChoice::Overwrite, false);
    let end = wait_end(&sink, t).await;
    assert!(matches!(end, TransferState::Failed { .. }), "{end:?}");
    // The temp file may remain (nothing can reach the server any more); the file never breaks.
    assert_eq!(fs::read_to_string(root.path().join("f")).unwrap(), "old");
    sink.wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await;
}

#[tokio::test]
async fn upload_denied_folder_skipped() {
    let local = TempDir::new().unwrap();
    fs::create_dir_all(local.path().join("top/a")).unwrap();
    fs::create_dir_all(local.path().join("top/b")).unwrap();
    fs::write(local.path().join("top/a/x"), "x").unwrap();
    fs::write(local.path().join("top/b/y"), "y").unwrap();
    let root = TempDir::new().unwrap();
    // Exists, so mkdir fails and the denied lstat decides.
    fs::create_dir_all(root.path().join("top/a")).unwrap();
    let env = env(Some(root.path()), Hostile::DenyLstat("a".into()), true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("top")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    let end = wait_end(&sink, t).await;
    let TransferState::Finished { files: 1, skipped } = &end else {
        panic!("{end:?}");
    };
    assert_eq!(skipped.len(), 1, "{end:?}");
    assert_eq!(skipped[0].path, "top/a");
    assert!(
        matches!(&skipped[0].reason, SkipReason::Error { error } if error.code == "sftp.permission_denied"),
        "{end:?}"
    );
    assert_eq!(tree(root.path()), ["top/", "top/a/", "top/b/", "top/b/y"]);
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn cancel_during_temp_create_leaves_nothing() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("f"), "new").unwrap();
    let root = TempDir::new().unwrap();
    let env = env(Some(root.path()), Hostile::SlowCreate, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("f")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    // Created on the server, its handle not yet answered.
    wait_temp(root.path()).await;
    env.mgr.sftp_cancel(id, t).await.unwrap();
    assert_eq!(
        wait_end(&sink, t).await,
        TransferState::Cancelled { files: 0 }
    );
    assert!(tree(root.path()).is_empty(), "{:?}", tree(root.path()));
    env.mgr.close(id).await.unwrap();
}

#[tokio::test]
async fn cancel_during_replace_keeps_a_file() {
    let local = TempDir::new().unwrap();
    fs::write(local.path().join("f"), "new").unwrap();
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("f"), "old").unwrap();
    let env = env(Some(root.path()), Hostile::SlowReplace, true).await;
    let (id, sink) = open_with_sink(&env).await;

    let items = vec![local.path().join("f")];
    let t = env.mgr.sftp_upload(id, items, "/".into()).await.unwrap();
    wait_conflicts(&sink, t, 1).await;
    answer(&env, id, ConflictChoice::Overwrite, false);
    // The old file is removed, the rename not yet sent.
    tokio::time::timeout(T, async {
        while root.path().join("f").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let _ = env.mgr.sftp_cancel(id, t).await;
    wait_end(&sink, t).await;
    assert_eq!(fs::read_to_string(root.path().join("f")).unwrap(), "new");
    assert_eq!(tree(root.path()), ["f"]);
    env.mgr.close(id).await.unwrap();
}
