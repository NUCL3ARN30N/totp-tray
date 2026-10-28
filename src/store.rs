//! Entries and the JSON config file (`%APPDATA%\TotpTray\config.json`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::totp::{self, Algorithm, OtpParams};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub issuer: String,
    pub secret: String,
    #[serde(default)]
    pub algorithm: Algorithm,
    #[serde(default = "default_digits")]
    pub digits: u32,
    #[serde(default = "default_period")]
    pub period: u64,
}

fn default_digits() -> u32 {
    6
}

fn default_period() -> u64 {
    30
}

impl Entry {
    pub fn from_params(p: OtpParams) -> Self {
        Entry {
            id: new_id(),
            name: p.name,
            issuer: p.issuer,
            secret: p.secret,
            algorithm: p.algorithm,
            digits: p.digits,
            period: p.period,
        }
    }

    pub fn params(&self) -> OtpParams {
        OtpParams {
            name: self.name.clone(),
            issuer: self.issuer.clone(),
            secret: self.secret.clone(),
            algorithm: self.algorithm,
            digits: self.digits,
            period: self.period,
        }
    }

    pub fn label(&self) -> String {
        match (self.issuer.is_empty(), self.name.is_empty()) {
            (false, false) => format!("{} ({})", self.name, self.issuer),
            (true, false) => self.name.clone(),
            (false, true) => self.issuer.clone(),
            (true, true) => "Unnamed".into(),
        }
    }

    fn same_account(&self, other: &Entry) -> bool {
        self.secret == other.secret
            && self.name == other.name
            && self.issuer == other.issuer
            && self.algorithm == other.algorithm
            && self.digits == other.digits
            && self.period == other.period
    }
}

pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{:x}{:04x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed) & 0xffff)
}

/// How a left click on the tray icon opens the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OpenMode {
    /// Borderless flyout above the tray that hides when it loses focus.
    #[default]
    Popup,
    /// Regular, movable window with a taskbar button.
    Window,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub open_mode: OpenMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "config_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

fn config_version() -> u32 {
    1
}

impl Default for Config {
    fn default() -> Self {
        Config { version: config_version(), settings: Settings::default(), entries: Vec::new() }
    }
}

impl Config {
    /// `TOTP_TRAY_CONFIG` overrides the location, e.g. for separate test profiles.
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("TOTP_TRAY_CONFIG") {
            return PathBuf::from(p);
        }
        dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("TotpTray").join("config.json")
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Config::default());
        }
        Self::read_file(&path)
    }

    pub fn save(&self) -> Result<(), String> {
        self.write_file(&Self::path())
    }

    /// Reads a config JSON file, or a text file with one URI per line.
    pub fn read_file(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        if text.trim_start().starts_with('{') {
            let mut cfg: Config =
                serde_json::from_str(&text).map_err(|e| format!("Invalid config file: {e}"))?;
            cfg.sanitize()?;
            Ok(cfg)
        } else {
            let (params, errors) = parse_uri_lines(&text);
            if params.is_empty() {
                return Err(errors.into_iter().next().unwrap_or_else(|| "File contains no entries".into()));
            }
            Ok(Config {
                version: config_version(),
                settings: Settings::default(),
                entries: params.into_iter().map(Entry::from_params).collect(),
            })
        }
    }

    pub fn write_file(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temp file first so a crash never leaves a half-written config.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| format!("Cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("Cannot write {}: {e}", path.display()))
    }

    /// Normalizes secrets, fills missing ids and rejects broken entries.
    fn sanitize(&mut self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            e.secret = totp::normalize_secret(&e.secret, totp::SecretEncoding::Base32)
                .map_err(|err| format!("Entry {} ('{}'): {err}", i + 1, e.label()))?;
            totp::validate_digits_period(e.digits, e.period)
                .map_err(|err| format!("Entry {} ('{}'): {err}", i + 1, e.label()))?;
            if e.id.is_empty() || !seen.insert(e.id.clone()) {
                e.id = new_id();
                seen.insert(e.id.clone());
            }
        }
        Ok(())
    }

    /// Appends entries that are not already present. Returns (added, skipped).
    pub fn merge(&mut self, incoming: Vec<Entry>) -> (usize, usize) {
        let (mut added, mut skipped) = (0, 0);
        for mut e in incoming {
            if self.entries.iter().any(|x| x.same_account(&e)) {
                skipped += 1;
                continue;
            }
            if e.id.is_empty() || self.entries.iter().any(|x| x.id == e.id) {
                e.id = new_id();
            }
            self.entries.push(e);
            added += 1;
        }
        (added, skipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, secret: &str) -> Entry {
        Entry {
            id: String::new(),
            name: name.into(),
            issuer: "Test".into(),
            secret: secret.into(),
            algorithm: Algorithm::Sha256,
            digits: 8,
            period: 60,
        }
    }

    #[test]
    fn export_import_roundtrip_and_merge() {
        let dir = std::env::temp_dir().join(format!("totp-tray-test-{}", new_id()));
        let path = dir.join("export.json");
        let mut cfg = Config::default();
        cfg.merge(vec![entry("a", "JBSWY3DPEHPK3PXP"), entry("b", "GEZDGNBVGY3TQOJQ")]);
        cfg.write_file(&path).unwrap();

        let loaded = Config::read_file(&path).unwrap();
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].algorithm, Algorithm::Sha256);
        assert_eq!((loaded.entries[1].digits, loaded.entries[1].period), (8, 60));

        // Re-importing the same file adds nothing.
        assert_eq!(cfg.merge(loaded.entries), (0, 2));

        // A plain URI list is accepted as a config file too.
        let txt = dir.join("uris.txt");
        let uris: String = cfg.entries.iter().map(|e| totp::build_uri(&e.params()) + "\n").collect();
        std::fs::write(&txt, uris).unwrap();
        assert_eq!(Config::read_file(&txt).unwrap().entries.len(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_bad_secret_in_config() {
        let dir = std::env::temp_dir().join(format!("totp-tray-test-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, r#"{"entries":[{"name":"x","secret":"not base32!"}]}"#).unwrap();
        assert!(Config::read_file(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Parses every non-empty line as a URI. Returns parsed entries and per-line errors.
pub fn parse_uri_lines(text: &str) -> (Vec<OtpParams>, Vec<String>) {
    let mut params = Vec::new();
    let mut errors = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match totp::parse_uri(line) {
            Ok(mut p) => params.append(&mut p),
            Err(e) => errors.push(format!("Line {}: {e}", n + 1)),
        }
    }
    (params, errors)
}
