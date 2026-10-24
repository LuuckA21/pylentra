use crate::error::CoreError;
use serde::de::{DeserializeOwned, IntoDeserializer};
use serde::{Deserialize, Deserializer, Serialize};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ts_rs::TS;
use uuid::Uuid;

pub const MAX_JUMP_DEPTH: usize = 5;

fn default_port() -> u16 {
    22
}

/// Reads an optional value, turning anything `T` does not recognise into `None`
/// so a newer or hand-edited file never makes the whole library unreadable.
pub(crate) fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    d: D,
) -> Result<Option<T>, D::Error> {
    let text = Option::<String>::deserialize(d)?;
    Ok(text.and_then(|s| {
        T::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(s)).ok()
    }))
}

/// Like [`lenient`], falling back to `T::default()`.
pub(crate) fn lenient_or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    d: D,
) -> Result<T, D::Error> {
    Ok(lenient(d)?.unwrap_or_default())
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TagColor {
    Red,
    Orange,
    Amber,
    Green,
    Teal,
    Blue,
    Violet,
    Gray,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConnIcon {
    Server,
    Database,
    Web,
    Nas,
    Raspberry,
    Cloud,
    Container,
    Terminal,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient"
    )]
    #[ts(optional)]
    pub color: Option<TagColor>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient"
    )]
    #[ts(optional)]
    pub icon: Option<ConnIcon>,
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
    /// The agent, then `SessionOptions::default_keys`, then password; nothing is saved.
    Auto,
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient"
    )]
    #[ts(optional)]
    pub color: Option<TagColor>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, TS)]
