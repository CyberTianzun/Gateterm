//! Desktop language preferences. Resolution is pure; OS detection happens only
//! at startup or when applying the "follow system" preference.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    ZhCn,
}

impl Locale {
    pub fn from_system_tag(tag: Option<&str>) -> Self {
        let language = tag
            .unwrap_or("")
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or("");
        if language.eq_ignore_ascii_case("zh") {
            Self::ZhCn
        } else {
            Self::En
        }
    }

    pub fn system() -> Self {
        Self::from_system_tag(sys_locale::get_locale().as_deref())
    }

    /// Keep both translations beside each other, including punctuation.
    pub const fn text(self, chinese: &'static str, english: &'static str) -> &'static str {
        match self {
            Self::ZhCn => chinese,
            Self::En => english,
        }
    }

    pub fn cloud_error(self, error: &gateterm_cloud_client::CloudError) -> String {
        use gateterm_cloud_client::CloudError;
        if self == Self::ZhCn {
            return error.user_message();
        }
        match error {
            CloudError::Unreachable(reason) => format!("Cannot connect to Cloud: {reason}"),
            CloudError::Unauthorized => "Your Cloud sign-in has expired. Sign in again.".into(),
            CloudError::RateLimited {
                retry_after: Some(seconds),
            } => format!("Too many requests. Try again in {seconds} seconds."),
            CloudError::RateLimited { retry_after: None } => {
                "Too many requests. Try again later.".into()
            }
            CloudError::Api { code, .. } => match code.as_str() {
                "invalid_credentials" => "Incorrect email or password".into(),
                "invalid_code" => "The confirmation code is incorrect or expired".into(),
                "code_exhausted" => "Too many incorrect attempts. Request a new code.".into(),
                "email_taken" => "This email is already registered".into(),
                "weak_password" => "Password must contain 8–128 characters".into(),
                "invalid_email" => "Invalid email address".into(),
                "invalid_authorization_code" => {
                    "Browser authorization has expired. Sign in again.".into()
                }
                "wrong_password" => "Incorrect password".into(),
                other => format!("Cloud returned an error: {other}"),
            },
        }
    }

    pub fn apply_error(self, err: &gateterm_server::agent::apply::ApplyError) -> String {
        use gateterm_server::agent::apply::ApplyError;
        match err {
            ApplyError::Disabled => self
                .text(
                    "agent 集成的安装功能未启用（config.toml 中 agents.install_enabled = false）",
                    "Agent integration installing is disabled (agents.install_enabled = false in config.toml)",
                )
                .into(),
            ApplyError::Invalid { path, detail } => crate::tr!(
                self,
                "{} 配置无效，拒绝改写：{detail}",
                "{} is invalid; refused to modify: {detail}",
                path.display()
            ),
            ApplyError::Io { path, detail } => crate::tr!(
                self,
                "读写 {} 失败：{detail}",
                "Failed to read or write {}: {detail}",
                path.display()
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LanguagePref {
    #[default]
    Auto,
    En,
    ZhCn,
}

impl LanguagePref {
    pub const ALL: [Self; 3] = [Self::Auto, Self::En, Self::ZhCn];

    pub fn from_config(value: &str) -> Self {
        match value {
            "en" => Self::En,
            "zh-CN" => Self::ZhCn,
            _ => Self::Auto,
        }
    }

    pub const fn code(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::En => "en",
            Self::ZhCn => "zh-CN",
        }
    }

    pub fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::Auto => locale.text("跟随系统", "Follow system"),
            Self::En => "English",
            Self::ZhCn => "简体中文",
        }
    }

    pub const fn resolve(self, system: Locale) -> Locale {
        match self {
            Self::Auto => system,
            Self::En => Locale::En,
            Self::ZhCn => Locale::ZhCn,
        }
    }
}

/// Like format!, but both language branches are checked by the compiler.
#[macro_export]
macro_rules! tr {
    ($locale:expr, $zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        match $locale {
            $crate::i18n::Locale::ZhCn => format!($zh $(, $arg)*),
            $crate::i18n::Locale::En => format!($en $(, $arg)*),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_os_language_tags_and_falls_back_to_english() {
        for tag in ["zh", "zh-CN", "zh_TW.UTF-8", "zh-Hant-HK", "ZH_sg"] {
            assert_eq!(Locale::from_system_tag(Some(tag)), Locale::ZhCn);
        }
        for tag in [
            None,
            Some(""),
            Some("C"),
            Some("en-US"),
            Some("ja-JP"),
            Some("de_DE.UTF-8"),
        ] {
            assert_eq!(Locale::from_system_tag(tag), Locale::En);
        }
    }

    #[test]
    fn explicit_preference_overrides_system_and_auto_tracks_it() {
        for system in [Locale::En, Locale::ZhCn] {
            assert_eq!(LanguagePref::Auto.resolve(system), system);
            assert_eq!(LanguagePref::En.resolve(system), Locale::En);
            assert_eq!(LanguagePref::ZhCn.resolve(system), Locale::ZhCn);
        }
        for pref in LanguagePref::ALL {
            assert_eq!(LanguagePref::from_config(pref.code()), pref);
        }
        assert_eq!(
            LanguagePref::from_config("future-language"),
            LanguagePref::Auto
        );
    }

    #[test]
    fn apply_error_translates_all_variants_in_english_without_chinese() {
        use std::path::PathBuf;
        use gateterm_server::agent::apply::ApplyError;

        let cases = [
            ApplyError::Disabled,
            ApplyError::Invalid {
                path: PathBuf::from("/tmp/claude/settings.json"),
                detail: "expected colon at line 2".into(),
            },
            ApplyError::Io {
                path: PathBuf::from("/tmp/codex/hooks.json"),
                detail: "permission denied".into(),
            },
        ];

        for err in &cases {
            let en = Locale::En.apply_error(err);
            let zh = Locale::ZhCn.apply_error(err);
            assert!(!en.is_empty());
            assert!(!zh.is_empty());
            assert_ne!(en, zh, "expected distinct translations for {err:?}");
            assert!(
                !en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "English ApplyError leaked Chinese: {en}"
            );
        }

        let en_disabled = Locale::En.apply_error(&cases[0]);
        assert!(en_disabled.to_ascii_lowercase().contains("disabled"), "{en_disabled}");

        let en_invalid = Locale::En.apply_error(&cases[1]);
        assert!(en_invalid.contains("/tmp/claude/settings.json"), "{en_invalid}");
        assert!(en_invalid.contains("expected colon at line 2"), "{en_invalid}");

        let en_io = Locale::En.apply_error(&cases[2]);
        assert!(en_io.contains("/tmp/codex/hooks.json"), "{en_io}");
        assert!(en_io.contains("permission denied"), "{en_io}");
    }
}
