//! Authentication end to end: password, keyboard-interactive, key files and the SSH agent.
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use pylentra_core::error::ApiError;
use pylentra_core::known_hosts::KnownHosts;
use pylentra_core::model::{AuthMethod, Connection};
use pylentra_core::secrets::{FailingSecretStore, MemorySecretStore, SecretKind, SecretStore};
use pylentra_core::ssh::events::{
    CloseReason, KbdPrompt, PromptAnswer, PromptKind, SessionEvent, Stage,
};
use pylentra_core::ssh::{SessionId, SessionManager, SessionOptions};
use russh::keys::ssh_key::{Cipher, Kdf, LineEnding};
use russh::keys::{Algorithm, PrivateKey};
use secrecy::ExposeSecret;
use support::{CollectSink, TestServer, TestServerConfig};
use tempfile::TempDir;
use uuid::Uuid;

const T: Duration = Duration::from_secs(5);
const PW: &str = "pw";
const PASSPHRASE: &str = "correct horse";

struct Env {
    dir: TempDir,
    known_hosts: Arc<KnownHosts>,
    secrets: Arc<MemorySecretStore>,
    mgr: SessionManager,
}

impl Env {
    fn new(opts: SessionOptions) -> Self {
        Self::with_store(opts, None)
    }

    /// `store` replaces the in-memory store the manager would otherwise use.
    fn with_store(opts: SessionOptions, store: Option<Arc<dyn SecretStore>>) -> Self {
        let dir = TempDir::new().unwrap();
        let known_hosts = Arc::new(KnownHosts::new(dir.path().join("known_hosts"), None));
        let secrets = Arc::new(MemorySecretStore::default());
        let store = store.unwrap_or_else(|| secrets.clone());
        let mgr = SessionManager::new(store, known_hosts.clone(), opts);
        Env {
            dir,
            known_hosts,
            secrets,
            mgr,
        }
    }

    /// Opens `c` against `server`, whose host key is trusted first.
    fn open(&self, server: &TestServer, c: Connection) -> (SessionId, Arc<CollectSink>) {
        self.known_hosts
            .learn("127.0.0.1", server.addr.port(), &server.host_key)
            .unwrap();
        let sink = Arc::new(CollectSink::default());
        let id = self.mgr.open(vec![c], 80, 24, sink.clone());
        (id, sink)
    }

    fn stored(&self, c: &Connection, kind: SecretKind) -> Option<String> {
        self.secrets
            .get(c.id, kind)
            .unwrap()
            .map(|s| s.expose_secret().to_owned())
    }

    fn store(&self, c: &Connection, kind: SecretKind, value: &str) {
        self.secrets
            .set(c.id, kind, &value.to_string().into())
            .unwrap();
    }

    fn key_file(&self, key: &PrivateKey, passphrase: Option<&str>) -> PathBuf {
        write_key(self.dir.path(), key, passphrase)
    }
}

/// Writes `key` as an OpenSSH file in `dir`, encrypted when `passphrase` is given.
fn write_key(dir: &Path, key: &PrivateKey, passphrase: Option<&str>) -> PathBuf {
    let path = dir.join(format!("id_{}", Uuid::new_v4()));
    let key = match passphrase {
        // One bcrypt round keeps the tests fast; the format is the same.
        Some(p) => key
            .encrypt_with(
                Cipher::Aes256Ctr,
                Kdf::Bcrypt {
                    salt: vec![7; 16],
                    rounds: 1,
                },
                42,
                p,
            )
            .unwrap(),
        None => key.clone(),
    };
    key.write_openssh_file(&path, LineEnding::LF).unwrap();
    path
}

fn env() -> Env {
    Env::new(SessionOptions::default())
}

fn conn(server: &TestServer, auth: AuthMethod) -> Connection {
    Connection {
        id: Uuid::new_v4(),
        name: "test".into(),
        group_id: None,
        host: "127.0.0.1".into(),
        port: server.addr.port(),
        username: "u".into(),
        auth,
        jump_via: None,
        forwards: vec![],
        color: None,
        icon: None,
    }
}

