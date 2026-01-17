use clap::{Parser, Subcommand};
use mouse_assist_core::{default_config_path, load_config, save_config, Action, Config};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
#[command(name = "mouse-assist-daemon")]
#[command(about = "Remap mouse buttons to system actions", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Print the default config path and exit.
    ConfigPath,
    /// Write a default config file if it doesn't exist.
    WriteDefaultConfig {
        /// Override output path (defaults to the standard config location).
        #[arg(long)]
        path: Option<PathBuf>,
        /// Overwrite if the file already exists.
        #[arg(long)]
        force: bool,
    },
    /// List /dev/input/event* devices (best-effort; may require permissions).
    ListDevices,
    /// Run the background event loop (defaults to all matching devices).
    Run {
        /// Restrict to a single /dev/input/eventX device node.
        #[arg(long)]
        device: Option<PathBuf>,
        /// Path to a config.toml (defaults to the standard config location).
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(thiserror::Error, Debug)]
enum AppError {
    #[error(transparent)]
    Config(#[from] mouse_assist_core::ConfigError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

fn main() -> Result<(), AppError> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::ConfigPath => {
            println!("{}", default_config_path()?.display());
        }
        Command::WriteDefaultConfig { path, force } => {
            let path = path.unwrap_or(default_config_path()?);
            if path.exists() && !force {
                warn!("config already exists: {}", path.display());
                return Ok(());
            }
            save_config(&path, &Config::default())?;
            info!("wrote config: {}", path.display());
        }
        Command::ListDevices => {
            list_devices()?;
        }
        Command::Run { device, config } => {
            let config_path = config.unwrap_or(default_config_path()?);
            let config = if config_path.exists() {
                load_config(&config_path)?
            } else {
                warn!(
                    "config not found (creating default): {}",
                    config_path.display()
                );
                let cfg = Config::default();
                save_config(&config_path, &cfg)?;
                cfg
            };
            if let Some(device_path) =
                device.or_else(|| config.device_by_path.as_ref().map(PathBuf::from))
            {
                run_device(&device_path, &config)?;
            } else {
                run_all_devices(&config)?;
            }
        }
    }

    Ok(())
}

fn list_devices() -> Result<(), AppError> {
    let mut entries: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir("/dev/input")? {
        let path = entry?.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("event") {
                entries.push(path);
            }
        }
    }
    entries.sort();

    for path in entries {
        match evdev::Device::open(&path) {
            Ok(dev) => {
                let name = dev.name().unwrap_or("<unknown>");
                println!("{}  {}", path.display(), name);
            }
            Err(err) => {
                println!("{}  <unreadable: {}>", path.display(), err);
            }
        }
    }
    Ok(())
}

fn run_device(device_path: &Path, config: &Config) -> Result<(), AppError> {
    info!("opening device: {}", device_path.display());
    let mut dev = evdev::Device::open(device_path)?;
    info!("device name: {}", dev.name().unwrap_or("<unknown>"));
    dev.set_nonblocking(false)?;

    let mut executor = ActionExecutor::new(config)?;

    loop {
        for ev in dev.fetch_events()? {
            if let evdev::EventSummary::Key(_event, keycode, value) = ev.destructure() {
                if value == 1 {
                    let code = keycode.code();
                    if let Some(binding) = config
                        .bindings
                        .iter()
                        .find(|b| b.button.linux_input_code() == code)
                    {
                        executor.execute_action(&binding.action);
                    }
                }
            }
        }
    }
}

