# TOTP Tray

A small Windows tray tool for **testing TOTP (RFC 6238)**. You can import secrets, look at the current, previous and next codes, and move the clock around to see what a code was or will be at any point in time.

Built in Rust with [Slint](https://slint.dev) (Material style, follows the Windows light/dark setting) and [`tray-icon`](https://crates.io/crates/tray-icon).

## Features

- **Lives in the tray.** The app starts silently in the tray. The very first launch shows the popup once, so you can see where it lives.
  - **Tray popup (default):** left-click the tray icon to open a borderless flyout above the tray. It stays on top and closes when you click elsewhere, press `Esc`, or click the icon again.
  - **Separate window:** the pop-out button (⧉) in the header, *Open as separate window* in the ⋮ menu, or *Open in window* in the tray menu opens a normal movable window with a taskbar button. Its position and size are remembered. The dock button switches back to the popup.
  - Whichever you choose last is what a tray click opens next time (saved in the config).
  - The right-click tray menu has *Open popup*, *Open in window*, *Copy code: <entry>* for every entry, and *Quit*.
- **Timestamp panel.** Shows the Unix timestamp, UTC and local time, and the mode chip (`LIVE`, `OFFSET +30s`, `FROZEN`).
  - `−30s / −1s / +1s / +30s` shift the clock.
  - **Freeze / Resume** stops the clock or lets it run again from the frozen point.
  - The time field accepts Unix seconds, Unix milliseconds (13+ digits), RFC 3339 (`2026-01-01T12:00:00Z`), local `YYYY-MM-DD HH:MM[:SS]`, or a relative shift (`+90`, `-5m`, `+1h`, `+2d`).
  - **Live** resets to real time.
- **Per entry.** Shows the code with a countdown bar (turns red in the last 5 s), the previous and next codes, the counter `T`, the algorithm, digits and period. Click the code or the copy icon to copy it. The eye icon reveals the base32 secret and the `otpauth://` URI. Entries can be edited and deleted.
- **Add or edit.** Paste an `otpauth://` URI and press *Parse*, or fill in the fields by hand. The secret can be entered as **Base32, Hex or ASCII** (handy for RFC test vectors). SHA1/SHA256/SHA512, 1–10 digits and any period are supported. A live preview shows the resulting code.
- **Import**
  - Paste multiple `otpauth://totp/...` URIs, one per line.
  - `otpauth-migration://offline?data=...` (Google Authenticator export) is decoded into all of its TOTP entries.
  - **From clipboard:** text URIs, or a QR code screenshot (e.g. Win+Shift+S).
  - **QR image…:** PNG/JPEG/BMP/GIF/WebP files.
  - **Config file…:** a JSON config or a `.txt` list of URIs. It merges and skips duplicates, or replaces everything when the switch is on.
- **Export** the config as JSON, or as a `.txt` list of `otpauth://` URIs.
- **Clear all**, with a confirmation step.

## Build & run

```bash
cargo build --release
```

The binary is `target/release/totp-tray.exe`. Release builds have no console window. It starts in the tray, so it can go straight into the Startup folder. Pass `--show` to open the separate window on launch. Only one instance runs at a time.

```bash
cargo test
```

The tests cover the RFC 6238 Appendix B vectors (SHA1/256/512), URI parsing and round trips, migration payload decoding, time-input parsing and config import/export.

## Config

The config is stored at `%APPDATA%\TotpTray\config.json`. Set `TOTP_TRAY_CONFIG` to use a different file, for example a separate profile per test environment.

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
