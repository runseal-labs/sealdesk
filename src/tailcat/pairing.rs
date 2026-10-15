use std::{
    error::Error,
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

const PREFIX: &str = "SEALDESK:";

#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode {
    pub address: String,
    pub password: Option<String>,
    pub name: Option<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PairingError {
    InvalidFormat,
    InvalidEscape,
    MissingField(&'static str),
    DuplicateField(&'static str),
    UnsupportedVersion,
    InvalidAddress,
    InvalidExpiry,
    Expired,
    InvalidClock,
}

impl fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("Invalid SealDesk pairing code"),
            Self::InvalidEscape => formatter.write_str("Invalid pairing code escape"),
            Self::MissingField(field) => write!(formatter, "Missing pairing field: {field}"),
            Self::DuplicateField(field) => write!(formatter, "Duplicate pairing field: {field}"),
            Self::UnsupportedVersion => {
                formatter.write_str("Unsupported pairing version; please upgrade SealDesk")
            }
            Self::InvalidAddress => formatter.write_str("Invalid tailcat address"),
            Self::InvalidExpiry => formatter.write_str("Invalid pairing expiry timestamp"),
            Self::Expired => {
                formatter.write_str("Pairing code expired; ask the host to refresh the QR code")
            }
            Self::InvalidClock => formatter.write_str("System clock is before the Unix epoch"),
        }
    }
}

impl Error for PairingError {}

impl PairingCode {
    pub fn parse(input: &str) -> Result<Self, PairingError> {
        Self::parse_at(input, unix_now()?)
    }

    pub fn parse_at(input: &str, now: u64) -> Result<Self, PairingError> {
        let input = input.trim();
        let body = input
            .get(..PREFIX.len())
            .filter(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
            .and_then(|_| input.get(PREFIX.len()..))
            .ok_or(PairingError::InvalidFormat)?;
        let mut version = None;
        let mut address = None;
        let mut password = None;
        let mut name = None;
        let mut expiry = None;
        let mut field = String::new();
        let mut escaped = false;
        let mut finished = false;
        let mut characters = body.chars();
        while let Some(character) = characters.next() {
            if escaped {
                if !is_special(character) {
                    return Err(PairingError::InvalidEscape);
                }
                field.push(character);
                escaped = false;
            } else if character == '\\' {
                if !field.contains(':') {
                    return Err(PairingError::InvalidEscape);
                }
                escaped = true;
            } else if character == ';' {
                if field.is_empty() {
                    if characters.next().is_some() {
                        return Err(PairingError::InvalidFormat);
                    }
                    finished = true;
                    break;
                }
                let (key, value) = field.split_once(':').ok_or(PairingError::InvalidFormat)?;
                let (slot, label) = match key {
                    "V" => (&mut version, "V"),
                    "A" => (&mut address, "A"),
                    "P" => (&mut password, "P"),
                    "N" => (&mut name, "N"),
                    "E" => (&mut expiry, "E"),
                    "" => return Err(PairingError::InvalidFormat),
                    _ => {
                        field.clear();
                        continue;
                    }
                };
                if slot.replace(value.to_owned()).is_some() {
                    return Err(PairingError::DuplicateField(label));
                }
                field.clear();
            } else {
                field.push(character);
            }
        }
        if escaped {
            return Err(PairingError::InvalidEscape);
        }
        if !finished {
            return Err(PairingError::InvalidFormat);
        }
        if version.ok_or(PairingError::MissingField("V"))? != "1" {
            return Err(PairingError::UnsupportedVersion);
        }
        let expires_at = expiry
            .map(|value| {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(PairingError::InvalidExpiry);
                }
                value.parse().map_err(|_| PairingError::InvalidExpiry)
            })
            .transpose()?;
        let pairing = Self {
            address: address.ok_or(PairingError::MissingField("A"))?,
            password,
            name,
            expires_at,
        };
        pairing.validate_at(now)?;
        Ok(pairing)
    }

    pub fn encode(&self) -> Result<String, PairingError> {
        self.encode_at(unix_now()?)
    }

