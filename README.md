# mouse-assist

Linux tool for translating extra mouse buttons into system actions.

This repo is a Rust workspace with:
- `mouse-assist-daemon`: background service that listens to `/dev/input` events and executes actions.
- `mouse-assist-config-app`: visual configuration app for editing remapping settings.
- `mouse-assist-core`: shared config/types.

## Quick start (dev)

1) Create an initial config:
```bash
cargo run -p mouse-assist-daemon -- write-default-config
```

2) Run the config app:
```bash
cargo run -p mouse-assist-config-app
```

3) Run the daemon:
```bash
cargo run -p mouse-assist-daemon -- run
```

On X11 sessions (`XDG_SESSION_TYPE=x11`, e.g., Linux Mint Cinnamon), this uses an X11 backend (no `/dev/input` or `/dev/uinput` permissions needed).
On Wayland sessions, the daemon falls back to the evdev/uinput approach, which typically requires udev/group setup.

### Prevent browser back/forward (X11)

Browsers often handle mouse side buttons (X11 buttons 8/9) as Back/Forward. If you bind those
buttons to something else (e.g. volume), you may see both actions at once.

To suppress the default application handling, run the daemon with:
```bash
cargo run -p mouse-assist-daemon -- run --grab
```
