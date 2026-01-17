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

3) Run the daemon (requires permission to read `/dev/input/event*` and to inject keys via `/dev/uinput`):
```bash
cargo run -p mouse-assist-daemon -- run
```
