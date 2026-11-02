# Arclight

A native Linux launcher for World of Warcraft, built with Rust and [Iced](https://iced.rs).

Arclight manages Wine/Proton runners, Battle.net installation, and game launching from a single GUI.

## Features

- Auto-detect or manually select Wine/Proton-GE runners
- Install and launch Battle.net within a managed Wine prefix
- Launch Retail, Classic, and user-defined WoW clients from one install root
- Show installed and latest game versions, with a warning when a client is out of date. Installed
  versions come from the install's `.build.info`; the latest comes from one read-only request per
  product to Blizzard's public version service (`<region>.version.battle.net`), cached for 30
  minutes and switchable off in Settings. Arclight never downloads or patches game files.
- Detect installed flavors (PTR, Beta, Classic Era, ...) in the install folder and offer to add them as clients
- Background image for the main screen: your own (globally or per client), or official artwork. Official
  artwork is read from the public WoW game pages (`worldofwarcraft.blizzard.com`), checked about
  weekly, and cached in `~/.cache/arclight/art`. Blizzard's images are never bundled or
  redistributed, and the Settings checkbox turns this off.
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
