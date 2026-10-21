//! User authentication for one hop: password (with keyboard-interactive fallback), key file or agent.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::MethodKind;
use russh::client::{AuthResult, Handle, Handler, KeyboardInteractiveAuthResponse, Prompt};
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg, load_secret_key};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use super::events::{KbdPrompt, PromptAnswer, PromptKind, SessionEvent};
use super::{PromptBroker, SessionId, SessionOptions, SessionSink};
use crate::error::CoreError;
use crate::model::{AuthMethod, Connection, expand_home};
use crate::secrets::{SecretKind, SecretStore};

/// Prompted password attempts per connection, as OpenSSH's `NumberOfPasswordPrompts`.
const MAX_PASSWORD_PROMPTS: u8 = 3;

pub(crate) struct AuthCtx<'a> {
    pub secrets: &'a dyn SecretStore,
    pub prompts: &'a PromptBroker,
    pub session: SessionId,
    pub sink: &'a dyn SessionSink,
    pub opts: &'a SessionOptions,
    /// The hop being authenticated, lowercased; named in every prompt.
    pub host: &'a str,
    pub port: u16,
}

impl AuthCtx<'_> {
    /// The saved secret; a keychain failure reads as none, so the user is asked instead.
    fn saved(&self, conn: Uuid, kind: SecretKind) -> Option<SecretString> {
        self.secrets.get(conn, kind).unwrap_or_else(|e| {
            log::warn!("cannot read the saved {kind:?}: {e}");
            None
        })
    }

    /// Best effort: a keychain failure must not fail a session that already authenticated.
    fn save(&self, conn: Uuid, kind: SecretKind, value: &SecretString) {
        if let Err(e) = self.secrets.set(conn, kind, value) {
            log::warn!("cannot save the {kind:?}: {e}");
        }
    }

    async fn ask(&self, kind: PromptKind) -> PromptAnswer {
        let prompt = SessionEvent::SecretPrompt {
            host: self.host.into(),
            port: self.port,
            kind,
        };
        self.prompts.ask(self.session, self.sink, prompt).await
    }

    /// Returns (secret, save); anything but a `Secret` answer cancels.
    async fn ask_secret(&self, kind: PromptKind) -> Result<(SecretString, bool), CoreError> {
        match self.ask(kind).await {
            PromptAnswer::Secret { value, save } => Ok((value.into(), save)),
            _ => Err(CoreError::Cancelled),
        }
    }
}

/// Authenticates `conn.username` on an already connected hop, using `conn.auth`.
pub(crate) async fn authenticate<H: Handler>(
    handle: &mut Handle<H>,
    conn: &Connection,
    ctx: &AuthCtx<'_>,
) -> Result<(), CoreError> {
    match &conn.auth {
        AuthMethod::Password { .. } => password(handle, conn, ctx).await,
        AuthMethod::Key { path, .. } => key(handle, conn, &expand_home(path), ctx).await,
        AuthMethod::Agent => agent(handle, &conn.username, ctx.opts).await,
    }
}

async fn password<H: Handler>(
    handle: &mut Handle<H>,
    conn: &Connection,
    ctx: &AuthCtx<'_>,
) -> Result<(), CoreError> {
    let user = conn.username.as_str();
    // "none" lists the server's methods, so nobody types a password a server will not take.
    let offered = match handle.authenticate_none(user).await? {
        AuthResult::Success => return Ok(()),
        AuthResult::Failure {
            remaining_methods, ..
        } => remaining_methods,
    };
    let kbd = !offered.contains(&MethodKind::Password);
    if kbd && !offered.contains(&MethodKind::KeyboardInteractive) {
        return Err(CoreError::AuthFailed { tried: vec![] });
    }
    let mut src = Secrets {
        ctx,
        saved: ctx.saved(conn.id, SecretKind::Password),
        prompts: 0,
        method: if kbd {
            "keyboard-interactive"
        } else {
            "password"
        },
        typed: None,
    };
    loop {
        let ok = if kbd {
            keyboard_interactive(handle, user, &mut src).await?
        } else {
            let pw = src.password().await?;
            match handle
                .authenticate_password(user, pw.expose_secret())
                .await?
            {
                AuthResult::Success => true,
                AuthResult::Failure {
                    remaining_methods, ..
                } if remaining_methods.contains(&MethodKind::Password) => false,
                // russh servers withdraw password after one miss; retrying would drop the connection.
                AuthResult::Failure { .. } => return Err(src.failed()),
            }
        };
        if ok {
            if let Some(pw) = &src.typed {
                ctx.save(conn.id, SecretKind::Password, pw);
            }
            return Ok(());
        }
    }
}

