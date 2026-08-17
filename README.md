# Arclight

A native Linux launcher for World of Warcraft, built with Rust and [Iced](https://iced.rs).

Arclight manages Wine/Proton runners, Battle.net installation, and game launching from a single GUI.

## Features

- Auto-detect or manually select Wine/Proton-GE runners
- Install and launch Battle.net within a managed Wine prefix
- Launch WoW with configurable arguments and environment variables

## Building

```sh
cargo build --release
```

The binary is output to `target/release/arclight`.

## Configuration

Settings are stored in `~/.config/arclight/config.toml` and can be edited via the Settings screen in the GUI.

## License

See [LICENSE](LICENSE) for details.
