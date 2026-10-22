use crate::error::CoreError;
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

pub const KEYCHAIN_SERVICE: &str = "me.luucka.pylentra";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SecretKind {
    Password,
    Passphrase,
}

pub trait SecretStore: Send + Sync {
    /// A missing secret is `Ok(None)`.
    fn get(&self, conn: Uuid, kind: SecretKind) -> Result<Option<SecretString>, CoreError>;
    fn set(&self, conn: Uuid, kind: SecretKind, value: &SecretString) -> Result<(), CoreError>;
    /// Deleting a missing secret is `Ok(())`.
    fn delete(&self, conn: Uuid, kind: SecretKind) -> Result<(), CoreError>;
}

/// Keychain account name: `<uuid>/password` or `<uuid>/passphrase`.
pub fn account(conn: Uuid, kind: SecretKind) -> String {
    let suffix = match kind {
        SecretKind::Password => "password",
        SecretKind::Passphrase => "passphrase",
    };
    format!("{conn}/{suffix}")
}

/// Maps a keyring failure to `SecretUnavailable`. `NoEntry` is not an error
/// for callers: `KeyringStore` handles it before reaching this function.
fn map_keyring_err(e: keyring::Error) -> CoreError {
    CoreError::SecretUnavailable {
        reason: e.to_string(),
    }
}

pub struct KeyringStore;

impl KeyringStore {
    fn entry(conn: Uuid, kind: SecretKind) -> Result<keyring::Entry, CoreError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, &account(conn, kind)).map_err(map_keyring_err)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, conn: Uuid, kind: SecretKind) -> Result<Option<SecretString>, CoreError> {
        match Self::entry(conn, kind)?.get_password() {
            Ok(v) => Ok(Some(v.into())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn set(&self, conn: Uuid, kind: SecretKind, value: &SecretString) -> Result<(), CoreError> {
        Self::entry(conn, kind)?
            .set_password(value.expose_secret())
            .map_err(map_keyring_err)
    }

    fn delete(&self, conn: Uuid, kind: SecretKind) -> Result<(), CoreError> {
        match Self::entry(conn, kind)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
pub use test_util::{FailingSecretStore, MemorySecretStore};

#[cfg(any(test, feature = "test-util"))]
mod test_util {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    pub struct MemorySecretStore {
        map: Mutex<HashMap<(Uuid, SecretKind), String>>,
    }

    impl MemorySecretStore {
        fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(Uuid, SecretKind), String>> {
            // A poisoned map is still a valid map.
            self.map.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    impl SecretStore for MemorySecretStore {
        fn get(&self, conn: Uuid, kind: SecretKind) -> Result<Option<SecretString>, CoreError> {
            Ok(self.lock().get(&(conn, kind)).cloned().map(Into::into))
        }

        fn set(&self, conn: Uuid, kind: SecretKind, value: &SecretString) -> Result<(), CoreError> {
            self.lock()
                .insert((conn, kind), value.expose_secret().to_owned());
            Ok(())
        }

        fn delete(&self, conn: Uuid, kind: SecretKind) -> Result<(), CoreError> {
            self.lock().remove(&(conn, kind));
            Ok(())
        }
    }

    /// Every call fails with `SecretUnavailable`.
    pub struct FailingSecretStore;

    fn unavailable() -> CoreError {
        CoreError::SecretUnavailable {
            reason: "failing test store".into(),
        }
    }

    impl SecretStore for FailingSecretStore {
        fn get(&self, _: Uuid, _: SecretKind) -> Result<Option<SecretString>, CoreError> {
            Err(unavailable())
        }
        fn set(&self, _: Uuid, _: SecretKind, _: &SecretString) -> Result<(), CoreError> {
            Err(unavailable())
        }
        fn delete(&self, _: Uuid, _: SecretKind) -> Result<(), CoreError> {
            Err(unavailable())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_format() {
        let id = Uuid::new_v4();
        assert_eq!(account(id, SecretKind::Password), format!("{id}/password"));
        assert_eq!(
            account(id, SecretKind::Passphrase),
            format!("{id}/passphrase")
        );
    }

    #[test]
    fn memory_store_set_get_delete() {
        let s = MemorySecretStore::default();
        let id = Uuid::new_v4();
        assert!(s.get(id, SecretKind::Password).unwrap().is_none());
        s.set(id, SecretKind::Password, &"pw".to_string().into())
            .unwrap();
        let got = s.get(id, SecretKind::Password).unwrap().unwrap();
        assert_eq!(got.expose_secret(), "pw");
        assert!(s.get(id, SecretKind::Passphrase).unwrap().is_none());
        s.delete(id, SecretKind::Password).unwrap();
        s.delete(id, SecretKind::Password).unwrap();
        assert!(s.get(id, SecretKind::Password).unwrap().is_none());
    }

    #[test]
    fn failing_store_always_fails() {
        let id = Uuid::new_v4();
        let e = FailingSecretStore
            .get(id, SecretKind::Password)
            .unwrap_err();
        assert_eq!(e.code(), "secret.unavailable");
    }

    #[test]
    #[ignore = "needs a real OS keychain"]
    fn keyring_roundtrip() {
        let s = KeyringStore;
        let id = Uuid::new_v4();
        s.set(id, SecretKind::Password, &"pw".to_string().into())
            .unwrap();
        let got = s.get(id, SecretKind::Password).unwrap().unwrap();
        assert_eq!(got.expose_secret(), "pw");
        s.delete(id, SecretKind::Password).unwrap();
        assert!(s.get(id, SecretKind::Password).unwrap().is_none());
        s.delete(id, SecretKind::Password).unwrap();
    }

    #[test]
    fn keyring_errors_map_to_secret_unavailable() {
        let e = map_keyring_err(keyring::Error::NoDefaultStore);
        assert!(
            matches!(e, CoreError::SecretUnavailable { ref reason } if *reason == keyring::Error::NoDefaultStore.to_string())
        );
    }
}
