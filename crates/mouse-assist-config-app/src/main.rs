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
    selected_binding: Option<usize>,
    status: String,
}

impl App {
    fn new() -> Self {
        let config_path = default_config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
        let config = load_config(&config_path).unwrap_or_default();
        let selected_binding = (!config.bindings.is_empty()).then_some(0);
        Self {
            config_path,
            config,
            selected_binding,
            status: String::new(),
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Config: {}", self.config_path.display()));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save").clicked() {
                        match save_config(&self.config_path, &self.config) {
                            Ok(_) => self.status = "Saved".into(),
                            Err(e) => self.status = format!("Save failed: {e}"),
                        }
                    }
                    if !self.status.is_empty() {
                        ui.separator();
                        ui.label(&self.status);
                    }
                });
            });
        });

        let style = ctx.style();
        let panel_frame = egui::Frame::central_panel(&style);

        egui::SidePanel::left("bindings_list")
            .default_width(520.0)
            .min_width(360.0)
            .frame(panel_frame.clone())
            .show(ctx, |ui| {
                ui.heading("Bindings");
                ui.add_space(8.0);

                let mut remove_index: Option<usize> = None;

                let bottom_tile_height = 44.0;
                let max_scroll_height =
                    (ui.available_height() - bottom_tile_height - ui.spacing().item_spacing.y)
                        .max(0.0);
                egui::ScrollArea::vertical()
                    .max_height(max_scroll_height)
                    .show(ui, |ui| {
                        for (idx, binding) in self.config.bindings.iter_mut().enumerate() {
                            let is_selected = self.selected_binding == Some(idx);
                            let visuals = ui.visuals();
                            let selected_stroke = visuals.selection.stroke;
                            let frame = egui::Frame::group(ui.style()).stroke(if is_selected {
                                selected_stroke
                            } else {
                                visuals.widgets.noninteractive.bg_stroke
                            });

                            frame.show(ui, |ui| {
                                ui.set_min_width(ui.available_width());

                                ui.horizontal(|ui| {
                                    if ui
                                        .selectable_label(is_selected, format!("#{idx}"))
                                        .clicked()
                                    {
                                        self.selected_binding = Some(idx);
                                    }
                                    ui.separator();

                                    ui.label("Button:");
                                    let previous_button = binding.button;
                                    let button_response =
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
                                                    MouseButton::WheelTiltLeft,
                                                    MouseButton::WheelTiltRight,
                                                ] {
                                                    ui.selectable_value(
                                                        &mut binding.button,
                                                        b,
                                                        format!("{:?}", b),
                                                    );
                                                }
                                            });
                                    if binding.button != previous_button
                                        || button_response.response.clicked()
                                        || button_response.response.changed()
                                    {
                                        self.selected_binding = Some(idx);
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let response = ui
                                                .add_sized(
                                                    [28.0, 28.0],
                                                    egui::Button::new(
                                                        egui::RichText::new("×")
                                                            .color(egui::Color32::LIGHT_RED),
                                                    ),
                                                )
                                                .on_hover_text("Remove binding");
                                            if response.clicked() {
                                                remove_index = Some(idx);
                                            }
                                        },
                                    );
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
                                            self.selected_binding = Some(idx);
                                            *argv = text
                                                .split_whitespace()
                                                .map(|s| s.to_string())
                                                .collect();
                                        }
                                        if switch {
                                            self.selected_binding = Some(idx);
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
                                            .add(egui::TextEdit::singleline(&mut text).hint_text(
                                                "keys (space-separated, e.g. KEY_LEFTMETA KEY_L)",
                                            ))
                                            .changed()
                                        {
                                            self.selected_binding = Some(idx);
                                            *keys = text
                                                .split_whitespace()
                                                .map(|s| s.to_string())
                                                .collect();
                                        }
                                        if switch {
                                            self.selected_binding = Some(idx);
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
                    });

                let add_clicked = {
                    let visuals = ui.visuals();
                    egui::Frame::group(ui.style())
                        .stroke(visuals.widgets.noninteractive.bg_stroke)
                        .show(ui, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 28.0],
                                egui::Button::new("Add binding"),
                            )
                            .clicked()
                        })
                        .inner
                };
                if add_clicked {
                    self.config.bindings.push(Binding {
                        button: MouseButton::BtnSide,
                        action: Action::KeyCombo {
                            keys: vec!["KEY_BACK".into()],
                        },
                    });
                    self.selected_binding = Some(self.config.bindings.len().saturating_sub(1));
                }

                if let Some(idx) = remove_index {
                    if idx < self.config.bindings.len() {
                        self.config.bindings.remove(idx);
                        self.selected_binding = match self.selected_binding {
                            None => None,
                            Some(selected) if selected == idx => {
                                if self.config.bindings.is_empty() {
                                    None
                                } else {
                                    Some(idx.min(self.config.bindings.len().saturating_sub(1)))
                                }
                            }
                            Some(selected) if selected > idx => Some(selected - 1),
                            Some(selected) => Some(selected),
                        };
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                ui.heading("Info");
                ui.add_space(8.0);

                let Some(selected_idx) = self.selected_binding else {
                    ui.label("Select a binding to see details.");
                    return;
                };
                let Some(binding) = self.config.bindings.get(selected_idx) else {
                    ui.label("Select a binding to see details.");
                    return;
                };

                ui.label(format!("Selected: {}", binding.button.toml_name()));
                ui.add_space(8.0);

                match &binding.action {
                    Action::KeyCombo { keys } => {
                        ui.label("key_combo:");
                        ui.label("- Keys are Linux evdev key names like KEY_BACK.");
                        ui.label("- Presses all keys, then releases them (chord).");
                        if keys.is_empty() {
                            ui.label("- (No keys configured)");
                        }
                    }
                    Action::Command { argv } => {
                        ui.label("command:");
                        ui.label("- Executes argv directly (no shell).");
                        if argv.is_empty() {
                            ui.label("- (No argv configured)");
                        }
                    }
                }

                ui.add_space(12.0);
                ui.label("TOML snippet:");

                let mut snippet = mouse_assist_core::binding_to_toml_string(binding)
                    .unwrap_or_else(|e| format!("# Failed to render snippet: {e}\n"));
                ui.add(
                    egui::TextEdit::multiline(&mut snippet)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6)
                        .interactive(false),
                );
            });
    }
}
