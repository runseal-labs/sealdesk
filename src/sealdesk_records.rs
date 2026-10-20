//! PR2 connection-record and QR-mask rules.
//!
//! Flutter UI calls the matching Dart API in
//! `flutter/lib/common/sealdesk_connection_record.dart` and
//! `flutter/lib/common/sealdesk_qr_mask.dart`. Keep the two sides in lockstep.
//! The LAN badge is the literal `LAN`, not the translated tab label.

use hbb_common::regex::Regex;
use lazy_static::lazy_static;
use serde_json::{json, Map, Value};

pub const LAN_BADGE: &str = "LAN";

pub const PR2_I18N_KEYS: &[&str] = &[
    "Scan to connect",
    "Recognize QR from gallery",
    "Recognize screen or clipboard QR",
    "Recognize QR from image",
    "tc… / lan: / SEALDESK: pairing code",
    "Encrypted direct connection",
    "Compatible with RustDesk · Unencrypted",
    "compat-lan-hint",
    "Clear history",
    "Clear connection history?",
    "Favorites are kept.",
    "No records",
    "No favorites",
    "Searching…",
    "Recent",
    "LAN",
    "Tap to show",
    "Sharing",
    "Invalid connection address",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Tailcat,
    Lan,
    Legacy,
}

impl RecordKind {
    fn as_str(self) -> &'static str {
        match self {
            RecordKind::Tailcat => "tailcat",
            RecordKind::Lan => "lan",
            RecordKind::Legacy => "legacy",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "tailcat" => Some(RecordKind::Tailcat),
            "lan" => Some(RecordKind::Lan),
            "legacy" => Some(RecordKind::Legacy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectEvent {
    pub address: String,
    pub hostname: String,
    pub alias: String,
    pub password: String,
    pub remember_password: bool,
    pub connected_at_millis: i64,
    pub favorite: Option<bool>,
}

impl ConnectEvent {
    pub fn new(address: impl Into<String>, connected_at_millis: i64) -> Self {
        Self {
            address: address.into(),
            hostname: String::new(),
            alias: String::new(),
            password: String::new(),
            remember_password: false,
            connected_at_millis,
            favorite: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub id: String,
    pub kind: RecordKind,
    pub alias: String,
    pub hostname: String,
    pub address: String,
    pub connected_at_millis: i64,
    pub favorite: bool,
    pub in_history: bool,
    pub password: String,
}

impl ConnectionRecord {
    pub fn display_address(&self) -> String {
        display_address(&self.address)
    }

    pub fn badge(&self) -> Option<&'static str> {
        record_badge(&self.address)
    }

    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("id".into(), json!(self.id));
        map.insert("kind".into(), json!(self.kind.as_str()));
        map.insert("alias".into(), json!(self.alias));
        map.insert("hostname".into(), json!(self.hostname));
        map.insert("address".into(), json!(self.address));
        map.insert("connectedAtMillis".into(), json!(self.connected_at_millis));
        map.insert("favorite".into(), json!(self.favorite));
        map.insert("inHistory".into(), json!(self.in_history));
        if !self.password.is_empty() {
            map.insert("password".into(), json!(self.password));
        }
        Value::Object(map)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    InvalidAddress,
    InvalidRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConnectionRecordStore {
    records: Vec<ConnectionRecord>,
}

impl ConnectionRecordStore {
    pub fn get(&self, id: &str) -> Option<ConnectionRecord> {
        self.records.iter().find(|record| record.id == id).cloned()
    }

    pub fn upsert(&mut self, event: ConnectEvent) -> Result<ConnectionRecord, RecordError> {
        let address = event.address.trim().to_owned();
        let kind = classify_address(&address).ok_or(RecordError::InvalidAddress)?;
        let id = record_id(&address)?;
        let previous = self.get(&id);
        let hostname = {
            let trimmed = event.hostname.trim();
            if !trimmed.is_empty() {
                trimmed.to_owned()
            } else {
                previous
                    .as_ref()
                    .map(|record| record.hostname.clone())
                    .unwrap_or_default()
            }
        };
        let alias = {
            let trimmed = event.alias.trim();
            if !trimmed.is_empty() {
                trimmed.to_owned()
            } else if let Some(record) = previous.as_ref() {
                if !record.alias.is_empty() {
                    record.alias.clone()
                } else if !hostname.is_empty() {
                    hostname.clone()
                } else {
                    display_address(&address)
                }
            } else if !hostname.is_empty() {
                hostname.clone()
            } else {
                display_address(&address)
            }
        };
        let record = ConnectionRecord {
            id: id.clone(),
            kind,
            alias,
            hostname,
            address,
            connected_at_millis: event.connected_at_millis,
            favorite: event.favorite.or_else(|| previous.as_ref().map(|record| record.favorite)).unwrap_or(false),
            in_history: true,
            password: password_to_store(event.remember_password, &event.password),
        };
        self.records.retain(|item| item.id != id);
        self.records.push(record.clone());
        Ok(record)
    }

    pub fn list_recent(&self) -> Vec<ConnectionRecord> {
        sorted(&self.records, |record| record.in_history)
    }

    pub fn list_favorites(&self) -> Vec<ConnectionRecord> {
        sorted(&self.records, |record| record.favorite)
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let before = self.records.len();
        self.records.retain(|record| record.id != id);
        self.records.len() != before
    }

    pub fn clear_history(&mut self) {
        self.records = self
            .records
            .iter()
            .filter(|record| record.favorite)
            .map(|record| {
                let mut kept = record.clone();
                kept.in_history = false;
                kept
            })
            .collect();
    }

    pub fn to_json(&self) -> Value {
        json!({
            "records": self.records.iter().map(ConnectionRecord::to_json).collect::<Vec<_>>(),
        })
    }

    pub fn from_json(value: &Value) -> Result<Self, RecordError> {
        let records = value
            .get("records")
            .and_then(Value::as_array)
            .ok_or(RecordError::InvalidRecord)?;
        let mut store = Self::default();
        for item in records {
            let record = ConnectionRecord::from_json(item)?;
            if store.get(&record.id).is_some() {
                return Err(RecordError::InvalidRecord);
            }
            store.records.push(record);
        }
        Ok(store)
    }
}

impl ConnectionRecord {
    fn from_json(value: &Value) -> Result<Self, RecordError> {
        let address = value
            .get("address")
            .and_then(Value::as_str)
            .ok_or(RecordError::InvalidRecord)?
            .to_owned();
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or(RecordError::InvalidRecord)?
            .to_owned();
        let kind_name = value
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(RecordError::InvalidRecord)?;
        let kind = RecordKind::parse(kind_name).ok_or(RecordError::InvalidRecord)?;
        let alias = json_string(value, "alias")?;
        let hostname = json_string(value, "hostname")?;
        let connected_at_millis = value
            .get("connectedAtMillis")
            .and_then(Value::as_i64)
            .ok_or(RecordError::InvalidRecord)?;
        let favorite = value
            .get("favorite")
            .and_then(Value::as_bool)
            .ok_or(RecordError::InvalidRecord)?;
        let in_history = value
            .get("inHistory")
            .and_then(Value::as_bool)
            .ok_or(RecordError::InvalidRecord)?;
        let password = match value.get("password") {
            None => String::new(),
            Some(Value::String(text)) => text.clone(),
            Some(_) => return Err(RecordError::InvalidRecord),
        };
        if classify_address(&address) != Some(kind) || record_id(&address)? != id {
            return Err(RecordError::InvalidRecord);
        }
        Ok(Self {
            id,
            kind,
            alias,
            hostname,
            address,
            connected_at_millis,
            favorite,
            in_history,
            password,
        })
    }
}

pub fn password_to_store(remember_password: bool, password: &str) -> String {
    if !remember_password {
        String::new()
    } else {
        password.to_owned()
    }
}

pub fn classify_address(address: &str) -> Option<RecordKind> {
    let value = address.trim();
    if value.len() > 40 && tailcat_re().is_match(value) {
        return Some(RecordKind::Tailcat);
    }
    if lan_re().is_match(value) || ipv4_re().is_match(value) {
        return Some(RecordKind::Lan);
    }
    if legacy_port_re().is_match(value) || user_host_re().is_match(value) {
        return Some(RecordKind::Legacy);
    }
    None
}

pub fn record_id(address: &str) -> Result<String, RecordError> {
    let value = address.trim();
    match classify_address(value) {
        Some(RecordKind::Lan) => lan_ip(value)
            .map(|ip| format!("lan:{ip}"))
            .ok_or(RecordError::InvalidAddress),
        Some(_) => Ok(value.to_owned()),
        None => Err(RecordError::InvalidAddress),
    }
}

pub fn short_address(address: &str) -> String {
    let chars: Vec<char> = address.chars().collect();
    if chars.len() <= 12 {
        return address.to_owned();
    }
    let head: String = chars.iter().take(6).collect();
    let tail: String = chars.iter().skip(chars.len() - 3).collect();
    format!("{head}…{tail}")
}

pub fn display_address(address: &str) -> String {
    let value = address.trim();
    match classify_address(value) {
        Some(RecordKind::Tailcat) => short_address(value),
        Some(RecordKind::Lan) => lan_ip(value).unwrap_or_else(|| value.to_owned()),
        Some(RecordKind::Legacy) => {
            if let Some(captures) = legacy_port_re().captures(value) {
                return captures.get(1).unwrap().as_str().to_owned();
            }
            if user_host_re().is_match(value) {
                if let Some(at) = value.rfind('@') {
                    return value[at + 1..].to_owned();
                }
            }
            value.to_owned()
        }
        None => value.to_owned(),
    }
}

pub fn record_badge(address: &str) -> Option<&'static str> {
    match classify_address(address) {
        Some(RecordKind::Lan | RecordKind::Legacy) => Some(LAN_BADGE),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrTapAction {
    Reveal,
    Enlarge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMask {
    obscured: bool,
    connected_count: i32,
}

impl Default for QrMask {
    fn default() -> Self {
        Self {
            obscured: false,
            connected_count: 0,
        }
    }
}

impl QrMask {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_state(obscured: bool, connected_count: i32) -> Self {
        Self {
            obscured,
            connected_count: connected_count.max(0),
        }
    }

    pub fn obscured(&self) -> bool {
        self.obscured
    }

    pub fn connected_count(&self) -> i32 {
        self.connected_count
    }

    pub fn shows_tap_to_show(&self) -> bool {
        self.obscured
    }

    pub fn on_connected_count(&mut self, count: i32) {
        let next = count.max(0);
        if self.connected_count == 0 && next > 0 {
            self.obscured = true;
        }
        self.connected_count = next;
    }

    pub fn show(&mut self) {
        self.obscured = false;
    }

    pub fn hide(&mut self) {
        self.obscured = true;
    }

    pub fn on_qr_tap(&mut self) -> QrTapAction {
        if self.obscured {
            self.show();
            QrTapAction::Reveal
        } else {
            QrTapAction::Enlarge
        }
    }
}

fn sorted(
    records: &[ConnectionRecord],
    include: impl Fn(&ConnectionRecord) -> bool,
) -> Vec<ConnectionRecord> {
    let mut items: Vec<ConnectionRecord> = records.iter().filter(|record| include(record)).cloned().collect();
    items.sort_by(|a, b| {
        b.connected_at_millis
            .cmp(&a.connected_at_millis)
            .then_with(|| a.id.cmp(&b.id))
    });
    items
}

fn json_string(value: &Value, key: &str) -> Result<String, RecordError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(RecordError::InvalidRecord)
}

fn lan_ip(value: &str) -> Option<String> {
    if let Some(captures) = lan_re().captures(value) {
        return captures.get(1).map(|item| item.as_str().to_owned());
    }
    if ipv4_re().is_match(value) {
        return Some(value.to_owned());
    }
    None
}

fn tailcat_re() -> &'static Regex {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^tc[A-Za-z0-9_-]+$").unwrap();
    }
    &RE
}

fn ipv4_re() -> &'static Regex {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"^(?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}$"
        )
        .unwrap();
    }
    &RE
}

fn lan_re() -> &'static Regex {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"^lan:((?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}):(\d+)/([0-9a-fA-F]+)$"
        )
        .unwrap();
    }
    &RE
}

