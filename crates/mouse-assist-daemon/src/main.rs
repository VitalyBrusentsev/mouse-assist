use clap::{Parser, Subcommand};
use mouse_assist_core::{
    default_config_path, load_config, save_config, Action, Config, MouseButton,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, info, warn};
use x11rb::connection::Connection as _;
use x11rb::protocol::{xinput, xproto, Event};
use x11rb::protocol::{
    xinput::ConnectionExt as _, xproto::ConnectionExt as _, xtest::ConnectionExt as _,
};

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
    #[error("x11 connect error: {0}")]
    X11Connect(#[from] x11rb::errors::ConnectError),
    #[error("x11 connection error: {0}")]
    X11Connection(#[from] x11rb::errors::ConnectionError),
    #[error("x11 reply error: {0}")]
    X11Reply(#[from] x11rb::errors::ReplyError),
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
            } else if is_x11_session() {
                run_x11(&config)?;
            } else {
                run_all_devices(&config)?;
            }
        }
    }

    Ok(())
}

fn is_x11_session() -> bool {
    match std::env::var("XDG_SESSION_TYPE") {
        Ok(t) if t == "x11" => return true,
        Ok(t) if t == "wayland" => return false,
        _ => {}
    }
    std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none()
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
                        .find(|b| b.button.linux_key_code() == Some(code))
                    {
                        executor.execute_action(&binding.action);
                    }
                }
            }
            if let evdev::EventSummary::RelativeAxis(_event, axis, value) = ev.destructure() {
                if let Some(tilt) = wheel_tilt_from_relative_axis(axis, value) {
                    let button = match tilt {
                        WheelTilt::Left => MouseButton::WheelTiltLeft,
                        WheelTilt::Right => MouseButton::WheelTiltRight,
                    };
                    if let Some(binding) = config.bindings.iter().find(|b| b.button == button) {
                        executor.execute_action(&binding.action);
                    }
                }
            }
        }
    }
}

