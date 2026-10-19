use hbb_common::regex::Regex;
use std::ops::Deref;

mod ar;
mod az;
mod be;
mod bg;
mod ca;
mod cn;
mod cs;
mod da;
mod de;
mod el;
mod en;
mod eo;
mod es;
mod et;
mod eu;
mod fa;
mod gu;
mod fr;
mod he;
mod hi;
mod hr;
mod hu;
mod id;
mod it;
mod ja;
mod ko;
mod kz;
mod lt;
mod lv;
mod nb;
mod nl;
mod pl;
mod pt_BR;
mod pt_PT;
mod ro;
mod ru;
mod sc;
mod sk;
mod sl;
mod sq;
mod sr;
mod sv;
mod th;
mod tr;
mod tw;
mod uk;
mod ur;
mod vi;
mod ta;
mod ge;
mod fi;
mod ml;
mod gl;

pub const LANGS: &[(&str, &str)] = &[
    ("en", "English"),
    ("it", "Italiano"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("nl", "Nederlands"),
    ("nb", "Norsk bokmål"),
    ("zh-cn", "简体中文"),
    ("zh-tw", "繁體中文"),
    ("pt-pt", "Português (Portugal)"),
    ("pt-br", "Português (Brasil)"),
    ("es", "Español"),
    ("et", "Eesti keel"),
    ("eu", "Euskara"),
    ("hu", "Magyar"),
    ("bg", "Български"),
    ("be", "Беларуская"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("id", "Indonesia"),
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("eo", "Esperanto"),
    ("tr", "Türkçe"),
    ("vi", "Tiếng Việt"),
    ("pl", "Polski"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("kz", "Қазақ"),
    ("uk", "Українська"),
    ("ur", "اردو"),
    ("fa", "فارسی"),
    ("ca", "Català"),
    ("gl", "Galego"),
    ("el", "Ελληνικά"),
    ("sv", "Svenska"),
    ("sq", "Shqip"),
    ("sr", "Srpski"),
    ("th", "ภาษาไทย"),
    ("sl", "Slovenščina"),
    ("ro", "Română"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("ar", "العربية"),
    ("he", "עברית"),
    ("hr", "Hrvatski"),
    ("sc", "Sardu"),
    ("ta", "தமிழ்"),
    ("ge", "ქართული"),
    ("fi", "Suomi"),
    ("ml", "മലയാളം"),
    ("hi", "हिंदी"),
    ("gu", "ગુજરાતી"),
    ("az", "Azərbaycan dili"),
];

pub(crate) fn cjk_ui_unavailable() -> bool {
    cfg!(all(
        target_os = "linux",
        target_arch = "aarch64",
        feature = "flutter"
    ))
}

pub(crate) fn is_cjk_lang(lang_or_locale: &str) -> bool {
    let lang = lang_or_locale
        .split(|c| c == '-' || c == '_')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    matches!(lang.as_str(), "zh" | "ja" | "ko")
}

fn resolve_lang(saved_lang: &str, locale: &str, cjk_fallback: bool) -> String {
    let locale = locale.to_lowercase();
    let mut lang = saved_lang.to_lowercase();
    if cjk_fallback && is_cjk_lang(&lang) {
        return "en".to_owned();
    }
    if lang.is_empty() {
        // zh_CN on Linux, zh-Hans-CN on mac, zh_CN_#Hans on Android
        if locale.starts_with("zh") {
            lang = (if locale.contains("tw") {
                "zh-tw"
            } else {
                "zh-cn"
            })
            .to_owned();
        }
    }
    if lang.is_empty() {
        // pt_PT on Linux, pt-PT on mac, pt_PT on Android.
        // Per CLDR locale inheritance, every Portuguese-speaking locale
        // besides bare "pt" and Brazil's own variants (pt-BR/pt_BR) has
        // pt-PT as its parent (Angola, Mozambique, Cape Verde, etc.),
        // so it should resolve to European Portuguese.
        if locale.starts_with("pt") {
            lang = (if locale == "pt" || locale.starts_with("pt-br") || locale.starts_with("pt_br") {
                "pt-br"
            } else {
                "pt-pt"
            })
            .to_owned();
        }
    }
    if lang.is_empty() {
        lang = locale
            .split("-")
            .next()
            .map(|x| x.split("_").next().unwrap_or_default())
            .unwrap_or_default()
            .to_owned();
    }
    if cjk_fallback && is_cjk_lang(&lang) {
        "en".to_owned()
    } else {
        lang
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
    let lang = resolve_lang(
        &hbb_common::config::LocalConfig::get_option("lang"),
        locale,
        cjk_ui_unavailable(),
    );
    let m = match lang.as_str() {
        "fr" => fr::T.deref(),
        "zh-cn" => cn::T.deref(),
        "it" => it::T.deref(),
        "zh-tw" => tw::T.deref(),
        "de" => de::T.deref(),
        "nb" => nb::T.deref(),
        "nl" => nl::T.deref(),
        "es" => es::T.deref(),
        "et" => et::T.deref(),
        "eu" => eu::T.deref(),
        "hu" => hu::T.deref(),
        "ru" => ru::T.deref(),
        "eo" => eo::T.deref(),
        "id" => id::T.deref(),
        "br" => pt_BR::T.deref(),
        "pt" => pt_BR::T.deref(),
        "pt-br" => pt_BR::T.deref(),
        "pt-pt" => pt_PT::T.deref(),
        "tr" => tr::T.deref(),
        "cs" => cs::T.deref(),
        "da" => da::T.deref(),
        "sk" => sk::T.deref(),
        "vi" => vi::T.deref(),
        "pl" => pl::T.deref(),
        "ja" => ja::T.deref(),
        "ko" => ko::T.deref(),
        "kz" => kz::T.deref(),
        "uk" => uk::T.deref(),
        "fa" => fa::T.deref(),
        "fi" => fi::T.deref(),
        "ca" => ca::T.deref(),
        "el" => el::T.deref(),
        "sv" => sv::T.deref(),
        "sq" => sq::T.deref(),
        "sr" => sr::T.deref(),
        "th" => th::T.deref(),
        "sl" => sl::T.deref(),
        "ro" => ro::T.deref(),
        "lt" => lt::T.deref(),
        "lv" => lv::T.deref(),
        "ar" => ar::T.deref(),
        "bg" => bg::T.deref(),
        "be" => be::T.deref(),
        "he" => he::T.deref(),
        "hr" => hr::T.deref(),
        "ur" => ur::T.deref(),
        "sc" => sc::T.deref(),
        "ta" => ta::T.deref(),
        "ge" => ge::T.deref(),
        "ml" => ml::T.deref(),
        "hi" => hi::T.deref(),
        "gu" => gu::T.deref(),
        "gl" => gl::T.deref(),
        "az" => az::T.deref(),
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
    fn test_resolve_lang_forces_english_for_cjk_locale_when_target_disables_cjk() {
        use super::resolve_lang as f;

        assert_eq!(f("", "zh_CN", true), "en");
        assert_eq!(f("", "ja-JP", true), "en");
        assert_eq!(f("", "ko_KR", true), "en");
    }

    #[test]
    fn test_resolve_lang_preserves_cjk_when_target_allows_cjk() {
        use super::resolve_lang as f;

        assert_eq!(f("zh-cn", "en-US", false), "zh-cn");
        assert_eq!(f("", "zh_TW", false), "zh-tw");
        assert_eq!(f("", "ja-JP", false), "ja");
    }

    #[test]
    fn pr2_compat_copy_keeps_the_rustdesk_protocol_name() {
        let compat = "Compatible with RustDesk \u{b7} Unencrypted";
        let hint = "This device uses RustDesk compatibility mode and is unencrypted on the LAN.";
        assert_eq!(super::apply_branding(compat, compat), compat);
        assert_eq!(super::apply_branding("compat-lan-hint", hint), hint);
        assert!(
            !super::apply_branding("compat-lan-hint", hint).contains("SealDesk")
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
    fn test_resolve_lang_detects_pt_pt_and_pt_br_from_locale() {
        use super::resolve_lang as f;

        assert_eq!(f("", "pt-PT", false), "pt-pt");
        assert_eq!(f("", "pt_PT", false), "pt-pt");
        assert_eq!(f("", "pt-BR", false), "pt-br");
        assert_eq!(f("", "pt_BR", false), "pt-br");
        assert_eq!(f("", "pt-AO", false), "pt-pt");
        assert_eq!(f("", "pt-MZ", false), "pt-pt");
    }
}