fn legacy_port_re() -> &'static Regex {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"^((?:25[0-5]|2[0-4]\d|1?\d?\d)(?:\.(?:25[0-5]|2[0-4]\d|1?\d?\d)){3}):(\d+)$"
        )
        .unwrap();
    }
    &RE
}

fn user_host_re() -> &'static Regex {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^[^@\s]+@[^@\s:]+$").unwrap();
    }
    &RE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../flutter/test/fixtures/sealdesk_pr2_records.json"))
            .unwrap()
    }

    fn text<'a>(value: &'a Value, key: &str) -> &'a str {
        value.get(key).and_then(Value::as_str).unwrap()
    }

    #[test]
    fn classifies_shortens_and_keeps_legacy_ids() {
        let fixture = fixture();
        let tailcat = text(&fixture, "tailcat");
        assert!(tailcat.len() > 40);
        assert_eq!(classify_address(tailcat), Some(RecordKind::Tailcat));
        assert_eq!(short_address(tailcat), text(&fixture, "tailcat_short"));
        assert_eq!(display_address(tailcat), text(&fixture, "tailcat_short"));
        assert_eq!(record_badge(tailcat), None);
        assert_eq!(record_id(tailcat).unwrap(), tailcat);
        assert_eq!(short_address("tcshort"), "tcshort");
        assert_eq!(short_address("123456789012"), "123456789012");
        assert_eq!(short_address("1234567890123"), "123456…123");

        let lan_url = text(&fixture, "lan_url");
        assert_eq!(classify_address(lan_url), Some(RecordKind::Lan));
        assert_eq!(record_id(lan_url).unwrap(), text(&fixture, "lan_id"));
        assert_eq!(display_address(lan_url), text(&fixture, "lan_ip"));
        assert_eq!(record_badge(lan_url), Some(LAN_BADGE));
        assert_eq!(
            record_id(text(&fixture, "plain_ip")).unwrap(),
            format!("lan:{}", text(&fixture, "plain_ip"))
        );

        let legacy_port = text(&fixture, "legacy_port");
        assert_eq!(classify_address(legacy_port), Some(RecordKind::Legacy));
        assert_eq!(record_id(legacy_port).unwrap(), legacy_port);
        assert_eq!(display_address(legacy_port), "192.168.31.20");
        let legacy_user = text(&fixture, "legacy_user");
        assert_eq!(record_id(legacy_user).unwrap(), legacy_user);
        assert_eq!(display_address(legacy_user), text(&fixture, "legacy_user_display"));
        assert_eq!(record_badge(legacy_user), Some(LAN_BADGE));
        assert_eq!(classify_address("SEALDESK:V:1;"), None);
        assert!(record_id("not an address").is_err());
    }

    #[test]
    fn lan_and_tc_stay_distinct_and_legacy_is_not_migrated() {
        let fixture = fixture();
        let mut store = ConnectionRecordStore::default();
        let tailcat = text(&fixture, "tailcat");
        let mut event = ConnectEvent::new(tailcat, 10);
        event.hostname = "mini".into();
        store.upsert(event).unwrap();
        let mut event = ConnectEvent::new(text(&fixture, "lan_url"), 20);
        event.hostname = "mini".into();
        store.upsert(event).unwrap();
        let mut event = ConnectEvent::new(text(&fixture, "legacy_port"), 5);
        event.hostname = "old-pc".into();
        store.upsert(event).unwrap();
        store
            .upsert(ConnectEvent::new(
                format!("  {}  ", text(&fixture, "legacy_user")),
                6,
            ))
            .unwrap();

        let recent = store.list_recent();
        let ids: Vec<&str> = recent.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                text(&fixture, "lan_id"),
                tailcat,
                text(&fixture, "legacy_user"),
                text(&fixture, "legacy_port"),
            ]
        );
        assert_eq!(store.list_recent()[0].display_address(), text(&fixture, "lan_ip"));
        assert_eq!(store.list_recent()[0].badge(), Some("LAN"));
        assert_eq!(store.list_recent()[1].alias, "mini");
        assert!(store.get("lan:192.168.31.20").is_none());

        store
            .upsert(ConnectEvent::new(text(&fixture, "lan_url_rotated_key"), 30))
            .unwrap();
        let lan_count = store
            .list_recent()
            .iter()
            .filter(|record| record.kind == RecordKind::Lan)
            .count();
        assert_eq!(lan_count, 1);
        assert_eq!(
            store.get(text(&fixture, "lan_id")).unwrap().address,
            text(&fixture, "lan_url_rotated_key")
        );
    }

    #[test]
    fn sorts_newest_first_then_id() {
        let mut store = ConnectionRecordStore::default();
        store.upsert(ConnectEvent::new("10.0.0.2", 10)).unwrap();
        store.upsert(ConnectEvent::new("10.0.0.1", 10)).unwrap();
        store.upsert(ConnectEvent::new("10.1.0.1", 11)).unwrap();
        let recent = store.list_recent();
        let ids: Vec<&str> = recent.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids, vec!["lan:10.1.0.1", "lan:10.0.0.1", "lan:10.0.0.2"]);
    }

    #[test]
    fn password_is_stored_only_when_remembered() {
        assert_eq!(password_to_store(false, "secret"), "");
        assert_eq!(password_to_store(true, "secret"), "secret");

        let fixture = fixture();
        let mut store = ConnectionRecordStore::default();
        let mut event = ConnectEvent::new(text(&fixture, "lan_url"), 1);
        event.password = "secret".into();
        event.remember_password = true;
        store.upsert(event).unwrap();
        assert_eq!(store.get(text(&fixture, "lan_id")).unwrap().password, "secret");

        let mut event = ConnectEvent::new(text(&fixture, "lan_url"), 2);
        event.password = "temp".into();
        store.upsert(event).unwrap();
        let record = store.get(text(&fixture, "lan_id")).unwrap();
        assert!(record.password.is_empty());
        assert!(record.to_json().get("password").is_none());

        let mut event = ConnectEvent::new(text(&fixture, "lan_url"), 3);
        event.password = "secret".into();
        event.remember_password = true;
        store.upsert(event).unwrap();
        let mut event = ConnectEvent::new(text(&fixture, "lan_url"), 4);
        event.password = "secret".into();
        store.upsert(event).unwrap();
        assert!(store.get(text(&fixture, "lan_id")).unwrap().password.is_empty());
    }

    #[test]
    fn alias_defaults_to_hostname_and_edits_stick() {
        let tailcat = text(&fixture(), "tailcat").to_owned();
        let mut store = ConnectionRecordStore::default();
        let mut event = ConnectEvent::new(tailcat.clone(), 1);
        event.hostname = "mini".into();
        store.upsert(event).unwrap();
        assert_eq!(store.get(&tailcat).unwrap().alias, "mini");
        let mut event = ConnectEvent::new(tailcat.clone(), 2);
        event.hostname = "mini".into();
        event.alias = "living-room".into();
        store.upsert(event).unwrap();
        let mut event = ConnectEvent::new(tailcat.clone(), 3);
        event.hostname = "renamed".into();
        store.upsert(event).unwrap();
        let record = store.get(&tailcat).unwrap();
        assert_eq!(record.alias, "living-room");
        assert_eq!(record.hostname, "renamed");
    }

    #[test]
    fn delete_and_clear_history_keep_favorites() {
        let fixture = fixture();
        let mut store = ConnectionRecordStore::default();
        let mut plain = ConnectEvent::new(text(&fixture, "plain_ip"), 1);
        plain.favorite = Some(true);
        store.upsert(plain).unwrap();
        store
            .upsert(ConnectEvent::new(text(&fixture, "legacy_port"), 2))
            .unwrap();
        let mut legacy = ConnectEvent::new(text(&fixture, "legacy_user"), 3);
        legacy.favorite = Some(true);
        store.upsert(legacy).unwrap();
        store
            .upsert(ConnectEvent::new(text(&fixture, "tailcat"), 4))
            .unwrap();

        assert!(!store.delete("missing"));
        assert!(store.delete(text(&fixture, "legacy_port")));
        assert!(store.get(text(&fixture, "legacy_port")).is_none());

        store.clear_history();
        assert!(store.list_recent().is_empty());
        let plain_id = format!("lan:{}", text(&fixture, "plain_ip"));
        let favorites = store.list_favorites();
        let favs: Vec<&str> = favorites.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(favs, vec![text(&fixture, "legacy_user"), plain_id.as_str()]);

        store
            .upsert(ConnectEvent::new(text(&fixture, "plain_ip"), 8))
            .unwrap();
        assert_eq!(store.list_recent().len(), 1);
        assert_eq!(store.list_favorites().len(), 2);
        assert!(store
            .get(&format!("lan:{}", text(&fixture, "plain_ip")))
            .unwrap()
            .favorite);
    }

    #[test]
    fn json_round_trip_omits_empty_password() {
        let tailcat = text(&fixture(), "tailcat").to_owned();
        let mut store = ConnectionRecordStore::default();
        let mut event = ConnectEvent::new(tailcat.clone(), 9);
        event.hostname = "mini".into();
        event.password = "temp".into();
        store.upsert(event).unwrap();
        let restored = ConnectionRecordStore::from_json(&store.to_json()).unwrap();
        let record = restored.get(&tailcat).unwrap();
        assert!(record.password.is_empty());
        assert!(record.to_json().get("password").is_none());
        assert_eq!(record.alias, "mini");
        assert!(record.in_history);
        assert_eq!(restored.to_json(), store.to_json());

        let mut bad = record.to_json();
        bad.as_object_mut().unwrap().insert("id".into(), json!("other"));
        assert!(ConnectionRecordStore::from_json(&json!({"records": [bad]})).is_err());
    }

    #[test]
    fn qr_mask_follows_zero_to_positive_only() {
        let mut mask = QrMask::new();
        assert!(!mask.obscured());
        assert!(!mask.shows_tap_to_show());
        mask.on_connected_count(0);
        assert!(!mask.obscured());
        mask.on_connected_count(1);
        assert!(mask.obscured());
        assert!(mask.shows_tap_to_show());
        assert_eq!(mask.connected_count(), 1);
        mask.on_connected_count(4);
        assert!(mask.obscured());
        mask.show();
        mask.on_connected_count(5);
        assert!(!mask.obscured());
        mask.on_connected_count(0);
        assert!(!mask.obscured());
        mask.on_connected_count(2);
        assert!(mask.obscured());
        mask.on_connected_count(-3);
        assert_eq!(mask.connected_count(), 0);
        assert!(mask.obscured());
    }

    #[test]
    fn qr_tap_reveals_then_enlarges() {
        let mut mask = QrMask::new();
        mask.on_connected_count(1);
        assert_eq!(mask.on_qr_tap(), QrTapAction::Reveal);
        assert!(!mask.obscured());
        assert_eq!(mask.on_qr_tap(), QrTapAction::Enlarge);
        assert!(!mask.obscured());
        mask.hide();
        assert!(mask.shows_tap_to_show());
        mask.on_connected_count(1);
        assert!(mask.obscured());
    }

    #[test]
    fn seeded_qr_count_is_not_a_transition() {
        let mut seeded = QrMask::with_state(false, 3);
        assert!(!seeded.obscured());
        seeded.on_connected_count(3);
        assert!(!seeded.obscured());
        let mut negative = QrMask::with_state(false, -1);
        assert_eq!(negative.connected_count(), 0);
        negative.on_connected_count(1);
        assert!(negative.obscured());
    }
}
