/// macOS Privacy & Security deep links for the three host TCC prompts.
///
/// macOS 13+ uses the System Settings extension anchor. Older releases keep
/// the legacy preference URL, which is also the fallback if the extension
/// URL fails to open.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacyKind {
    Screen,
    Accessibility,
    Input,
}

impl PrivacyKind {
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "screen" => Some(Self::Screen),
            "accessibility" => Some(Self::Accessibility),
            "input" => Some(Self::Input),
            _ => None,
        }
    }

    pub fn anchor(self) -> &'static str {
        match self {
            Self::Screen => "Privacy_ScreenCapture",
            Self::Accessibility => "Privacy_Accessibility",
            Self::Input => "Privacy_ListenEvent",
        }
    }

    /// Stable code shared with the Swift guide panel.
    pub fn code(self) -> i32 {
        match self {
            Self::Screen => 0,
            Self::Accessibility => 1,
            Self::Input => 2,
        }
    }
}

pub fn privacy_settings_urls(major: u32, anchor: &str) -> Vec<String> {
    let legacy = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    if major >= 13 {
        vec![
            format!(
                "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?{anchor}"
            ),
            legacy,
        ]
    } else {
        vec![legacy]
    }
}

#[cfg(test)]
mod tests {
    use super::{privacy_settings_urls, PrivacyKind};

    #[test]
    fn macos_13_uses_extension_url_then_legacy_fallback() {
        for (kind, anchor) in [
            (PrivacyKind::Screen, "Privacy_ScreenCapture"),
            (PrivacyKind::Accessibility, "Privacy_Accessibility"),
            (PrivacyKind::Input, "Privacy_ListenEvent"),
        ] {
            assert_eq!(kind.anchor(), anchor);
            let urls = privacy_settings_urls(13, anchor);
            assert_eq!(
                urls,
                vec![
                    format!(
                        "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?{anchor}"
                    ),
                    format!("x-apple.systempreferences:com.apple.preference.security?{anchor}"),
                ]
            );
            assert_eq!(privacy_settings_urls(15, anchor), urls);
        }
    }

    #[test]
    fn older_than_macos_13_keeps_legacy_url_only() {
        let urls = privacy_settings_urls(12, PrivacyKind::Screen.anchor());
        assert_eq!(
            urls,
            vec![
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
                    .to_owned()
            ]
        );
        assert_eq!(
            privacy_settings_urls(11, PrivacyKind::Input.anchor()).len(),
            1
        );
    }

    #[test]
    fn kind_codes_match_the_guide_panel() {
        assert_eq!(PrivacyKind::parse("screen").unwrap().code(), 0);
        assert_eq!(PrivacyKind::parse("accessibility").unwrap().code(), 1);
        assert_eq!(PrivacyKind::parse("input").unwrap().code(), 2);
        assert!(PrivacyKind::parse("Privacy_ScreenCapture").is_none());
        assert!(PrivacyKind::parse("").is_none());
    }
}
