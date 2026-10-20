use hbb_common::regex::Regex;
use std::ops::Deref;

mod cn;
mod en;

pub const LANGS: &[(&str, &str)] = &[("zh-cn", "中文"), ("en", "English")];

fn is_zh(lang_or_locale: &str) -> bool {
    lang_or_locale
        .split(|c| c == '-' || c == '_')
        .next()
        .unwrap_or_default()
        .eq_ignore_ascii_case("zh")
}

// Saved "zh-cn" or "en" is used as-is. Any other value, including empty,
// "default", and retired tags, follows the system locale: zh* -> zh-cn,
// otherwise English.
fn resolve_lang(saved_lang: &str, locale: &str) -> String {
    let saved = saved_lang.trim().to_lowercase();
    let source = if saved == "zh-cn" || saved == "en" {
        saved.as_str()
    } else {
        locale
    };
    if is_zh(source) {
        "zh-cn".to_owned()
    } else {
        "en".to_owned()
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn translate(name: String) -> String {
    let locale = sys_locale::get_locale().unwrap_or_default();
    translate_locale(name, &locale)
}

fn preserves_literal_rustdesk(name: &str) -> bool {
    name.starts_with("upgrade_rustdesk_server_pro")
        || matches!(
            name,
            "powered_by_me"
                | "Allow RustDesk compatible connections"
                | "Compatible with RustDesk · Unencrypted"
                | "compat-lan-hint"
                | "compat-lan-enable-warning"
        )
}

fn apply_branding(name: &str, text: &str) -> String {
    if crate::is_rustdesk() || !text.contains("RustDesk") || preserves_literal_rustdesk(name) {
        return text.to_owned();
    }
    let app_name = crate::get_app_name();
    if !app_name.contains("RustDesk") {
        return text.replace("RustDesk", &app_name);
    }
    // https://github.com/rustdesk/rustdesk-server-pro/issues/845
    // If app_name contains "RustDesk" (e.g., "RustDesk-Admin"), avoid replacing
    // "RustDesk" inside the already-substituted app name.
    const PLACEHOLDER: &str = "#A-P-P-N-A-M-E#";
    if text.contains(PLACEHOLDER) {
        return text.to_owned();
    }
    text.replace(&app_name, PLACEHOLDER)
        .replace("RustDesk", &app_name)
        .replace(PLACEHOLDER, &app_name)
}

pub fn translate_locale(name: String, locale: &str) -> String {
    let lang = resolve_lang(&hbb_common::config::LocalConfig::get_option("lang"), locale);
    let m = match lang.as_str() {
        "zh-cn" => cn::T.deref(),
        _ => en::T.deref(),
    };
    let (name, placeholder_value) = extract_placeholder(&name);
    let replace = |s: &&str| {
        let mut s = s.to_string();
        if let Some(value) = placeholder_value.as_ref() {
            s = s.replace("{}", &value);
        }
        apply_branding(&name, &s)
    };
    if let Some(v) = m.get(&name as &str) {
        if !v.is_empty() {
            return replace(v);
        }
    }
    if lang != "en" {
        if let Some(v) = en::T.get(&name as &str) {
            if !v.is_empty() {
                return replace(v);
            }
        }
    }
    replace(&name.as_str())
}

// Matching pattern is {}
// Write {value} in the UI and {} in the translation file
//
// Example:
// Write in the UI: translate("There are {24} hours in a day")
// Write in the translation file: ("There are {} hours in a day", "{} hours make up a day")
fn extract_placeholder(input: &str) -> (String, Option<String>) {
    if let Ok(re) = Regex::new(r#"\{(.*?)\}"#) {
        if let Some(captures) = re.captures(input) {
            if let Some(inner_match) = captures.get(1) {
                let name = re.replace(input, "{}").to_string();
                let value = inner_match.as_str().to_string();
                return (name, Some(value));
            }
        }
    }
    (input.to_string(), None)
}

mod test {
    #[test]
    fn test_extract_placeholders() {
        use super::extract_placeholder as f;

        assert_eq!(f(""), ("".to_string(), None));
        assert_eq!(
            f("{3} sessions"),
            ("{} sessions".to_string(), Some("3".to_string()))
        );
        assert_eq!(f(" } { "), (" } { ".to_string(), None));
        // Allow empty value
        assert_eq!(
            f("{} sessions"),
            ("{} sessions".to_string(), Some("".to_string()))
        );
        // Match only the first one
        assert_eq!(
            f("{2} times {4} makes {8}"),
            ("{} times {4} makes {8}".to_string(), Some("2".to_string()))
        );
    }

    #[test]
    fn test_resolve_lang_uses_chinese_for_zh_and_english_otherwise() {
        use super::resolve_lang as f;

        assert_eq!(f("", "zh_CN"), "zh-cn");
        assert_eq!(f("", "zh-Hans-CN"), "zh-cn");
        assert_eq!(f("", "zh_CN_#Hans"), "zh-cn");
        assert_eq!(f("", "zh_TW"), "zh-cn");
        assert_eq!(f("", "zh-Hant-TW"), "zh-cn");
        assert_eq!(f("", "en-US"), "en");
        assert_eq!(f("", "ja-JP"), "en");
        assert_eq!(f("", "pt-BR"), "en");
        assert_eq!(f("zh-cn", "en-US"), "zh-cn");
        assert_eq!(f("ZH-CN", "en-US"), "zh-cn");
        assert_eq!(f("en", "zh_CN"), "en");
        assert_eq!(f("de", "zh_CN"), "zh-cn");
        assert_eq!(f("de", "en-US"), "en");
        assert_eq!(f("ja", "zh_CN"), "zh-cn");
        assert_eq!(f("ja", "en-US"), "en");
        assert_eq!(f("zh-tw", "en-US"), "en");
        assert_eq!(f("zh-tw", "zh_CN"), "zh-cn");
        assert_eq!(f("zh_TW", "en-US"), "en");
        assert_eq!(f("zh_TW", "zh_TW"), "zh-cn");
        assert_eq!(f("fr", "zh_CN"), "zh-cn");
        assert_eq!(f("pt", "zh_CN"), "zh-cn");
        assert_eq!(f("pt-br", "en-US"), "en");
        assert_eq!(f("default", "zh_CN"), "zh-cn");
        assert_eq!(f("default", "en-US"), "en");
    }

    #[test]
    fn pr2_compat_copy_keeps_the_rustdesk_protocol_name() {
        let compat = "Compatible with RustDesk \u{b7} Unencrypted";
        let hint = "This device uses RustDesk compatibility mode and is unencrypted on the LAN.";
        assert_eq!(super::apply_branding(compat, compat), compat);
        assert_eq!(super::apply_branding("compat-lan-hint", hint), hint);
        assert!(!super::apply_branding("compat-lan-hint", hint).contains("SealDesk"));
        let warning = "RustDesk compatible LAN connections are not encrypted. Anyone on the local network can read this session. Enable only for old RustDesk clients.";
        assert_eq!(
            super::apply_branding("compat-lan-enable-warning", warning),
            warning
        );
        let branded = super::apply_branding("install_tip", "Grant RustDesk access");
        assert_eq!(branded, "Grant SealDesk access");
    }

    #[test]
    fn pr2_i18n_keys_exist_for_every_language() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lang");
        for key in crate::sealdesk_records::PR2_I18N_KEYS {
            let needle = format!("(\"{key}\",");
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(text.contains(&needle), "{} missing {key}", path.display());
            }
        }
        assert_eq!(
            super::cn::T.get("No records").copied(),
            Some("\u{6682}\u{65e0}\u{8bb0}\u{5f55}")
        );
        assert_eq!(
            super::en::T.get("Searching\u{2026}").copied(),
            Some("Searching\u{2026}")
        );
        assert_eq!(
            super::cn::T.get("compat-lan-hint").copied(),
            Some("\u{8be5}\u{8bbe}\u{5907}\u{4f7f}\u{7528} RustDesk \u{517c}\u{5bb9}\u{6a21}\u{5f0f}\u{ff0c}\u{5c40}\u{57df}\u{7f51}\u{5185}\u{672a}\u{52a0}\u{5bc6}")
        );
        assert_eq!(
            super::cn::T.get("Tap to show").copied(),
            Some("\u{70b9}\u{6309}\u{663e}\u{793a}")
        );
        assert_eq!(
            super::cn::T.get("LAN").copied(),
            Some("\u{5c40}\u{57df}\u{7f51}")
        );
    }

    #[test]
    fn scroll_about_and_2fa_copy() {
        assert_eq!(super::en::T.get("ScrollAuto").copied(), Some("Auto scroll"));
        assert_eq!(super::en::T.get("Scrollbar").copied(), Some("Scroll bar"));
        assert_eq!(super::en::T.get("ScrollEdge").copied(), Some("Edge scroll"));
        assert_eq!(
            super::cn::T.get("ScrollAuto").copied(),
            Some("\u{81ea}\u{52a8}\u{6eda}\u{52a8}")
        );
        assert_eq!(
            super::cn::T.get("Scrollbar").copied(),
            Some("\u{6eda}\u{52a8}\u{6761}")
        );
        assert_eq!(
            super::cn::T.get("ScrollEdge").copied(),
            Some("\u{8fb9}\u{7f18}\u{6eda}\u{52a8}")
        );
        assert_eq!(
            super::cn::T.get("About SealDesk").copied(),
            Some("\u{5173}\u{4e8e} SealDesk")
        );
        assert!(super::en::T.get("About SealDesk").is_none());
        assert_eq!(
            super::cn::T.get("2FA").copied(),
            Some("\u{53cc}\u{91cd}\u{8ba4}\u{8bc1}")
        );
        assert!(super::en::T.get("2FA").is_none());
    }
}