    pub fn encode_at(&self, now: u64) -> Result<String, PairingError> {
        self.validate_at(now)?;
        let mut output = format!("{PREFIX}V:1;A:{};", self.address);
        for (key, value) in [("P", &self.password), ("N", &self.name)] {
            if let Some(value) = value {
                output.push_str(key);
                output.push(':');
                for character in value.chars() {
                    if is_special(character) {
                        output.push('\\');
                    }
                    output.push(character);
                }
                output.push(';');
            }
        }
        if let Some(expiry) = self.expires_at {
            output.push_str(&format!("E:{expiry};"));
        }
        output.push(';');
        Ok(output)
    }

    fn validate_at(&self, now: u64) -> Result<(), PairingError> {
        if !super::is_addr(&self.address) {
            return Err(PairingError::InvalidAddress);
        }
        if self.expires_at.map_or(false, |expiry| expiry <= now) {
            return Err(PairingError::Expired);
        }
        Ok(())
    }
}

fn is_special(character: char) -> bool {
    matches!(character, '\\' | ';' | ',' | ':' | '"')
}

fn unix_now() -> Result<u64, PairingError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| PairingError::InvalidClock)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: &str = "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-";

    fn pairing() -> PairingCode {
        PairingCode {
            address: ADDRESS.to_owned(),
            password: Some("secret".to_owned()),
            name: Some("ros2go-gpu".to_owned()),
            expires_at: Some(200),
        }
    }

    fn assert_error(input: &str, expected: PairingError) {
        assert!(PairingCode::parse_at(input, 100).err() == Some(expected));
    }

    #[test]
    fn matches_flutter_protocol_fixtures() {
        let fixtures: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../flutter/test/fixtures/sealdesk_pairing.json"
        ))
        .unwrap();
        for fixture in fixtures {
            let input = fixture["input"].as_str().unwrap();
            let now = fixture["now"].as_u64().unwrap();
            let result = PairingCode::parse_at(input, now);
            if let Some(expected) = fixture.get("expected") {
                let pairing = result.unwrap();
                assert_eq!(pairing.address, expected["address"].as_str().unwrap());
                assert_eq!(pairing.password.as_deref(), expected["password"].as_str());
                assert_eq!(pairing.name.as_deref(), expected["name"].as_str());
                assert_eq!(pairing.expires_at, expected["expires_at"].as_u64());
            } else {
                assert!(result.is_err(), "fixture: {}", fixture["label"]);
            }
        }
    }

    #[test]
    fn canonical_round_trip() {
        let original = pairing();
        let encoded = original.encode_at(100).unwrap();
        assert_eq!(
            encoded,
            format!("SEALDESK:V:1;A:{ADDRESS};P:secret;N:ros2go-gpu;E:200;;")
        );
        assert!(PairingCode::parse_at(&encoded, 100).unwrap() == original);
    }

    #[test]
    fn escape_special_characters_and_unicode() {
        let mut original = pairing();
        original.password = Some("\\;,:\" 中文🔑".to_owned());
        original.name = Some("卧室;电脑\\\"一号\",:🖥".to_owned());
        let encoded = original.encode_at(100).unwrap();
        assert!(encoded.contains(r#"P:\\\;\,\:\" 中文🔑;"#));
        assert!(PairingCode::parse_at(&encoded, 100).unwrap() == original);
    }

    #[test]
    fn optional_fields_can_be_missing_or_empty() {
        let parsed = PairingCode::parse_at(&format!("SEALDESK:A:{ADDRESS};V:1;;"), 100).unwrap();
        assert!(parsed.password.is_none());
        assert!(parsed.name.is_none());
        assert!(parsed.expires_at.is_none());
        let encoded = parsed.encode_at(100).unwrap();
        assert_eq!(encoded, format!("SEALDESK:V:1;A:{ADDRESS};;"));
        let parsed =
            PairingCode::parse_at(&format!("SEALDESK:V:1;A:{ADDRESS};P:;N:;;"), 100).unwrap();
        assert_eq!(parsed.password.as_deref(), Some(""));
        assert_eq!(parsed.name.as_deref(), Some(""));
    }

    #[test]
    fn prefix_order_and_unknown_fields() {
        let input = format!(
            r#"  sealdesk:X:future\;field;N:电脑;E:200;A:{ADDRESS};P:secret;V:1;X:ignored;;  "#
        );
        let parsed = PairingCode::parse_at(&input, 100).unwrap();
        assert_eq!(parsed.name.as_deref(), Some("电脑"));
        assert_eq!(parsed.password.as_deref(), Some("secret"));
        assert_eq!(parsed.expires_at, Some(200));
    }

    #[test]
    fn field_names_are_case_sensitive() {
        assert_error(
            &format!("SEALDESK:v:1;A:{ADDRESS};;"),
            PairingError::MissingField("V"),
        );
        assert_error(
            &format!("SEALDESK:V:1;a:{ADDRESS};;"),
            PairingError::MissingField("A"),
        );
    }

    #[test]
    fn missing_required_fields_and_unsupported_versions() {
        assert_error("SEALDESK:;", PairingError::MissingField("V"));
        assert_error("SEALDESK:V:1;;", PairingError::MissingField("A"));
        for version in ["", "0", "2", "01"] {
            assert_error(
                &format!("SEALDESK:V:{version};A:{ADDRESS};;"),
                PairingError::UnsupportedVersion,
            );
        }
    }

    #[test]
    fn rejects_duplicate_known_fields() {
        for field in ["V", "A", "P", "N", "E"] {
            let input = format!("SEALDESK:V:1;A:{ADDRESS};P:secret;N:host;E:200;{field}:other;;");
            assert!(matches!(
                PairingCode::parse_at(&input, 100),
                Err(PairingError::DuplicateField(_))
            ));
        }
    }

    #[test]
    fn rejects_expired_codes_at_boundary() {
        let encoded = pairing().encode_at(100).unwrap();
        assert!(PairingCode::parse_at(&encoded, 199).is_ok());
        for now in [200, 201, u64::MAX] {
            assert!(PairingCode::parse_at(&encoded, now).err() == Some(PairingError::Expired));
            assert!(pairing().encode_at(now).err() == Some(PairingError::Expired));
        }
    }

    #[test]
    fn rejects_invalid_expiry() {
        for expiry in [
            "",
            "-1",
            "+200",
            "1.5",
            "tomorrow",
            " 200",
            "18446744073709551616",
        ] {
            assert_error(
                &format!("SEALDESK:V:1;A:{ADDRESS};E:{expiry};;"),
                PairingError::InvalidExpiry,
            );
        }
        assert_error(
            &format!("SEALDESK:V:1;A:{ADDRESS};E:0;;"),
            PairingError::Expired,
        );
    }

    #[test]
    fn rejects_invalid_addresses_on_parse_and_encode() {
        for address in [
            "",
            "123456789",
            "tcshort",
            "https://example.com",
            "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef=",
        ] {
            assert_error(
                &format!("SEALDESK:V:1;A:{address};;"),
                PairingError::InvalidAddress,
            );
            let mut invalid = pairing();
            invalid.address = address.to_owned();
            assert!(invalid.encode_at(100).err() == Some(PairingError::InvalidAddress));
        }
    }

    #[test]
    fn rejects_malformed_or_truncated_codes() {
        for input in [
            "",
            "电脑",
            "SEALDESK",
            "SEALDESK:V:1",
            "SEALDESK:V:1;",
            "SEALDESK:V:1;;extra",
            "SEALDESK:V:1;;;",
            "SEALDESK:V:1;broken;;",
            "SEALDESK::empty;;",
        ] {
            assert_error(input, PairingError::InvalidFormat);
        }
        for suffix in ["", ";", ";N:unterminated"] {
            assert_error(
                &format!("SEALDESK:V:1;A:{ADDRESS}{suffix}"),
                PairingError::InvalidFormat,
            );
        }
    }

    #[test]
    fn rejects_invalid_and_dangling_escapes() {
        for suffix in [r"P:bad\x;;", "N:trailing\\", r"X:bad\x;;"] {
            assert_error(
                &format!("SEALDESK:V:1;A:{ADDRESS};{suffix}"),
                PairingError::InvalidEscape,
            );
        }
        assert_error(
            &format!(r"SEALDESK:V:1;A:{ADDRESS};P:ends\;;"),
            PairingError::InvalidFormat,
        );
        assert_error(
            &format!(r"SEALDESK:V:1;A\:{ADDRESS};;"),
            PairingError::InvalidEscape,
        );
    }

    #[test]
    fn live_clock_round_trip() {
        let mut original = pairing();
        original.expires_at = Some(unix_now().unwrap() + 600);
        assert!(PairingCode::parse(&original.encode().unwrap()).unwrap() == original);
    }
}
