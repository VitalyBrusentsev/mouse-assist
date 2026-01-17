use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const APP_NAME: &str = "mouse-assist";
pub const CONFIG_FILE_NAME: &str = "config.toml";

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to determine config directory")]
    NoConfigDir,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Config {
    #[serde(default)]
    pub device_by_path: Option<String>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device_by_path: None,
            bindings: vec![
                Binding {
                    button: MouseButton::BtnSide,
                    action: Action::KeyCombo {
                        keys: vec!["KEY_BACK".into()],
                    },
                },
                Binding {
                    button: MouseButton::BtnExtra,
                    action: Action::KeyCombo {
                        keys: vec!["KEY_FORWARD".into()],
                    },
                },
                Binding {
                    button: MouseButton::BtnForward,
                    action: Action::KeyCombo {
                        keys: vec!["KEY_VOLUMEUP".into()],
                    },
                },
                Binding {
                    button: MouseButton::BtnBack,
                    action: Action::KeyCombo {
                        keys: vec!["KEY_VOLUMEDOWN".into()],
                    },
                },
            ],
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub button: MouseButton,
    pub action: Action,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MouseButton {
    BtnLeft,
    BtnRight,
    BtnMiddle,
    BtnSide,
    BtnExtra,
    BtnForward,
    BtnBack,
    BtnTask,
    WheelTiltLeft,
    WheelTiltRight,
}

impl MouseButton {
    pub fn linux_key_code(self) -> Option<u16> {
        match self {
            Self::BtnLeft => Some(0x110),
            Self::BtnRight => Some(0x111),
            Self::BtnMiddle => Some(0x112),
            Self::BtnSide => Some(0x113),
            Self::BtnExtra => Some(0x114),
            Self::BtnForward => Some(0x115),
            Self::BtnBack => Some(0x116),
            Self::BtnTask => Some(0x117),
            Self::WheelTiltLeft | Self::WheelTiltRight => None,
        }
    }

    pub fn x11_button_number(self) -> Option<u32> {
        match self {
            Self::BtnLeft => Some(1),
            Self::BtnMiddle => Some(2),
            Self::BtnRight => Some(3),
            Self::BtnSide | Self::BtnBack => Some(8),
            Self::BtnExtra | Self::BtnForward => Some(9),
            Self::BtnTask => None,
            Self::WheelTiltLeft => Some(6),
            Self::WheelTiltRight => Some(7),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Command { argv: Vec<String> },
    KeyCombo { keys: Vec<String> },
}

pub fn default_config_path() -> Result<PathBuf, ConfigError> {
    let dirs = BaseDirs::new().ok_or(ConfigError::NoConfigDir)?;
    Ok(dirs.config_dir().join(APP_NAME).join(CONFIG_FILE_NAME))
}

pub fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let raw = fs::read_to_string(path)?;
    Ok(toml::from_str(&raw)?)
}

pub fn save_config(path: &Path, config: &Config) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = config_to_toml_string(config)?;
    fs::write(path, raw)?;
    Ok(())
}

fn config_to_toml_string(config: &Config) -> Result<String, ConfigError> {
    fn toml_string(value: &str) -> String {
        toml::Value::String(value.to_owned()).to_string()
    }

    fn toml_array_of_strings(values: &[String]) -> String {
        toml::Value::Array(values.iter().cloned().map(toml::Value::String).collect()).to_string()
    }

    fn action_inline(action: &Action) -> String {
        match action {
            Action::Command { argv } => format!(
                "{{ type = {}, argv = {} }}",
                toml_string("command"),
                toml_array_of_strings(argv)
            ),
            Action::KeyCombo { keys } => format!(
                "{{ type = {}, keys = {} }}",
                toml_string("key_combo"),
                toml_array_of_strings(keys)
            ),
        }
    }

    let mut out = String::new();

    if let Some(device_by_path) = &config.device_by_path {
        out.push_str("device_by_path = ");
        out.push_str(&toml_string(device_by_path));
        out.push('\n');
        out.push('\n');
    }

    for (idx, binding) in config.bindings.iter().enumerate() {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str("[[bindings]]\n");

        let button = toml::Value::try_from(&binding.button)?;
        out.push_str("button = ");
        out.push_str(&button.to_string());
        out.push('\n');

        out.push_str("action = ");
        out.push_str(&action_inline(&binding.action));
        out.push('\n');
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_toml() {
        let cfg = Config::default();
        let raw = config_to_toml_string(&cfg).unwrap();
        let decoded: Config = toml::from_str(&raw).unwrap();
        assert_eq!(decoded, cfg);
    }

    #[test]
    fn config_parses_inline_action_table() {
        let raw = r#"
[[bindings]]
button = "BTN_SIDE"
action = { type = "key_combo", keys = ["KEY_BACK"] }
"#;
        let decoded: Config = toml::from_str(raw).unwrap();
        assert_eq!(decoded.bindings.len(), 1);
        assert_eq!(decoded.bindings[0].button, MouseButton::BtnSide);
        assert_eq!(
            decoded.bindings[0].action,
            Action::KeyCombo {
                keys: vec!["KEY_BACK".into()]
            }
        );
    }

    #[test]
    fn config_parses_expanded_action_subtable() {
        let raw = r#"
[[bindings]]
button = "BTN_SIDE"

[bindings.action]
type = "key_combo"
keys = ["KEY_BACK"]
"#;
        let decoded: Config = toml::from_str(raw).unwrap();
        assert_eq!(decoded.bindings.len(), 1);
        assert_eq!(decoded.bindings[0].button, MouseButton::BtnSide);
        assert_eq!(
            decoded.bindings[0].action,
            Action::KeyCombo {
                keys: vec!["KEY_BACK".into()]
            }
        );
    }

    #[test]
    fn config_serializes_actions_inline() {
        let cfg = Config {
            device_by_path: None,
            bindings: vec![Binding {
                button: MouseButton::WheelTiltRight,
                action: Action::KeyCombo {
                    keys: vec!["KEY_FORWARD".into()],
                },
            }],
        };
        let raw = config_to_toml_string(&cfg).unwrap();
        assert!(raw.contains("action = {"));
        assert!(!raw.contains("[bindings.action]"));
        let decoded: Config = toml::from_str(&raw).unwrap();
        assert_eq!(decoded, cfg);
    }
}
