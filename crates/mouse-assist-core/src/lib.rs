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
}

impl MouseButton {
    pub fn linux_input_code(self) -> u16 {
        match self {
            Self::BtnLeft => 0x110,
            Self::BtnRight => 0x111,
            Self::BtnMiddle => 0x112,
            Self::BtnSide => 0x113,
            Self::BtnExtra => 0x114,
            Self::BtnForward => 0x115,
            Self::BtnBack => 0x116,
            Self::BtnTask => 0x117,
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
    let raw = toml::to_string_pretty(config)?;
    fs::write(path, raw)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip_toml() {
        let cfg = Config::default();
        let raw = toml::to_string_pretty(&cfg).unwrap();
        let decoded: Config = toml::from_str(&raw).unwrap();
        assert_eq!(decoded, cfg);
    }
}