fn password_auth() -> AuthMethod {
    AuthMethod::Password { save: false }
}

fn key_auth(path: &Path) -> AuthMethod {
    AuthMethod::Key {
        path: path.to_path_buf(),
        save_passphrase: false,
    }
}

fn ed25519() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

async fn pw_server() -> TestServer {
    TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        ..Default::default()
    })
    .await
}

async fn key_server(key: &PrivateKey) -> TestServer {
    TestServer::start(TestServerConfig {
        authorized_key: Some(key.public_key().clone()),
        allow_password_method: false,
        ..Default::default()
    })
    .await
}

fn secret(value: &str, save: bool) -> PromptAnswer {
    PromptAnswer::Secret {
        value: value.into(),
        save,
    }
}

fn is_ready(e: &SessionEvent) -> bool {
    *e == SessionEvent::Status {
        stage: Stage::Ready,
    }
}

async fn ready(sink: &CollectSink) {
    sink.wait_event(is_ready, T).await;
}

fn prompts(sink: &CollectSink) -> Vec<PromptKind> {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            SessionEvent::SecretPrompt { kind, .. } => Some(kind.clone()),
            _ => None,
        })
        .collect()
}

/// `can_save` of every `SecretPrompt` so far.
fn can_saves(sink: &CollectSink) -> Vec<bool> {
    sink.events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|e| match e {
            SessionEvent::SecretPrompt { can_save, .. } => Some(*can_save),
            _ => None,
        })
        .collect()
}