fn run_all_devices(config: &Config) -> Result<(), AppError> {
    let key_binding_codes: Vec<evdev::KeyCode> = config
        .bindings
        .iter()
        .filter_map(|b| b.button.linux_key_code().map(evdev::KeyCode::new))
        .collect();
    let wants_wheel_tilt = config.bindings.iter().any(|b| {
        matches!(
            b.button,
            MouseButton::WheelTiltLeft | MouseButton::WheelTiltRight
        )
    });

    let mut devices: Vec<(PathBuf, evdev::Device)> = evdev::enumerate()
        .filter_map(|(path, dev)| {
            let keys_match = dev.supported_keys().map_or(false, |keys| {
                key_binding_codes.iter().any(|c| keys.contains(*c))
            });
            let rel_match = wants_wheel_tilt
                && dev.supported_relative_axes().map_or(false, |axes| {
                    axes.contains(evdev::RelativeAxisCode::REL_HWHEEL)
                        || axes.contains(evdev::RelativeAxisCode::REL_HWHEEL_HI_RES)
                });
            if !keys_match && !rel_match {
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
                                        .find(|b| b.button.linux_key_code() == Some(code))
                                    {
                                        executor.execute_action(&binding.action);
                                    }
                                }
                            }
                            if let evdev::EventSummary::RelativeAxis(_event, axis, value) =
                                ev.destructure()
                            {
                                if let Some(tilt) = wheel_tilt_from_relative_axis(axis, value) {
                                    saw_any = true;
                                    let button = match tilt {
                                        WheelTilt::Left => MouseButton::WheelTiltLeft,
                                        WheelTilt::Right => MouseButton::WheelTiltRight,
                                    };
                                    if let Some(binding) =
                                        config.bindings.iter().find(|b| b.button == button)
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

fn run_x11(config: &Config) -> Result<(), AppError> {
    let (conn, screen_num) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen_num].root;

    conn.xinput_xi_query_version(2, 0)?.reply()?;
    conn.xtest_get_version(2, 2)?.reply()?;

    conn.xinput_xi_select_events(
        root,
        &[xinput::EventMask {
            deviceid: 0,
            mask: vec![xinput::XIEventMask::RAW_BUTTON_PRESS],
        }],
    )?;
    conn.flush()?;

    let mut executor = X11Executor::new(conn, root, config)?;

    loop {
        match executor.conn.wait_for_event()? {
            Event::XinputRawButtonPress(ev) => executor.on_button_press(ev.detail),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WheelTilt {
    Left,
    Right,
}

fn wheel_tilt_from_relative_axis(axis: evdev::RelativeAxisCode, value: i32) -> Option<WheelTilt> {
    if !matches!(
        axis,
        evdev::RelativeAxisCode::REL_HWHEEL | evdev::RelativeAxisCode::REL_HWHEEL_HI_RES
    ) {
        return None;
    }

    if value < 0 {
        Some(WheelTilt::Left)
    } else if value > 0 {
        Some(WheelTilt::Right)
    } else {
        None
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

struct X11Executor {
    conn: x11rb::rust_connection::RustConnection,
    root: xproto::Window,
    keysym_to_keycode: std::collections::HashMap<xproto::Keysym, xproto::Keycode>,
    bindings_by_button: std::collections::HashMap<u32, Action>,
}

impl X11Executor {
    fn new(
        conn: x11rb::rust_connection::RustConnection,
        root: xproto::Window,
        config: &Config,
    ) -> Result<Self, AppError> {
        let keysym_to_keycode = build_x11_keysym_map(&conn)?;
        let bindings_by_button = config
            .bindings
            .iter()
            .filter_map(|b| Some((b.button.x11_button_number()?, b.action.clone())))
            .collect();

        Ok(Self {
            conn,
            root,
            keysym_to_keycode,
            bindings_by_button,
        })
    }

    fn on_button_press(&mut self, button_detail: u32) {
        let action = self.bindings_by_button.get(&button_detail).cloned();
        if let Some(action) = action {
            self.execute_action(&action);
        }
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
        if keys == ["KEY_BACK"] {
            if self.inject_key_by_keysym(x11_dl::keysym::XF86XK_Back as u32) {
                return;
            }
            self.inject_keysym_combo(&[
                x11_dl::keysym::XK_Alt_L as u32,
                x11_dl::keysym::XK_Left as u32,
            ]);
            return;
        }
        if keys == ["KEY_FORWARD"] {
            if self.inject_key_by_keysym(x11_dl::keysym::XF86XK_Forward as u32) {
                return;
            }
            self.inject_keysym_combo(&[
                x11_dl::keysym::XK_Alt_L as u32,
                x11_dl::keysym::XK_Right as u32,
            ]);
            return;
        }

        let mut keycodes: Vec<xproto::Keycode> = Vec::new();
        for key in keys {
            let Some(keysym) = linux_key_name_to_x11_keysym(key) else {
                warn!("unknown key name in config (x11 backend): {key}");
                continue;
            };
            let Some(keycode) = self.keysym_to_keycode.get(&keysym).copied() else {
                warn!("no X11 keycode found for keysym=0x{keysym:x} (key={key})");
                continue;
            };
            keycodes.push(keycode);
        }

        self.inject_keycode_combo(&keycodes);
    }

    fn inject_key_by_keysym(&mut self, keysym: xproto::Keysym) -> bool {
        let Some(keycode) = self.keysym_to_keycode.get(&keysym).copied() else {
            return false;
        };
        self.inject_keycode_combo(&[keycode]);
        true
    }

    fn inject_keysym_combo(&mut self, keysyms: &[xproto::Keysym]) {
        let mut keycodes: Vec<xproto::Keycode> = Vec::with_capacity(keysyms.len());
        for &keysym in keysyms {
            let Some(keycode) = self.keysym_to_keycode.get(&keysym).copied() else {
                warn!("no X11 keycode found for keysym=0x{keysym:x}");
                return;
            };
            keycodes.push(keycode);
        }
        self.inject_keycode_combo(&keycodes);
    }

    fn inject_keycode_combo(&mut self, keycodes: &[xproto::Keycode]) {
        if keycodes.is_empty() {
            return;
        }

        for &keycode in keycodes {
            if let Err(err) =
                self.conn
                    .xtest_fake_input(xproto::KEY_PRESS_EVENT, keycode, 0, self.root, 0, 0, 0)
            {
                error!("xtest key press failed: {err}");
                return;
            }
        }
        if let Err(err) = self.conn.flush() {
            error!("x11 flush failed: {err}");
            return;
        }

        for &keycode in keycodes.iter().rev() {
            if let Err(err) = self.conn.xtest_fake_input(
                xproto::KEY_RELEASE_EVENT,
                keycode,
                0,
                self.root,
                0,
                0,
                0,
            ) {
                error!("xtest key release failed: {err}");
                return;
            }
        }
        if let Err(err) = self.conn.flush() {
            error!("x11 flush failed: {err}");
        }
    }
}

fn build_x11_keysym_map(
    conn: &x11rb::rust_connection::RustConnection,
) -> Result<std::collections::HashMap<xproto::Keysym, xproto::Keycode>, AppError> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max.saturating_sub(min).saturating_add(1);

    let reply = conn.get_keyboard_mapping(min, count)?.reply()?;
    let per = reply.keysyms_per_keycode as usize;
    let mut map = std::collections::HashMap::new();

    if per == 0 {
        return Ok(map);
    }

    for (idx, chunk) in reply.keysyms.chunks(per).enumerate() {
        let keycode = min.wrapping_add(idx as u8);
        for &keysym in chunk {
            if keysym != 0 {
                map.entry(keysym).or_insert(keycode);
            }
        }
    }

    Ok(map)
}

fn linux_key_name_to_x11_keysym(key: &str) -> Option<xproto::Keysym> {
    match key {
        "KEY_VOLUMEUP" => Some(x11_dl::keysym::XF86XK_AudioRaiseVolume as u32),
        "KEY_VOLUMEDOWN" => Some(x11_dl::keysym::XF86XK_AudioLowerVolume as u32),
        "KEY_MUTE" => Some(x11_dl::keysym::XF86XK_AudioMute as u32),
        "KEY_BACK" => Some(x11_dl::keysym::XF86XK_Back as u32),
        "KEY_FORWARD" => Some(x11_dl::keysym::XF86XK_Forward as u32),
        "KEY_LEFTALT" => Some(x11_dl::keysym::XK_Alt_L as u32),
        "KEY_RIGHTALT" => Some(x11_dl::keysym::XK_Alt_R as u32),
        "KEY_LEFTCTRL" => Some(x11_dl::keysym::XK_Control_L as u32),
        "KEY_RIGHTCTRL" => Some(x11_dl::keysym::XK_Control_R as u32),
        "KEY_LEFTSHIFT" => Some(x11_dl::keysym::XK_Shift_L as u32),
        "KEY_RIGHTSHIFT" => Some(x11_dl::keysym::XK_Shift_R as u32),
        "KEY_LEFTMETA" => Some(x11_dl::keysym::XK_Super_L as u32),
        "KEY_RIGHTMETA" => Some(x11_dl::keysym::XK_Super_R as u32),
        "KEY_LEFT" => Some(x11_dl::keysym::XK_Left as u32),
        "KEY_RIGHT" => Some(x11_dl::keysym::XK_Right as u32),
        _ => {
            if let Some(letter) = key.strip_prefix("KEY_") {
                if letter.len() == 1 {
                    let c = letter.as_bytes()[0];
                    if (b'A'..=b'Z').contains(&c) {
                        let lower = (c + 32) as char;
                        return Some(match lower {
                            'a' => x11_dl::keysym::XK_a as u32,
                            'b' => x11_dl::keysym::XK_b as u32,
                            'c' => x11_dl::keysym::XK_c as u32,
                            'd' => x11_dl::keysym::XK_d as u32,
                            'e' => x11_dl::keysym::XK_e as u32,
                            'f' => x11_dl::keysym::XK_f as u32,
                            'g' => x11_dl::keysym::XK_g as u32,
                            'h' => x11_dl::keysym::XK_h as u32,
                            'i' => x11_dl::keysym::XK_i as u32,
                            'j' => x11_dl::keysym::XK_j as u32,
                            'k' => x11_dl::keysym::XK_k as u32,
                            'l' => x11_dl::keysym::XK_l as u32,
                            'm' => x11_dl::keysym::XK_m as u32,
                            'n' => x11_dl::keysym::XK_n as u32,
                            'o' => x11_dl::keysym::XK_o as u32,
                            'p' => x11_dl::keysym::XK_p as u32,
                            'q' => x11_dl::keysym::XK_q as u32,
                            'r' => x11_dl::keysym::XK_r as u32,
                            's' => x11_dl::keysym::XK_s as u32,
                            't' => x11_dl::keysym::XK_t as u32,
                            'u' => x11_dl::keysym::XK_u as u32,
                            'v' => x11_dl::keysym::XK_v as u32,
                            'w' => x11_dl::keysym::XK_w as u32,
                            'x' => x11_dl::keysym::XK_x as u32,
                            'y' => x11_dl::keysym::XK_y as u32,
                            'z' => x11_dl::keysym::XK_z as u32,
                            _ => return None,
                        });
                    }
                }
            }
            None
        }
    }
}