fn run_all_devices(config: &Config) -> Result<(), AppError> {
    let binding_codes: Vec<evdev::KeyCode> = config
        .bindings
        .iter()
        .map(|b| evdev::KeyCode::new(b.button.linux_input_code()))
        .collect();

    let mut devices: Vec<(PathBuf, evdev::Device)> = evdev::enumerate()
        .filter_map(|(path, dev)| {
            let keys = dev.supported_keys()?;
            if !binding_codes.iter().any(|c| keys.contains(*c)) {
                return None;
            }
            if let Err(err) = dev.set_nonblocking(true) {
                warn!("failed to set nonblocking for {}: {err}", path.display());
            }
            Some((path, dev))
        })
        .collect();

    if devices.is_empty() {
        warn!("no input devices matched current bindings; try `list-devices` or pass `--device`");
        return Ok(());
    }

    info!("listening on {} device(s)", devices.len());
    for (path, dev) in &devices {
        info!(
            "device: {} ({})",
            path.display(),
            dev.name().unwrap_or("<unknown>")
        );
    }

    let mut executor = ActionExecutor::new(config)?;

    loop {
        let mut saw_any = false;
        let mut i = 0;
        while i < devices.len() {
            let path_for_log = devices[i].0.clone();
            let mut remove = false;
            let mut remove_reason: Option<std::io::Error> = None;

            {
                let (_path, dev) = &mut devices[i];
                match dev.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if let evdev::EventSummary::Key(_event, keycode, value) =
                                ev.destructure()
                            {
                                if value == 1 {
                                    saw_any = true;
                                    let code = keycode.code();
                                    if let Some(binding) = config
                                        .bindings
                                        .iter()
                                        .find(|b| b.button.linux_input_code() == code)
                                    {
                                        executor.execute_action(&binding.action);
                                    }
                                }
                            }
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(err) => {
                        remove = true;
                        remove_reason = Some(err);
                    }
                }
            }

            if remove {
                let err = remove_reason.expect("remove implies error");
                warn!(
                    "dropping device {} due to error: {err}",
                    path_for_log.display()
                );
                devices.remove(i);
            } else {
                i += 1;
            }
        }

        if devices.is_empty() {
            warn!("no devices left to read; exiting");
            return Ok(());
        }

        if !saw_any {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct ActionExecutor {
    keyboard: Option<evdev::uinput::VirtualDevice>,
}

impl ActionExecutor {
    fn new(config: &Config) -> Result<Self, AppError> {
        let keys = collect_uinput_keys(config);
        let keyboard = if keys.iter().next().is_none() {
            None
        } else {
            match evdev::uinput::VirtualDevice::builder()
                .and_then(|b| b.name("mouse-assist-virtual-keyboard").with_keys(&keys))
                .and_then(|b| b.build())
            {
                Ok(dev) => Some(dev),
                Err(err) => {
                    warn!("failed to initialize uinput keyboard (KeyCombo disabled): {err}");
                    None
                }
            }
        };

        Ok(Self { keyboard })
    }

    fn execute_action(&mut self, action: &Action) {
        match action {
            Action::Command { argv } => self.execute_command(argv),
            Action::KeyCombo { keys } => self.execute_key_combo(keys),
        }
    }

    fn execute_command(&self, argv: &[String]) {
        if argv.is_empty() {
            warn!("ignoring empty command argv");
            return;
        }
        let mut cmd = std::process::Command::new(&argv[0]);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        match cmd.spawn() {
            Ok(_) => info!("executed command: {:?}", argv),
            Err(err) => error!("failed to execute {:?}: {}", argv, err),
        }
    }

    fn execute_key_combo(&mut self, keys: &[String]) {
        let Some(keyboard) = &mut self.keyboard else {
            warn!("key injection unavailable (uinput device not initialized)");
            return;
        };

        let parsed: Vec<evdev::KeyCode> = keys
            .iter()
            .filter_map(|k| match evdev::KeyCode::from_str(k) {
                Ok(code) => Some(code),
                Err(_) => {
                    warn!("unknown key code in config: {}", k);
                    None
                }
            })
            .collect();

        if parsed.is_empty() {
            return;
        }

        let mut events: Vec<evdev::InputEvent> = Vec::with_capacity(parsed.len());
        for code in &parsed {
            events.push(evdev::InputEvent::new_now(
                evdev::EventType::KEY.0,
                code.0,
                1,
            ));
        }
        if let Err(err) = keyboard.emit(&events) {
            error!("failed to inject key press: {err}");
            return;
        }

        let mut events: Vec<evdev::InputEvent> = Vec::with_capacity(parsed.len());
        for code in parsed.iter().rev() {
            events.push(evdev::InputEvent::new_now(
                evdev::EventType::KEY.0,
                code.0,
                0,
            ));
        }
        if let Err(err) = keyboard.emit(&events) {
            error!("failed to inject key release: {err}");
        }
    }
}

fn collect_uinput_keys(config: &Config) -> evdev::AttributeSet<evdev::KeyCode> {
    let mut keys: Vec<evdev::KeyCode> = Vec::new();
    for binding in &config.bindings {
        if let Action::KeyCombo { keys: combo } = &binding.action {
            for key in combo {
                if let Ok(code) = evdev::KeyCode::from_str(key) {
                    keys.push(code);
                }
            }
        }
    }

    if keys.is_empty() {
        return evdev::AttributeSet::new();
    }

    keys.sort_by_key(|k| k.code());
    keys.dedup_by_key(|k| k.code());
    evdev::AttributeSet::from_iter(keys)
}