/// Waits for the `n`-th `SecretPrompt` (1-based) and returns its kind.
async fn nth_prompt(sink: &CollectSink, n: usize) -> PromptKind {
    let wait = async {
        loop {
            if let Some(k) = prompts(sink).get(n - 1) {
                return k.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    match tokio::time::timeout(T, wait).await {
        Ok(k) => k,
        Err(_) => panic!("no prompt #{n}; events: {:?}", sink.events.lock().unwrap()),
    }
}

async fn closed(sink: &CollectSink) -> CloseReason {
    match sink
        .wait_event(|e| matches!(e, SessionEvent::Closed { .. }), T)
        .await
    {
        SessionEvent::Closed { reason } => reason,
        _ => unreachable!(),
    }
}

async fn closed_error(sink: &CollectSink) -> ApiError {
    match closed(sink).await {
        CloseReason::Error { error } => error,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn accepted(server: &TestServer) -> Vec<String> {
    server.accepted_auth.lock().unwrap().clone()
}

#[tokio::test]
async fn prompted_password_saved_only_after_success() {
    let server = pw_server().await;
    let env = env();
    let c = conn(&server, password_auth());
    let (id, sink) = env.open(&server, c.clone());

    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret("wrong", true)).unwrap();
    assert_eq!(nth_prompt(&sink, 2).await, PromptKind::Password);
    assert_eq!(env.stored(&c, SecretKind::Password), None);

    env.mgr.answer(id, secret(PW, true)).unwrap();
    ready(&sink).await;
    assert_eq!(env.stored(&c, SecretKind::Password).as_deref(), Some(PW));
}

#[tokio::test]
async fn prompted_password_not_saved_when_save_false() {
    let server = pw_server().await;
    let env = env();
    let c = conn(&server, password_auth());
    let (id, sink) = env.open(&server, c.clone());
    nth_prompt(&sink, 1).await;
    env.mgr.answer(id, secret(PW, false)).unwrap();
    ready(&sink).await;
    assert_eq!(env.stored(&c, SecretKind::Password), None);
}

#[tokio::test]
async fn stale_keychain_password_falls_back_to_prompt() {
    let server = pw_server().await;
    let env = env();
    let c = conn(&server, password_auth());
    env.store(&c, SecretKind::Password, "stale");
    let (id, sink) = env.open(&server, c.clone());

    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret(PW, true)).unwrap();
    ready(&sink).await;
    assert_eq!(prompts(&sink).len(), 1);
    assert_eq!(env.stored(&c, SecretKind::Password).as_deref(), Some(PW));
}

#[tokio::test]
async fn wrong_password_fails_after_three_prompts() {
    let server = pw_server().await;
    let env = env();
    let c = conn(&server, password_auth());
    env.store(&c, SecretKind::Password, "stale");
    let (id, sink) = env.open(&server, c);
    for n in 1..=3 {
        nth_prompt(&sink, n).await;
        env.mgr.answer(id, secret("wrong", true)).unwrap();
    }
    let error = closed_error(&sink).await;
    assert_eq!(error.code, "auth.all_failed");
    assert_eq!(error.params["tried"], "password");
    assert_eq!(prompts(&sink).len(), 3);
}

#[tokio::test]
async fn server_withdrawing_password_fails_without_prompting() {
    let server = TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        single_password_try: true,
        ..Default::default()
    })
    .await;
    let env = env();
    let c = conn(&server, password_auth());
    env.store(&c, SecretKind::Password, "stale");
    let (_, sink) = env.open(&server, c);
    let error = closed_error(&sink).await;
    assert_eq!(error.code, "auth.all_failed");
    assert_eq!(error.params["tried"], "password");
    assert!(prompts(&sink).is_empty());
}

#[tokio::test]
async fn keyboard_interactive_fallback_uses_password() {
    let server = TestServer::start(TestServerConfig {
        allow_password_method: false,
        kbd_interactive: Some(("Password: ".into(), PW.into())),
        ..Default::default()
    })
    .await;
    let env = env();
    let c = conn(&server, password_auth());
    env.store(&c, SecretKind::Password, PW);
    let (_, sink) = env.open(&server, c);
    ready(&sink).await;
    assert!(prompts(&sink).is_empty());
    assert_eq!(accepted(&server), ["keyboard-interactive"]);
}

#[tokio::test]
async fn keyboard_interactive_custom_prompt_asks_user() {
    let server = TestServer::start(TestServerConfig {
        allow_password_method: false,
        kbd_interactive: Some(("OTP: ".into(), "123456".into())),
        kbd_echo: true,
        ..Default::default()
    })
    .await;
    let env = env();
    let (id, sink) = env.open(&server, conn(&server, password_auth()));

    assert_eq!(
        nth_prompt(&sink, 1).await,
        PromptKind::KeyboardInteractive {
            name: "test".into(),
            instructions: String::new(),
            prompts: vec![KbdPrompt {
                text: "OTP: ".into(),
                echo: true,
            }],
        }
    );
    env.mgr
        .answer(
            id,
            PromptAnswer::KeyboardInteractive {
                responses: vec!["123456".into()],
                save: false,
            },
        )
        .unwrap();
    ready(&sink).await;
    assert_eq!(accepted(&server), ["keyboard-interactive"]);
}

#[tokio::test]
async fn keyboard_interactive_password_then_otp_saves_password() {
    let server = TestServer::start(TestServerConfig {
        allow_password_method: false,
        kbd_interactive: Some(("Password: ".into(), PW.into())),
        kbd_otp: Some("123456".into()),
        ..Default::default()
    })
    .await;
    let env = env();
    let c = conn(&server, password_auth());
    let (id, sink) = env.open(&server, c.clone());
    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret(PW, true)).unwrap();
    assert!(matches!(
        nth_prompt(&sink, 2).await,
        PromptKind::KeyboardInteractive { .. }
    ));
    env.mgr
        .answer(
            id,
            PromptAnswer::KeyboardInteractive {
                responses: vec!["123456".into()],
                save: false,
            },
        )
        .unwrap();
    ready(&sink).await;
    // The ticked save of the password round survives the OTP round.
    assert_eq!(env.stored(&c, SecretKind::Password).as_deref(), Some(PW));
}

#[tokio::test]
async fn unencrypted_key_auth() {
    let key = ed25519();
    let server = key_server(&key).await;
    let env = env();
    let path = env.key_file(&key, None);
    let (_, sink) = env.open(&server, conn(&server, key_auth(&path)));
    ready(&sink).await;
    assert!(prompts(&sink).is_empty());
    assert_eq!(accepted(&server), ["publickey"]);
}

