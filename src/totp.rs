//! RFC 4226 / RFC 6238 code generation plus parsing of `otpauth://` and
//! `otpauth-migration://` (Google Authenticator export) URIs.

use data_encoding::{BASE32_NOPAD, BASE64, BASE64_NOPAD, Encoding, HEXLOWER_PERMISSIVE, Specification};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Algorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub const ALL: [Algorithm; 3] = [Algorithm::Sha1, Algorithm::Sha256, Algorithm::Sha512];

    pub fn name(self) -> &'static str {
        match self {
            Algorithm::Sha1 => "SHA1",
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_uppercase().replace('-', "").as_str() {
            "SHA1" => Some(Algorithm::Sha1),
            "SHA256" => Some(Algorithm::Sha256),
            "SHA512" => Some(Algorithm::Sha512),
            _ => None,
        }
    }

    pub fn index(self) -> i32 {
        Self::ALL.iter().position(|a| *a == self).unwrap_or(0) as i32
    }

    pub fn from_index(i: i32) -> Self {
        Self::ALL.get(i as usize).copied().unwrap_or_default()
    }
}

/// Everything needed to generate codes for one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpParams {
    pub name: String,
    pub issuer: String,
    /// Canonical, unpadded upper-case base32.
    pub secret: String,
    pub algorithm: Algorithm,
    pub digits: u32,
    pub period: u64,
}

/// Base32 decoder that tolerates lower case, spaces, dashes, padding and
/// non-zero trailing bits, which some generators produce.
fn lenient_base32() -> Encoding {
    let mut spec = Specification::new();
    spec.symbols.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567");
    spec.check_trailing_bits = false;
    spec.encoding().expect("valid base32 spec")
}

pub fn decode_base32(secret: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = secret
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '=')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("Secret is empty".into());
    }
    lenient_base32()
        .decode(cleaned.as_bytes())
        .map_err(|e| format!("Secret is not valid base32 ({e})"))
}

pub fn encode_base32(bytes: &[u8]) -> String {
    BASE32_NOPAD.encode(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretEncoding {
    Base32,
    Hex,
    Ascii,
}

impl SecretEncoding {
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => SecretEncoding::Hex,
            2 => SecretEncoding::Ascii,
            _ => SecretEncoding::Base32,
        }
    }
}

/// Converts user input in any supported encoding to canonical base32.
pub fn normalize_secret(input: &str, encoding: SecretEncoding) -> Result<String, String> {
    let bytes = match encoding {
        SecretEncoding::Base32 => decode_base32(input)?,
        SecretEncoding::Hex => {
            let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
            let cleaned = cleaned.strip_prefix("0x").unwrap_or(&cleaned);
            HEXLOWER_PERMISSIVE
                .decode(cleaned.as_bytes())
                .map_err(|e| format!("Secret is not valid hex ({e})"))?
        }
        SecretEncoding::Ascii => input.as_bytes().to_vec(),
    };
    if bytes.is_empty() {
        return Err("Secret is empty".into());
    }
    Ok(encode_base32(&bytes))
}

pub fn hotp(key: &[u8], counter: u64, digits: u32, algorithm: Algorithm) -> String {
    let msg = counter.to_be_bytes();
    let hash: Vec<u8> = match algorithm {
        Algorithm::Sha1 => mac::<Hmac<sha1::Sha1>>(key, &msg),
        Algorithm::Sha256 => mac::<Hmac<sha2::Sha256>>(key, &msg),
        Algorithm::Sha512 => mac::<Hmac<sha2::Sha512>>(key, &msg),
    };
    let offset = (hash[hash.len() - 1] & 0x0f) as usize;
    let binary = u32::from_be_bytes([hash[offset], hash[offset + 1], hash[offset + 2], hash[offset + 3]]) & 0x7fff_ffff;
    let digits = digits.clamp(1, 10);
    let code = u64::from(binary) % 10u64.pow(digits);
    format!("{code:0width$}", width = digits as usize)
}

