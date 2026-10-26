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

    pub fn cloud_error(self, error: &octoterm_cloud_client::CloudError) -> String {
        use octoterm_cloud_client::CloudError;
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
}