#[tokio::test]
async fn encrypted_key_prompts_passphrase_three_attempts() {
    let key = ed25519();
    let server = key_server(&key).await;
    let env = env();
    let path = env.key_file(&key, Some(PASSPHRASE));
    let c = conn(&server, key_auth(&path));
    env.store(&c, SecretKind::Passphrase, "stale");
    let (id, sink) = env.open(&server, c);

    for attempt in 1..=3 {
        assert_eq!(
            nth_prompt(&sink, attempt.into()).await,
            PromptKind::Passphrase { attempt }
        );
        env.mgr.answer(id, secret("wrong", true)).unwrap();
    }
    assert_eq!(closed_error(&sink).await.code, "auth.key_unreadable");
    assert_eq!(prompts(&sink).len(), 3);
    assert!(accepted(&server).is_empty());
}

#[tokio::test]
async fn prompted_passphrase_saved_after_success() {
    let key = ed25519();
    let server = key_server(&key).await;
    let env = env();
    let path = env.key_file(&key, Some(PASSPHRASE));
    let c = conn(&server, key_auth(&path));
    let (id, sink) = env.open(&server, c.clone());

    nth_prompt(&sink, 1).await;
    env.mgr.answer(id, secret("wrong", true)).unwrap();
    assert_eq!(
        nth_prompt(&sink, 2).await,
        PromptKind::Passphrase { attempt: 2 }
    );
    env.mgr.answer(id, secret(PASSPHRASE, true)).unwrap();
    ready(&sink).await;
    assert_eq!(
        env.stored(&c, SecretKind::Passphrase).as_deref(),
        Some(PASSPHRASE)
    );
}

#[tokio::test]
async fn encrypted_key_passphrase_from_keychain() {
    let key = ed25519();
    let server = key_server(&key).await;
    let env = env();
    let path = env.key_file(&key, Some(PASSPHRASE));
    let c = conn(&server, key_auth(&path));
    env.store(&c, SecretKind::Passphrase, PASSPHRASE);
    let (_, sink) = env.open(&server, c);
    ready(&sink).await;
    assert!(prompts(&sink).is_empty());
}

#[tokio::test]
async fn wrong_key_reports_tried_methods() {
    let server = key_server(&ed25519()).await;
    let env = env();
    let path = env.key_file(&ed25519(), None);
    let (_, sink) = env.open(&server, conn(&server, key_auth(&path)));
    let error = closed_error(&sink).await;
    assert_eq!(error.code, "auth.all_failed");
    assert!(error.params["tried"].contains("publickey"), "{error:?}");
}

#[tokio::test]
async fn missing_key_file_is_unreadable() {
    let server = key_server(&ed25519()).await;
    let env = env();
    let path = env.dir.path().join("absent");
    let (_, sink) = env.open(&server, conn(&server, key_auth(&path)));
    assert_eq!(closed_error(&sink).await.code, "auth.key_unreadable");
}

/// Serves an SSH agent holding `keys` on a fresh Unix socket; the dir owns the socket.
#[cfg(unix)]
async fn spawn_agent(keys: &[PrivateKey]) -> (TempDir, PathBuf) {
    use russh::keys::agent::client::AgentClient;
    use tokio::net::UnixListener;

    let dir = TempDir::new().unwrap();
    let sock = dir.path().join("agent.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let incoming = futures::stream::unfold(listener, |l| async move {
        let next = l.accept().await.map(|(s, _)| s);
        Some((next, l))
    });
    tokio::spawn(russh::keys::agent::server::serve(Box::pin(incoming), ()));
    let mut client = AgentClient::connect_uds(&sock).await.unwrap();
    for key in keys {
        client.add_identity(key, &[]).await.unwrap();
    }
    (dir, sock)
}

#[cfg(unix)]
#[tokio::test]
async fn agent_auth_via_uds() {
    let key = ed25519();
    let server = key_server(&key).await;
    // The unauthorized key comes first: every identity is tried.
    let (_agent, sock) = spawn_agent(&[ed25519(), key]).await;
    let env = Env::new(SessionOptions {
        agent_path: Some(sock),
        ..Default::default()
    });
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Agent));
    ready(&sink).await;
    assert_eq!(accepted(&server), ["publickey"]);
}