fn mac<M: Mac + KeyInit>(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut m = <M as KeyInit>::new_from_slice(key).expect("HMAC accepts any key length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

/// Time step counter `T` for a unix timestamp in seconds.
pub fn counter_at(unix_secs: i64, period: u64) -> u64 {
    (unix_secs.max(0) as u64) / period.max(1)
}

pub fn totp(params: &OtpParams, counter: u64) -> Result<String, String> {
    let key = decode_base32(&params.secret)?;
    Ok(hotp(&key, counter, params.digits, params.algorithm))
}

/// Groups a code for display: `123 456`, `1234 5678`.
pub fn format_code(code: &str) -> String {
    let split = match code.len() {
        6 => 3,
        7 => 4,
        8 => 4,
        9 => 3,
        10 => 5,
        _ => return code.to_string(),
    };
    let mut out = String::with_capacity(code.len() + 2);
    for (i, c) in code.chars().enumerate() {
        if i > 0 && i % split == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(b) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parses one URI (`otpauth://totp/...` or `otpauth-migration://offline?data=...`).
pub fn parse_uri(uri: &str) -> Result<Vec<OtpParams>, String> {
    let uri = uri.trim();
    let url = url::Url::parse(uri).map_err(|e| format!("Not a valid URI ({e})"))?;
    match url.scheme() {
        "otpauth" => parse_otpauth(&url).map(|p| vec![p]),
        "otpauth-migration" => parse_migration(&url),
        other => Err(format!("Unsupported scheme '{other}'")),
    }
}

fn parse_otpauth(url: &url::Url) -> Result<OtpParams, String> {
    match url.host_str().map(|h| h.to_ascii_lowercase()) {
        Some(h) if h == "totp" => {}
        Some(h) if h == "hotp" => return Err("HOTP (counter based) URIs are not supported".into()),
        _ => return Err("Expected otpauth://totp/...".into()),
    }
    let label = percent_decode(url.path().trim_start_matches('/'));
    let (label_issuer, name) = match label.split_once(':') {
        Some((i, n)) => (i.trim().to_string(), n.trim().to_string()),
        None => (String::new(), label.trim().to_string()),
    };

    let mut secret = None;
    let mut issuer = None;
    let mut algorithm = Algorithm::Sha1;
    let mut digits = 6;
    let mut period = 30;
    for (k, v) in url.query_pairs() {
        match k.to_ascii_lowercase().as_str() {
            "secret" => secret = Some(v.into_owned()),
            "issuer" => issuer = Some(v.into_owned()),
            "algorithm" => algorithm = Algorithm::parse(&v).ok_or_else(|| format!("Unknown algorithm '{v}'"))?,
            "digits" => digits = v.parse().map_err(|_| format!("Invalid digits '{v}'"))?,
            "period" => period = v.parse().map_err(|_| format!("Invalid period '{v}'"))?,
            _ => {}
        }
    }
    let secret = secret.ok_or("URI has no secret parameter")?;
    let secret = normalize_secret(&secret, SecretEncoding::Base32)?;
    validate_digits_period(digits, period)?;
    Ok(OtpParams {
        name,
        issuer: issuer.filter(|i| !i.is_empty()).unwrap_or(label_issuer),
        secret,
        algorithm,
        digits,
        period,
    })
}

pub fn validate_digits_period(digits: u32, period: u64) -> Result<(), String> {
    if !(1..=10).contains(&digits) {
        return Err("Digits must be between 1 and 10".into());
    }
    if period == 0 {
        return Err("Period must be at least 1 second".into());
    }
    Ok(())
}

pub fn build_uri(p: &OtpParams) -> String {
    let label = if p.issuer.is_empty() {
        percent_encode(&p.name)
    } else {
        format!("{}:{}", percent_encode(&p.issuer), percent_encode(&p.name))
    };
    let mut uri = format!("otpauth://totp/{label}?secret={}", p.secret);
    if !p.issuer.is_empty() {
        uri.push_str(&format!("&issuer={}", percent_encode(&p.issuer)));
    }
    uri.push_str(&format!("&algorithm={}&digits={}&period={}", p.algorithm.name(), p.digits, p.period));
    uri
}

// --- Google Authenticator export (protobuf MigrationPayload) ---------------

enum Field<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let b = *buf.get(*pos).ok_or("Truncated migration payload")?;
        *pos += 1;
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("Invalid varint in migration payload".into())
}

fn read_fields(buf: &[u8]) -> Result<Vec<(u64, Field<'_>)>, String> {
    let mut pos = 0;
    let mut fields = Vec::new();
    while pos < buf.len() {
        let key = read_varint(buf, &mut pos)?;
        let (number, wire) = (key >> 3, key & 7);
        match wire {
            0 => fields.push((number, Field::Varint(read_varint(buf, &mut pos)?))),
            1 => pos += 8,
            2 => {
                let len = read_varint(buf, &mut pos)? as usize;
                let end = pos.checked_add(len).filter(|e| *e <= buf.len()).ok_or("Truncated migration payload")?;
                fields.push((number, Field::Bytes(&buf[pos..end])));
                pos = end;
            }
            5 => pos += 4,
            _ => return Err("Unsupported field in migration payload".into()),
        }
    }
    Ok(fields)
}

fn parse_migration(url: &url::Url) -> Result<Vec<OtpParams>, String> {
    let data = url
        .query_pairs()
        .find(|(k, _)| k == "data")
        .map(|(_, v)| v.replace(' ', "+"))
        .ok_or("Migration URI has no data parameter")?;
    let raw = BASE64
        .decode(data.as_bytes())
        .or_else(|_| BASE64_NOPAD.decode(data.trim_end_matches('=').as_bytes()))
        .map_err(|e| format!("Migration data is not valid base64 ({e})"))?;

    let mut out = Vec::new();
    for (number, field) in read_fields(&raw)? {
        let (1, Field::Bytes(otp)) = (number, field) else { continue };
        let mut secret = Vec::new();
        let (mut name, mut issuer) = (String::new(), String::new());
        let (mut algorithm, mut digits, mut kind) = (Algorithm::Sha1, 6, 2);
        for (n, f) in read_fields(otp)? {
            match (n, f) {
                (1, Field::Bytes(b)) => secret = b.to_vec(),
                (2, Field::Bytes(b)) => name = String::from_utf8_lossy(b).into_owned(),
                (3, Field::Bytes(b)) => issuer = String::from_utf8_lossy(b).into_owned(),
                (4, Field::Varint(v)) => {
                    algorithm = match v {
                        2 => Algorithm::Sha256,
                        3 => Algorithm::Sha512,
                        _ => Algorithm::Sha1,
                    }
                }
                (5, Field::Varint(v)) => digits = if v == 2 { 8 } else { 6 },
                (6, Field::Varint(v)) => kind = v,
                _ => {}
            }
        }
        if kind == 1 || secret.is_empty() {
            continue; // HOTP entries are skipped
        }
        // Google puts "Issuer:account" into the name field.
        if let Some((prefix, rest)) = name.split_once(':') {
            if issuer.is_empty() || prefix.trim() == issuer {
                if issuer.is_empty() {
                    issuer = prefix.trim().to_string();
                }
                name = rest.trim().to_string();
            }
        }
        out.push(OtpParams { name, issuer, secret: encode_base32(&secret), algorithm, digits, period: 30 });
    }
    if out.is_empty() {
        return Err("Migration payload contains no TOTP entries".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 Appendix B test vectors.
    fn rfc(alg: Algorithm, ascii: &str) -> OtpParams {
        OtpParams {
            name: "rfc".into(),
            issuer: String::new(),
            secret: normalize_secret(ascii, SecretEncoding::Ascii).unwrap(),
            algorithm: alg,
            digits: 8,
            period: 30,
        }
    }

    #[test]
    fn rfc6238_vectors() {
        let sha1 = rfc(Algorithm::Sha1, "12345678901234567890");
        let sha256 = rfc(Algorithm::Sha256, "12345678901234567890123456789012");
        let sha512 = rfc(Algorithm::Sha512, "1234567890123456789012345678901234567890123456789012345678901234");
        let cases: [(i64, &str, &str, &str); 6] = [
            (59, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ];
        for (t, a, b, c) in cases {
            assert_eq!(totp(&sha1, counter_at(t, 30)).unwrap(), a, "sha1 t={t}");
            assert_eq!(totp(&sha256, counter_at(t, 30)).unwrap(), b, "sha256 t={t}");
            assert_eq!(totp(&sha512, counter_at(t, 30)).unwrap(), c, "sha512 t={t}");
        }
    }

    #[test]
    fn uri_roundtrip() {
        let p = parse_uri("otpauth://totp/ACME%20Co:john.doe@email.com?secret=HXDMVJECJJWSRB3HWIZR4IFUGFTMXBOZ&issuer=ACME%20Co&algorithm=SHA256&digits=8&period=60").unwrap().remove(0);
        assert_eq!(p.issuer, "ACME Co");
        assert_eq!(p.name, "john.doe@email.com");
        assert_eq!(p.algorithm, Algorithm::Sha256);
        assert_eq!((p.digits, p.period), (8, 60));
        assert_eq!(parse_uri(&build_uri(&p)).unwrap().remove(0), p);
    }

    #[test]
    fn lenient_secret() {
        assert_eq!(normalize_secret("jbsw y3dp ehpk 3pxp====", SecretEncoding::Base32).unwrap(), "JBSWY3DPEHPK3PXP");
        assert_eq!(normalize_secret("48656c6c6f21deadbeef", SecretEncoding::Hex).unwrap(), "JBSWY3DPEHPK3PXP");
    }

    #[test]
    fn migration_payload() {
        // One TOTP entry: secret "Hello!\xde\xad\xbe\xef", name "Issuer:alice", issuer "Issuer", SHA1, 6 digits.
        let mut otp = vec![0x0a, 10];
        otp.extend_from_slice(b"Hello!\xde\xad\xbe\xef");
        otp.extend_from_slice(&[0x12, 12]);
        otp.extend_from_slice(b"Issuer:alice");
        otp.extend_from_slice(&[0x1a, 6]);
        otp.extend_from_slice(b"Issuer");
        otp.extend_from_slice(&[0x20, 1, 0x28, 1, 0x30, 2]);
        let mut payload = vec![0x0a, otp.len() as u8];
        payload.extend_from_slice(&otp);
        payload.extend_from_slice(&[0x10, 1]);
        let uri = format!("otpauth-migration://offline?data={}", percent_encode(&BASE64.encode(&payload)));
        let entries = parse_uri(&uri).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].secret, "JBSWY3DPEHPK3PXP");
        assert_eq!(entries[0].name, "alice");
        assert_eq!(entries[0].issuer, "Issuer");
    }

    #[test]
    fn grouping() {
        assert_eq!(format_code("123456"), "123 456");
        assert_eq!(format_code("12345678"), "1234 5678");
    }
}
