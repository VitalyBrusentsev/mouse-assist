# Repository Guidelines

## Project Structure

- `crates/mouse-assist-daemon/`: background service that reads Linux input events and triggers actions.
- `crates/mouse-assist-config-app/`: GUI app for editing user remapping settings.
- `crates/mouse-assist-core/`: shared config model + load/save helpers (TOML).
- `config/`: example configuration files.
- `systemd/`: sample `systemd --user` unit files.

## Build, Test, and Development Commands

- `cargo build --workspace`: build all crates.
- `cargo test --workspace`: run unit tests.
- `cargo fmt --all`: format (rustfmt).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: lint.
- `cargo run -p mouse-assist-daemon -- config-path`: print the default config location (XDG).
- `cargo run -p mouse-assist-daemon -- write-default-config`: create a default config at the standard XDG location.
- `cargo run -p mouse-assist-daemon -- run`: run the daemon against all matching devices.
- `cargo run -p mouse-assist-daemon -- run --device /dev/input/eventX`: restrict to one device node.
- `cargo run -p mouse-assist-daemon -- run --grab`: on X11, grab configured buttons (6-9) so apps don't receive them (prevents browser back/forward double-actions).
- `cargo run -p mouse-assist-config-app`: run the GUI config editor.

## Coding Style & Naming Conventions

- Rust 2021; keep diffs `rustfmt`-clean.
- Names: `snake_case` for modules/functions, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for input key names in TOML.
- Prefer small, testable units in `mouse-assist-core` and keep OS-specific logic in the daemon.

## Testing Guidelines

- Keep tests deterministic and local (avoid reading `/dev/input`).

## Commit & Pull Request Guidelines

- No established commit history yet. Use a consistent convention such as Conventional Commits (`feat:`, `fix:`, `chore:`).
- PRs should include: what changed, how to test (`cargo …`), screenshots for GUI changes, and any permission/security implications.

## Security & Configuration

- `command.argv` actions execute programs directly; treat `config.toml` as trusted input.
- X11 backend (Mint Cinnamon): uses XInput2 + XTest; no `/dev/input` or `/dev/uinput` permissions, but still has “can synthesize input” implications.
- Wayland: global key injection is restricted; evdev/uinput typically needs udev rules/groups (`/dev/input/event*`, `/dev/uinput`).

## Agent Notes

- Keep the TOML config stable/backwards-compatible; avoid hard DE dependencies.
- Wheel tilt is supported via `WHEEL_TILT_LEFT` / `WHEEL_TILT_RIGHT` in `bindings.button` (X11 buttons 6/7; evdev `REL_HWHEEL*`).

## Promising Future Improvements

- Gesture semantics: `press`/`hold`/`double_click`.
- Macros: sequences of actions with delays.
- True remapping: grab devices and re-emit a virtual mouse + keyboard.