#[cfg(unix)]
#[tokio::test]
async fn agent_without_authorized_key_fails() {
    let server = key_server(&ed25519()).await;
    let (_agent, sock) = spawn_agent(&[ed25519()]).await;
    let env = Env::new(SessionOptions {
        agent_path: Some(sock),
        ..Default::default()
    });
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Agent));
    let error = closed_error(&sink).await;
    assert_eq!(error.code, "auth.all_failed");
    assert_eq!(error.params["tried"], "publickey");
}

#[tokio::test]
async fn agent_missing_reports_unavailable() {
    let server = key_server(&ed25519()).await;
    let dir = TempDir::new().unwrap();
    let env = Env::new(SessionOptions {
        agent_path: Some(dir.path().join("no-agent.sock")),
        ..Default::default()
    });
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Agent));
    assert_eq!(closed_error(&sink).await.code, "auth.agent_unavailable");
}

#[tokio::test]
async fn close_during_secret_prompt_ends_task() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = env.open(&server, conn(&server, password_auth()));
    nth_prompt(&sink, 1).await;
    env.mgr.close(id).await.unwrap();
    assert_eq!(closed(&sink).await, CloseReason::UserClosed);
    assert_eq!(env.mgr.pending_prompts(), 0);
}

#[tokio::test]
async fn cancel_prompt_closes() {
    let server = pw_server().await;
    let env = env();
    let (id, sink) = env.open(&server, conn(&server, password_auth()));
    nth_prompt(&sink, 1).await;
    env.mgr.answer(id, PromptAnswer::Cancel).unwrap();
    assert_eq!(closed(&sink).await, CloseReason::UserClosed);
    assert_eq!(env.mgr.pending_prompts(), 0);
}

#[tokio::test]
async fn keychain_failure_still_prompts() {
    let server = pw_server().await;
    let env = Env::with_store(
        SessionOptions::default(),
        Some(Arc::new(FailingSecretStore)),
    );
    let (id, sink) = env.open(&server, conn(&server, password_auth()));
    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret(PW, true)).unwrap();
    ready(&sink).await;
}

#[tokio::test]
async fn saved_methods_still_offer_save() {
    let server = pw_server().await;
    let env = env();
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Password { save: true }));
    nth_prompt(&sink, 1).await;
    assert_eq!(can_saves(&sink), [true]);
}

/// `Auto` with `default_keys`, and the agent at `agent` (else none): never the developer's.
fn auto_env(agent: Option<PathBuf>, default_keys: Vec<PathBuf>) -> Env {
    let no_agent = || std::env::temp_dir().join(format!("no-agent-{}", Uuid::new_v4()));
    Env::new(SessionOptions {
        agent_path: Some(agent.unwrap_or_else(no_agent)),
        default_keys,
        ..Default::default()
    })
}

async fn key_and_pw_server(key: &PrivateKey, max_auth_attempts: Option<usize>) -> TestServer {
    TestServer::start(TestServerConfig {
        password: Some(PW.into()),
        authorized_key: Some(key.public_key().clone()),
        max_auth_attempts,
        ..Default::default()
    })
    .await
}

#[cfg(unix)]
#[tokio::test]
async fn auto_uses_the_agent_first() {
    let key = ed25519();
    let server = key_server(&key).await;
    let (_agent, sock) = spawn_agent(std::slice::from_ref(&key)).await;
    let keys = TempDir::new().unwrap();
    // Tried before the agent, it would ask for its passphrase.
    let encrypted = write_key(keys.path(), &key, Some(PASSPHRASE));
    let env = auto_env(Some(sock), vec![encrypted]);
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Auto));
    ready(&sink).await;
    assert!(prompts(&sink).is_empty());
    assert_eq!(accepted(&server), ["publickey"]);
}

