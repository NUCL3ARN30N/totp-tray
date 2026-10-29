# TOTP Tray

[![GitHub stars](https://img.shields.io/github/stars/NUCL3ARN30N/totp-tray?style=for-the-badge)](https://github.com/NUCL3ARN30N/totp-tray/stargazers)
[![GitHub forks](https://img.shields.io/github/forks/NUCL3ARN30N/totp-tray?style=for-the-badge)](https://github.com/NUCL3ARN30N/totp-tray/network/members)
[![GitHub issues](https://img.shields.io/github/issues/NUCL3ARN30N/totp-tray?style=for-the-badge)](https://github.com/NUCL3ARN30N/totp-tray/issues)
[![GitHub license](https://img.shields.io/github/license/NUCL3ARN30N/totp-tray?style=for-the-badge)](LICENSE)

![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
![Slint](https://img.shields.io/badge/Slint-2379F4?style=for-the-badge&logo=slint&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-0078D6?style=for-the-badge&logo=windows&logoColor=white)

> Small Windows tray tool for testing TOTP (RFC 6238) — import secrets, see current, previous and next codes, and move the clock to any point in time

Built in Rust with [Slint](https://slint.dev) (Material style, follows the Windows light/dark setting)
and [`tray-icon`](https://crates.io/crates/tray-icon).

## Quick Start

```bash
cargo build --release
```

The binary is `target/release/totp-tray.exe`. Release builds have no console window. It starts in the
tray, so it can go straight into the Startup folder. Pass `--show` to open the separate window on
launch. Only one instance runs at a time.

## Features

| #  | Feature               | Description                                                                                                                                                                                                                                                                           |
| -- | --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1  | **Lives in the tray** | Starts silently in the tray. The very first launch shows the popup once, so you can see where it lives                                                                                                                                                                                |
| 2  | **Timestamp panel**   | Unix timestamp, UTC and local time, and the mode chip (`LIVE`, `OFFSET +30s`, `FROZEN`)                                                                                                                                                                                               |
| 3  | **Per entry**         | Code with a countdown bar (turns red in the last 5 s), the previous and next codes, the counter `T`, the algorithm, digits and period. Click the code or the copy icon to copy it. The eye icon reveals the base32 secret and the `otpauth://` URI. Entries can be edited and deleted |
| 4  | **Add or edit**       | Paste an `otpauth://` URI and press *Parse*, or fill in the fields by hand. The secret can be entered as **Base32, Hex or ASCII** (handy for RFC test vectors). SHA1/SHA256/SHA512, 1–10 digits and any period are supported. A live preview shows the resulting code                 |
| 5  | **Import**            | URIs, Google Authenticator exports, QR codes and config files (see [Import](#import))                                                                                                                                                                                                 |
| 6  | **Export**            | The config as JSON, or as a `.txt` list of `otpauth://` URIs                                                                                                                                                                                                                          |
| 7  | **Clear all**         | With a confirmation step                                                                                                                                                                                                                                                              |

## Window Modes

| Mode                     | Description                                                                                                                                                                                                                                               |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Tray popup** (default) | Left-click the tray icon to open a borderless flyout above the tray. It stays on top and closes when you click elsewhere, press `Esc`, or click the icon again                                                                                            |
| **Separate window**      | The pop-out button (⧉) in the header, *Open as separate window* in the ⋮ menu, or *Open in window* in the tray menu opens a normal movable window with a taskbar button. Its position and size are remembered. The dock button switches back to the popup |
| **Last choice wins**     | Whichever you choose last is what a tray click opens next time (saved in the config)                                                                                                                                                                      |
| **Tray menu**            | Right-click for *Open popup*, *Open in window*, *Copy code: &lt;entry&gt;* for every entry, and *Quit*                                                                                                                                                    |

## Clock Controls

| Control                       | Effect                                                                                                                                                                   |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| **`−30s / −1s / +1s / +30s`** | Shift the clock                                                                                                                                                          |
| **Freeze / Resume**           | Stops the clock or lets it run again from the frozen point                                                                                                               |
| **Time field**                | Accepts Unix seconds, Unix milliseconds (13+ digits), RFC 3339 (`2026-01-01T12:00:00Z`), local `YYYY-MM-DD HH:MM[:SS]`, or a relative shift (`+90`, `-5m`, `+1h`, `+2d`) |
| **Live**                      | Resets to real time                                                                                                                                                      |

## Import

| Source                                     | Handling                                                                                                             |
| ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| **`otpauth://totp/...` URIs**              | Paste multiple, one per line                                                                                         |
| **`otpauth-migration://offline?data=...`** | Google Authenticator export, decoded into all of its TOTP entries                                                    |
| **From clipboard**                         | Text URIs, or a QR code screenshot (e.g. Win+Shift+S)                                                                |
| **QR image…**                              | PNG/JPEG/BMP/GIF/WebP files                                                                                          |
| **Config file…**                           | A JSON config or a `.txt` list of URIs. It merges and skips duplicates, or replaces everything when the switch is on |

## Config

The config is stored at `%APPDATA%\TotpTray\config.json`. Set `TOTP_TRAY_CONFIG` to use a different
file, for example a separate profile per test environment.

```json
{
  "version": 1,
  "settings": { "open_mode": "popup" },
  "entries": [
    { "id": "…", "name": "alice@example.com", "issuer": "ACME",
      "secret": "JBSWY3DPEHPK3PXP", "algorithm": "SHA1", "digits": 6, "period": 30 }
  ]
}
```

Only `secret` is required when you write a config by hand. Everything else has defaults.

> **Note:** secrets are stored **unencrypted**. This is a testing tool, so don't put production 2FA secrets in it.

## Tests

```bash
cargo test
```

The tests cover the RFC 6238 Appendix B vectors (SHA1/256/512), URI parsing and round trips,
migration payload decoding, time-input parsing and config import/export.

## Requirements

**OS:** Windows  
**Build:** Rust toolchain (`cargo`)  
**Storage:** `%APPDATA%\TotpTray\config.json`, or the file in `TOTP_TRAY_CONFIG`

---

Badges from [envoy1084/awesome-badges](https://github.com/envoy1084/awesome-badges)