#[ts(export)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TerminalTheme {
    #[default]
    App,
    Dracula,
    Nord,
    SolarizedDark,
    SolarizedLight,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    pub theme: Theme,
    pub font_family: String,
    pub font_size: u16,
    pub language: Option<String>,
    #[serde(deserialize_with = "lenient_or_default")]
    pub terminal_theme: TerminalTheme,
    #[serde(deserialize_with = "lenient_or_default")]
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub scrollback: u32,
    pub copy_on_select: bool,
    pub default_user: Option<String>,
    pub default_port: u16,
    pub connect_timeout_secs: u16,
    /// 0 turns the keepalive off.
    pub keepalive_secs: u16,
    pub auto_reconnect: bool,
    pub confirm_close_active: bool,
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            font_family: "monospace".into(),
            font_size: 14,
            language: None,
            terminal_theme: TerminalTheme::App,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 10_000,
            copy_on_select: false,
            default_user: None,
            default_port: 22,
            connect_timeout_secs: 15,
            keepalive_secs: 30,
            auto_reconnect: false,
            confirm_close_active: true,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), CoreError> {
        fn range(field: &str, v: u32, lo: u32, hi: u32) -> Result<(), CoreError> {
            if (lo..=hi).contains(&v) {
                return Ok(());
            }
            Err(invalid(field, &format!("must be between {lo} and {hi}")))
        }
        range("font_size", self.font_size.into(), 8, 32)?;
        range("scrollback", self.scrollback, 1_000, 100_000)?;
        range("default_port", self.default_port.into(), 1, 65_535)?;
        range(
            "connect_timeout_secs",
            self.connect_timeout_secs.into(),
            5,
            120,
        )?;
        if self.keepalive_secs != 0 {
            range("keepalive_secs", self.keepalive_secs.into(), 5, 300)?;
        }
        if self
            .default_user
            .as_deref()
            .is_some_and(|u| u.trim().is_empty())
        {
            return Err(invalid("default_user", "must not be empty"));
        }
        Ok(())
    }

    /// Connect timeout and keepalive interval (`None` when off) for new sessions,
    /// clamped to the valid ranges: a hand-edited file is loaded without `validate`.
    pub fn session_timeouts(&self) -> (Duration, Option<Duration>) {
        let secs = |v: u16, lo, hi| Duration::from_secs(v.clamp(lo, hi).into());
        (
            secs(self.connect_timeout_secs, 5, 120),
            (self.keepalive_secs != 0).then(|| secs(self.keepalive_secs, 5, 300)),
        )
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

impl ForwardRule {
    /// Reports fields as `<prefix>.<field>`, e.g. `forwards[0].bind_port`.
    pub fn validate(&self, prefix: &str) -> Result<(), CoreError> {
        let (bind_port, dest_port) = match self {
            Self::Local {
                bind_host,
                bind_port,
                dest_port,
                ..
            } => {
                if bind_host.parse::<IpAddr>().is_err() {
                    return Err(invalid(
                        format!("{prefix}.bind_host"),
                        "must be an IP address",
                    ));
                }
                (bind_port, dest_port)
            }
            Self::Remote {
                bind_port,
                dest_port,
                ..
            } => (bind_port, dest_port),
        };
        require_nonzero(*bind_port, format!("{prefix}.bind_port"))?;
        require_nonzero(*dest_port, format!("{prefix}.dest_port"))
    }
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
            rule.validate(&format!("forwards[{i}]"))?;
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

    #[derive(Deserialize, Debug, PartialEq)]
    struct Probe {
        #[serde(default, deserialize_with = "lenient")]
        color: Option<TagColor>,
        #[serde(default, deserialize_with = "lenient_or_default")]
        icon: ConnIcon2,
    }

    #[derive(Deserialize, Debug, Default, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum ConnIcon2 {
        #[default]
        A,
        B,
    }

    #[test]
    fn lenient_falls_back_on_unknown_or_missing() {
        let p: Probe = toml::from_str("color = 'teal'\nicon = 'b'").unwrap();
        assert_eq!((p.color, p.icon), (Some(TagColor::Teal), ConnIcon2::B));
        let p: Probe = toml::from_str("color = 'pink'\nicon = 'z'").unwrap();
        assert_eq!((p.color, p.icon), (None, ConnIcon2::A));
        let p: Probe = toml::from_str("").unwrap();
        assert_eq!((p.color, p.icon), (None, ConnIcon2::A));
    }

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
            color: None,
            icon: None,
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

    #[test]
    fn v0_1_settings_file_loads_with_new_defaults() {
        let s: Settings =
            toml::from_str("theme = \"Dark\"\nfont_family = \"x\"\nfont_size = 12").unwrap();
        assert_eq!(
            s,
            Settings {
                theme: Theme::Dark,
                font_family: "x".into(),
                font_size: 12,
                ..Settings::default()
            }
        );
        assert_eq!((s.scrollback, s.check_updates), (10_000, true));
    }

    #[test]
    fn unknown_terminal_theme_and_cursor_fall_back() {
        let s: Settings =
            toml::from_str("terminal_theme = \"matrix\"\ncursor_style = \"beam\"").unwrap();
        assert_eq!(
            (s.terminal_theme, s.cursor_style),
            (TerminalTheme::App, CursorStyle::Block)
        );
        let s: Settings =
            toml::from_str("terminal_theme = \"solarized_dark\"\ncursor_style = \"bar\"").unwrap();
        assert_eq!(
            (s.terminal_theme, s.cursor_style),
            (TerminalTheme::SolarizedDark, CursorStyle::Bar)
        );
    }

    #[test]
    fn validate_ranges() {
        type Set = fn(&mut Settings, u32);
        // default_port's upper bound is the u16 type itself, so only its lower edge is probed.
        let cases: [(&str, u32, u32, Set); 5] = [
            ("font_size", 8, 32, |s, v| s.font_size = v as u16),
            ("scrollback", 1_000, 100_000, |s, v| s.scrollback = v),
            ("default_port", 1, 65_535, |s, v| s.default_port = v as u16),
            ("connect_timeout_secs", 5, 120, |s, v| {
                s.connect_timeout_secs = v as u16
            }),
            ("keepalive_secs", 5, 300, |s, v| s.keepalive_secs = v as u16),
        ];
        let field_of = |s: &Settings| match s.validate() {
            Err(CoreError::InvalidInput { field, .. }) => Some(field),
            Err(e) => panic!("{e:?}"),
            Ok(()) => None,
        };
        assert_eq!(field_of(&Settings::default()), None);
        for (field, lo, hi, set) in cases {
            let mut s = Settings::default();
            set(&mut s, lo);
            assert_eq!(field_of(&s), None, "{field} low edge");
            set(&mut s, hi);
            assert_eq!(field_of(&s), None, "{field} high edge");
            // keepalive 0 means off, so only the others reject lo - 1.
            if field != "keepalive_secs" {
                set(&mut s, lo - 1);
                assert_eq!(field_of(&s).as_deref(), Some(field), "{field} below");
            }
            if field != "default_port" {
                set(&mut s, hi + 1);
                assert_eq!(field_of(&s).as_deref(), Some(field), "{field} above");
            }
        }
        let with_keepalive = |keepalive_secs| Settings {
            keepalive_secs,
            ..Settings::default()
        };
        assert_eq!(field_of(&with_keepalive(0)), None);
        assert_eq!(
            field_of(&with_keepalive(4)).as_deref(),
            Some("keepalive_secs")
        );
        let with_user = |u: &str| Settings {
            default_user: Some(u.into()),
            ..Settings::default()
        };
        assert_eq!(field_of(&with_user("  ")).as_deref(), Some("default_user"));
        assert_eq!(field_of(&with_user("luca")), None);
    }

    #[test]
    fn keepalive_zero_disables_keepalive() {
        let mut s = Settings::default();
        assert_eq!(
            s.session_timeouts(),
            (Duration::from_secs(15), Some(Duration::from_secs(30)))
        );
        s.keepalive_secs = 0;
        assert_eq!(s.session_timeouts(), (Duration::from_secs(15), None));
    }

    #[test]
    fn session_timeouts_clamp_hand_edited_values() {
        let s = |connect, keepalive| Settings {
            connect_timeout_secs: connect,
            keepalive_secs: keepalive,
            ..Settings::default()
        };
        let secs = Duration::from_secs;
        assert_eq!(s(0, 1).session_timeouts(), (secs(5), Some(secs(5))));
        assert_eq!(
            s(1_000, 1_000).session_timeouts(),
            (secs(120), Some(secs(300)))
        );
        assert_eq!(s(0, 0).session_timeouts(), (secs(5), None));
    }
}