#[tokio::test]
async fn auto_falls_back_to_a_default_key() {
    let key = ed25519();
    let server = key_server(&key).await;
    let keys = TempDir::new().unwrap();
    let env = auto_env(None, vec![write_key(keys.path(), &key, None)]);
    let (_, sink) = env.open(&server, conn(&server, AuthMethod::Auto));
    ready(&sink).await;
    assert!(prompts(&sink).is_empty());
    assert_eq!(accepted(&server), ["publickey"]);
}

#[tokio::test]
async fn auto_asks_the_passphrase_without_save() {
    let key = ed25519();
    let server = key_server(&key).await;
    let keys = TempDir::new().unwrap();
    let env = auto_env(None, vec![write_key(keys.path(), &key, Some(PASSPHRASE))]);
    let c = conn(&server, AuthMethod::Auto);
    let (id, sink) = env.open(&server, c.clone());
    assert_eq!(
        nth_prompt(&sink, 1).await,
        PromptKind::Passphrase { attempt: 1 }
    );
    assert_eq!(can_saves(&sink), [false]);
    env.mgr.answer(id, secret(PASSPHRASE, true)).unwrap();
    ready(&sink).await;
    assert_eq!(env.stored(&c, SecretKind::Passphrase), None);
}

#[tokio::test]
async fn auto_ends_with_the_password_prompt() {
    let server = key_and_pw_server(&ed25519(), None).await;
    let keys = TempDir::new().unwrap();
    let env = auto_env(None, vec![write_key(keys.path(), &ed25519(), None)]);
    let c = conn(&server, AuthMethod::Auto);
    let (id, sink) = env.open(&server, c.clone());
    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    assert_eq!(can_saves(&sink), [false]);
    env.mgr.answer(id, secret(PW, true)).unwrap();
    ready(&sink).await;
    assert_eq!(accepted(&server), ["password"]);
    assert_eq!(env.stored(&c, SecretKind::Password), None);
    assert_eq!(env.stored(&c, SecretKind::Passphrase), None);
}

#[tokio::test]
async fn auto_skips_missing_default_keys() {
    let server = pw_server().await;
    let keys = TempDir::new().unwrap();
    let env = auto_env(None, vec![keys.path().join("id_ed25519")]);
    let (id, sink) = env.open(&server, conn(&server, AuthMethod::Auto));
    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret(PW, false)).unwrap();
    ready(&sink).await;
}

#[cfg(unix)]
#[tokio::test]
async fn auto_skips_default_keys_the_agent_offered() {
    let (k1, k2) = (ed25519(), ed25519());
    // The two agent keys use up the failures: a third would end the connection.
    let server = key_and_pw_server(&ed25519(), Some(2)).await;
    let (_agent, sock) = spawn_agent(&[k1.clone(), k2]).await;
    let keys = TempDir::new().unwrap();
    let path = write_key(keys.path(), &k1, None);
    let mut pub_path = path.clone().into_os_string();
    pub_path.push(".pub");
    k1.public_key()
        .write_openssh_file(Path::new(&pub_path))
        .unwrap();
    let env = auto_env(Some(sock), vec![path]);
    let (id, sink) = env.open(&server, conn(&server, AuthMethod::Auto));
    assert_eq!(nth_prompt(&sink, 1).await, PromptKind::Password);
    env.mgr.answer(id, secret(PW, false)).unwrap();
    ready(&sink).await;
}

#[cfg(unix)]
#[tokio::test]
async fn auto_reports_every_method_tried() {
    let server = key_and_pw_server(&ed25519(), None).await;
    let (_agent, sock) = spawn_agent(&[ed25519()]).await;
    let keys = TempDir::new().unwrap();
    let env = auto_env(Some(sock), vec![write_key(keys.path(), &ed25519(), None)]);
    let (id, sink) = env.open(&server, conn(&server, AuthMethod::Auto));
    for n in 1..=3 {
        nth_prompt(&sink, n).await;
        env.mgr.answer(id, secret("wrong", false)).unwrap();
    }
    let error = closed_error(&sink).await;
    assert_eq!(error.code, "auth.all_failed");
    assert_eq!(error.params["tried"], "publickey,password");
}