/// Secrets for successive password attempts: the saved password once, then at most
/// `MAX_PASSWORD_PROMPTS` prompts.
struct Secrets<'c, 'a> {
    ctx: &'c AuthCtx<'a>,
    saved: Option<SecretString>,
    prompts: u8,
    method: &'static str,
    /// Typed at the latest prompt with `save` ticked: written to the keychain on success.
    typed: Option<SecretString>,
}

impl Secrets<'_, '_> {
    fn failed(&self) -> CoreError {
        CoreError::AuthFailed {
            tried: vec![self.method.into()],
        }
    }

    fn count_prompt(&mut self) -> Result<(), CoreError> {
        if self.prompts == MAX_PASSWORD_PROMPTS {
            return Err(self.failed());
        }
        self.prompts += 1;
        self.typed = None;
        Ok(())
    }

    async fn password(&mut self) -> Result<SecretString, CoreError> {
        if let Some(pw) = self.saved.take() {
            return Ok(pw);
        }
        self.count_prompt()?;
        let (pw, save) = self.ctx.ask_secret(PromptKind::Password).await?;
        self.typed = save.then(|| pw.clone());
        Ok(pw)
    }

    async fn responses(
        &mut self,
        name: String,
        instructions: String,
        prompts: &[Prompt],
    ) -> Result<Vec<String>, CoreError> {
        self.count_prompt()?;
        let prompts = prompts
            .iter()
            .map(|p| KbdPrompt {
                text: p.prompt.clone(),
                echo: p.echo,
            })
            .collect();
        let kind = PromptKind::KeyboardInteractive {
            name,
            instructions,
            prompts,
        };
        match self.ctx.ask(kind).await {
            // One-time codes and the like: never saved.
            PromptAnswer::KeyboardInteractive { responses, .. } => Ok(responses),
            _ => Err(CoreError::Cancelled),
        }
    }
}

/// One keyboard-interactive exchange. A lone hidden prompt gets the password (once);
/// anything else goes to the user.
async fn keyboard_interactive<H: Handler>(
    handle: &mut Handle<H>,
    user: &str,
    src: &mut Secrets<'_, '_>,
) -> Result<bool, CoreError> {
    let mut reply = handle
        .authenticate_keyboard_interactive_start(user, None)
        .await?;
    let mut password_sent = false;
    let mut asked = false;
    loop {
        let (name, instructions, prompts) = match reply {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            // Refused before asking anything, or withdrawn: retrying would only loop.
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods, ..
            } if !asked || !remaining_methods.contains(&MethodKind::KeyboardInteractive) => {
                return Err(src.failed());
            }
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => (name, instructions, prompts),
        };
        let responses = match prompts.as_slice() {
            [] => vec![],
            [p] if !p.echo && !password_sent => {
                password_sent = true;
                asked = true;
                vec![src.password().await?.expose_secret().to_owned()]
            }
            _ => {
                asked = true;
                src.responses(name, instructions, &prompts).await?
            }
        };
        reply = handle
            .authenticate_keyboard_interactive_respond(responses)
            .await?;
    }
}

async fn key<H: Handler>(
    handle: &mut Handle<H>,
    conn: &Connection,
    path: &Path,
    ctx: &AuthCtx<'_>,
) -> Result<(), CoreError> {
    let unreadable = |reason: String| CoreError::KeyUnreadable {
        path: path.display().to_string(),
        reason,
    };
    let (key, typed) = match load_key(path, None).await {
        Ok(key) => (key, None),
        Err(russh::keys::Error::KeyIsEncrypted) => unlock(path, conn.id, ctx)
            .await?
            .ok_or_else(|| unreadable("wrong passphrase".into()))?,
        Err(e) => return Err(unreadable(e.to_string())),
    };
    let hash = rsa_hash(handle, key.algorithm()).await?;
    let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
    if !handle
        .authenticate_publickey(&conn.username, key)
        .await?
        .success()
    {
        return Err(CoreError::AuthFailed {
            tried: vec!["publickey".into()],
        });
    }
    if let Some(passphrase) = typed {
        ctx.save(conn.id, SecretKind::Passphrase, &passphrase);
    }
    Ok(())
}

