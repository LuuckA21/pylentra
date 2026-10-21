use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use ts_rs::TS;
use uuid::Uuid;

pub const MAX_JUMP_DEPTH: usize = 5;

fn default_port() -> u16 {
    22
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub group_id: Option<Uuid>,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    #[serde(default)]
    pub jump_via: Option<Uuid>,
    #[serde(default)]
    pub forwards: Vec<ForwardRule>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum AuthMethod {
    Password {
        save: bool,
    },
    Key {
        path: PathBuf,
        save_passphrase: bool,
    },
    Agent,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum ForwardRule {
    Local {
        bind_host: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
        auto_start: bool,
    },
    Remote {
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
        auto_start: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, TS)]
#[ts(export)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    pub theme: Theme,
    pub font_family: String,
    pub font_size: u16,
    pub language: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            font_family: "monospace".into(),
            font_size: 14,
            language: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum Warning {
    KeyFileMissing { path: PathBuf },
}

/// Resolves a leading `~` (as in `~/.ssh/id_ed25519`) to the home directory.
/// Other paths, `~user/...` included, come back unchanged.
pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

fn invalid(field: impl Into<String>, reason: &str) -> CoreError {
    CoreError::InvalidInput {
        field: field.into(),
        reason: reason.into(),
    }
}

fn require_nonzero(port: u16, field: String) -> Result<(), CoreError> {
    if port == 0 {
        return Err(invalid(field, "port must be between 1 and 65535"));
    }
    Ok(())
}

impl Connection {
    /// Checks the fields this connection owns. Jump references and chain depth
    /// need the whole store, so they are checked there.
    pub fn validate(&self) -> Result<Vec<Warning>, CoreError> {
        for (field, value) in [
            ("name", &self.name),
            ("host", &self.host),
            ("username", &self.username),
        ] {
            if value.trim().is_empty() {
                return Err(invalid(field, "must not be empty"));
            }
        }
        require_nonzero(self.port, "port".into())?;

        for (i, rule) in self.forwards.iter().enumerate() {
            match rule {
                ForwardRule::Local {
                    bind_host,
                    bind_port,
                    dest_port,
                    ..
                } => {
                    if bind_host.parse::<IpAddr>().is_err() {
                        return Err(invalid(
                            format!("forwards[{i}].bind_host"),
                            "must be an IP address",
                        ));
                    }
                    require_nonzero(*bind_port, format!("forwards[{i}].bind_port"))?;
                    require_nonzero(*dest_port, format!("forwards[{i}].dest_port"))?;
                }
                ForwardRule::Remote {
                    bind_port,
                    dest_port,
                    ..
                } => {
                    require_nonzero(*bind_port, format!("forwards[{i}].bind_port"))?;
                    require_nonzero(*dest_port, format!("forwards[{i}].dest_port"))?;
                }
            }
        }

        let mut warnings = Vec::new();
        if let AuthMethod::Key { path, .. } = &self.auth {
            // The key may live on a removable disk, so a missing file only warns.
            if !expand_home(path).exists() {
                warnings.push(Warning::KeyFileMissing { path: path.clone() });
            }
        }
        Ok(warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn conn() -> Connection {
        Connection {
            id: Uuid::new_v4(),
            name: "web".into(),
            group_id: None,
            host: "example.com".into(),
            port: 22,
            username: "luca".into(),
            auth: AuthMethod::Password { save: false },
            jump_via: None,
            forwards: vec![],
        }
    }

    fn local(bind_host: &str, bind_port: u16, dest_port: u16) -> ForwardRule {
        ForwardRule::Local {
            bind_host: bind_host.into(),
            bind_port,
            dest_host: "db".into(),
            dest_port,
            auto_start: false,
        }
    }

    fn invalid_field(c: &Connection) -> String {
        match c.validate() {
            Err(CoreError::InvalidInput { field, .. }) => field,
            other => format!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn valid_connection_has_no_warnings_when_key_exists() {
        assert_eq!(conn().validate().unwrap(), vec![]);
        let mut c = conn();
        c.auth = AuthMethod::Key {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            save_passphrase: false,
        };
        assert_eq!(c.validate().unwrap(), vec![]);
    }

    #[test]
    fn empty_name_host_or_username_rejected() {
        for field in ["name", "host", "username"] {
            let mut c = conn();
            match field {
                "name" => c.name = "  ".into(),
                "host" => c.host = String::new(),
                _ => c.username = " \t".into(),
            }
            assert_eq!(invalid_field(&c), field);
        }
    }

    #[test]
    fn port_zero_rejected() {
        let mut c = conn();
        c.port = 0;
        assert_eq!(invalid_field(&c), "port");
    }

    #[test]
    fn missing_key_file_is_warning_not_error() {
        let mut c = conn();
        let path = PathBuf::from("/nonexistent/id_ed25519");
        c.auth = AuthMethod::Key {
            path: path.clone(),
            save_passphrase: false,
        };
        assert_eq!(
            c.validate().unwrap(),
            vec![Warning::KeyFileMissing { path }]
        );
    }

    #[test]
    fn expand_home_resolves_leading_tilde_only() {
        let home = std::env::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/.ssh/id_ed25519")),
            home.join(".ssh/id_ed25519")
        );
        for unchanged in ["/etc/key", "relative/key", "~other/key", "a/~/key"] {
            assert_eq!(expand_home(Path::new(unchanged)), PathBuf::from(unchanged));
        }
    }

    #[test]
    fn key_path_with_tilde_is_expanded_before_the_missing_check() {
        let mut c = conn();
        // `~` alone is the home directory, which exists.
        c.auth = AuthMethod::Key {
            path: PathBuf::from("~"),
            save_passphrase: false,
        };
        assert_eq!(c.validate().unwrap(), vec![]);
    }

    #[test]
    fn local_forward_bind_host_must_be_ip() {
        let mut c = conn();
        c.forwards = vec![local("localhost", 8080, 80)];
        assert_eq!(invalid_field(&c), "forwards[0].bind_host");
        for ok in ["127.0.0.1", "::1"] {
            c.forwards = vec![local(ok, 8080, 80)];
            assert_eq!(c.validate().unwrap(), vec![]);
        }
    }

    #[test]
    fn forward_ports_nonzero() {
        let mut c = conn();
        c.forwards = vec![local("127.0.0.1", 0, 80)];
        assert_eq!(invalid_field(&c), "forwards[0].bind_port");
        c.forwards = vec![local("127.0.0.1", 8080, 0)];
        assert_eq!(invalid_field(&c), "forwards[0].dest_port");
        let remote = |bind_port, dest_port| ForwardRule::Remote {
            bind_port,
            dest_host: "x".into(),
            dest_port,
            auto_start: true,
        };
        c.forwards = vec![local("::1", 1, 1), remote(0, 80)];
        assert_eq!(invalid_field(&c), "forwards[1].bind_port");
        c.forwards = vec![remote(9000, 0)];
        assert_eq!(invalid_field(&c), "forwards[0].dest_port");
    }

    #[test]
    fn toml_roundtrip_connection() {
        let mut c = conn();
        c.group_id = Some(Uuid::new_v4());
        c.jump_via = Some(Uuid::new_v4());
        c.auth = AuthMethod::Key {
            path: PathBuf::from("/home/luca/.ssh/id_ed25519"),
            save_passphrase: true,
        };
        c.forwards = vec![
            local("127.0.0.1", 8080, 80),
            ForwardRule::Remote {
                bind_port: 9000,
                dest_host: "localhost".into(),
                dest_port: 3000,
                auto_start: true,
            },
        ];
        let text = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<Connection>(&text).unwrap(), c);
    }

    #[test]
    fn hand_written_toml_may_omit_optional_fields() {
        let c: Connection = toml::from_str(&format!(
            "id = \"{}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n",
            Uuid::new_v4()
        ))
        .unwrap();
        assert_eq!(c.port, 22);
        assert!(c.forwards.is_empty() && c.group_id.is_none() && c.jump_via.is_none());
    }

    #[test]
    fn settings_defaults() {
        let s = Settings::default();
        assert_eq!(
            (s.theme, s.font_family.as_str(), s.font_size, s.language),
            (Theme::System, "monospace", 14, None)
        );
    }
}
