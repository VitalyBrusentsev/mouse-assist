use eframe::egui;
use mouse_assist_core::{
    default_config_path, load_config, save_config, Action, Binding, Config, MouseButton,
};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "mouse-assist",
        options,
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
}

struct App {
    config_path: PathBuf,
    config: Config,
    status: String,
}

impl App {
    fn new() -> Self {
        let config_path = default_config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
        let config = load_config(&config_path).unwrap_or_default();
        Self {
            config_path,
            config,
            status: String::new(),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Config: {}", self.config_path.display()));
                if ui.button("Save").clicked() {
                    match save_config(&self.config_path, &self.config) {
                        Ok(_) => self.status = "Saved".into(),
                        Err(e) => self.status = format!("Save failed: {e}"),
                    }
                }
                if ui.button("Add binding").clicked() {
                    self.config.bindings.push(Binding {
                        button: MouseButton::BtnSide,
                        action: Action::KeyCombo {
                            keys: vec!["KEY_BACK".into()],
                        },
                    });
                }
                if !self.status.is_empty() {
                    ui.separator();
                    ui.label(&self.status);
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Bindings");
            ui.add_space(8.0);

            let mut remove_index: Option<usize> = None;

            for (idx, binding) in self.config.bindings.iter_mut().enumerate() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(format!("#{idx}"));
                        ui.separator();

                        ui.label("Button:");
                        egui::ComboBox::from_id_salt(format!("button-{idx}"))
                            .selected_text(format!("{:?}", binding.button))
                            .show_ui(ui, |ui| {
                                for b in [
                                    MouseButton::BtnLeft,
                                    MouseButton::BtnRight,
                                    MouseButton::BtnMiddle,
                                    MouseButton::BtnSide,
                                    MouseButton::BtnExtra,
                                    MouseButton::BtnForward,
                                    MouseButton::BtnBack,
                                    MouseButton::BtnTask,
                                ] {
                                    ui.selectable_value(&mut binding.button, b, format!("{:?}", b));
                                }
                            });

                        if ui.button("Remove").clicked() {
                            remove_index = Some(idx);
                        }
                    });

                    ui.add_space(6.0);
                    let mut replacement_action: Option<Action> = None;
                    match &mut binding.action {
                        Action::Command { argv } => {
                            let mut switch = false;
                            ui.horizontal(|ui| {
                                ui.label("Action:");
                                ui.label("command");
                                if ui.button("Switch to key_combo").clicked() {
                                    switch = true;
                                }
                            });
                            let mut text = argv.join(" ");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut text)
                                        .hint_text("argv (space-separated)"),
                                )
                                .changed()
                            {
                                *argv = text.split_whitespace().map(|s| s.to_string()).collect();
                            }
                            if switch {
                                replacement_action = Some(Action::KeyCombo {
                                    keys: vec!["KEY_BACK".into()],
                                });
                            }
                        }
                        Action::KeyCombo { keys } => {
                            let mut switch = false;
                            ui.horizontal(|ui| {
                                ui.label("Action:");
                                ui.label("key_combo");
                                if ui.button("Switch to command").clicked() {
                                    switch = true;
                                }
                            });
                            let mut text = keys.join(" ");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut text).hint_text(
                                        "keys (space-separated, e.g. KEY_LEFTMETA KEY_L)",
                                    ),
                                )
                                .changed()
                            {
                                *keys = text.split_whitespace().map(|s| s.to_string()).collect();
                            }
                            if switch {
                                replacement_action = Some(Action::Command {
                                    argv: vec![
                                        "notify-send".into(),
                                        "mouse-assist".into(),
                                        "key combo triggered".into(),
                                    ],
                                });
                            }
                        }
                    }
                    if let Some(action) = replacement_action {
                        binding.action = action;
                    }
                });
                ui.add_space(8.0);
            }

            if let Some(idx) = remove_index {
                if idx < self.config.bindings.len() {
                    self.config.bindings.remove(idx);
                }
            }
        });
    }
}