/// Decrypts the key with the saved passphrase, else up to `max_passphrase_attempts` prompts.
/// Returns the key and the typed passphrase when the user asked to save it; `None` when
/// every attempt failed.
async fn unlock(
    path: &Path,
    conn: Uuid,
    ctx: &AuthCtx<'_>,
) -> Result<Option<(PrivateKey, Option<SecretString>)>, CoreError> {
    if let Some(passphrase) = ctx.saved(conn, SecretKind::Passphrase)
        && let Ok(key) = load_key(path, Some(passphrase)).await
    {
        return Ok(Some((key, None)));
    }
    for attempt in 1..=ctx.opts.max_passphrase_attempts {
        let (passphrase, save) = ctx.ask_secret(PromptKind::Passphrase { attempt }).await?;
        // The file already parsed as an encrypted key, so a failure here is the passphrase.
        if let Ok(key) = load_key(path, Some(passphrase.clone())).await {
            return Ok(Some((key, save.then_some(passphrase))));
        }
    }
    Ok(None)
}

/// File read and bcrypt run off the async workers.
async fn load_key(
    path: &Path,
    passphrase: Option<SecretString>,
) -> Result<PrivateKey, russh::keys::Error> {
    let path: PathBuf = path.into();
    tokio::task::spawn_blocking(move || {
        load_secret_key(path, passphrase.as_ref().map(|p| p.expose_secret()))
    })
    .await
    .unwrap_or(Err(russh::keys::Error::CouldNotReadKey))
}

/// The server's preferred RSA signature hash; `None` for other key types.
async fn rsa_hash<H: Handler>(
    handle: &Handle<H>,
    algo: russh::keys::Algorithm,
) -> Result<Option<HashAlg>, CoreError> {
    if !algo.is_rsa() {
        return Ok(None);
    }
    Ok(handle.best_supported_rsa_hash().await?.flatten())
}

async fn agent<H: Handler>(
    handle: &mut Handle<H>,
    user: &str,
    opts: &SessionOptions,
) -> Result<(), CoreError> {
    let mut client = connect_agent(opts).await?;
    let identities = client
        .request_identities()
        .await
        .map_err(agent_unavailable)?;
    for identity in identities {
        let key = identity.public_key().into_owned();
        let hash = rsa_hash(handle, key.algorithm()).await?;
        let result = handle
            .authenticate_publickey_with(user, key, hash, &mut client)
            .await
            .map_err(|e| CoreError::Ssh(e.to_string()))?;
        if result.success() {
            return Ok(());
        }
    }
    Err(CoreError::AuthFailed {
        tried: vec!["publickey".into()],
    })
}

type DynAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

fn agent_unavailable(e: russh::keys::Error) -> CoreError {
    CoreError::AgentUnavailable {
        reason: e.to_string(),
    }
}

/// `opts.agent_path`, else `SSH_AUTH_SOCK`.
#[cfg(unix)]
async fn connect_agent(opts: &SessionOptions) -> Result<DynAgent, CoreError> {
    let client = match &opts.agent_path {
        Some(path) => AgentClient::connect_uds(path).await,
        None => AgentClient::connect_env().await,
    };
    client.map(AgentClient::dynamic).map_err(agent_unavailable)
}

/// The OpenSSH agent's named pipe (or `opts.agent_path`), else Pageant.
#[cfg(windows)]
async fn connect_agent(opts: &SessionOptions) -> Result<DynAgent, CoreError> {
    let pipe = match &opts.agent_path {
        Some(path) => path.as_os_str(),
        None => std::ffi::OsStr::new(r"\\.\pipe\openssh-ssh-agent"),
    };
    match AgentClient::connect_named_pipe(pipe).await {
        Ok(client) => Ok(client.dynamic()),
        Err(e) => {
            log::debug!("OpenSSH agent pipe unavailable ({e}), trying Pageant");
            AgentClient::connect_pageant()
                .await
                .map(AgentClient::dynamic)
                .map_err(agent_unavailable)
        }
    }
}
