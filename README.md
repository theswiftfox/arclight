# Arclight

A native Linux launcher for World of Warcraft, built with Rust and [Iced](https://iced.rs).

Arclight manages Wine/Proton runners, Battle.net installation, and game launching from a single GUI.

## Features

- Auto-detect or manually select Wine/Proton-GE runners
- Install and launch Battle.net within a managed Wine prefix
- Launch Retail, Classic, and user-defined WoW clients from one install root
- Configure per-client executables, Wine/Proton runners, prefixes, arguments, and environment variables

## Building

```sh
cargo build --release
```

The binary is output to `target/release/arclight`.

Run checks with:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## Configuration

Settings are stored in `~/.config/arclight/config.toml` and can be edited via the Settings screen in the GUI.

Game client executable paths are relative to the shared World of Warcraft install root. Arclight
provides editable Retail and Classic entries; additional clients such as PTR, Beta, or future
releases can be added without an application update.

## License

See [LICENSE](LICENSE) for details.

Arclight bundles [Noto Emoji](https://fonts.google.com/noto/specimen/Noto+Emoji),
licensed under the [SIL Open Font License 1.1](assets/LICENSE-NotoEmoji.txt).
