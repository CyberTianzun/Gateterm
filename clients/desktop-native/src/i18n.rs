use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LanguagePref {
    #[default]
    Auto,
    #[serde(rename = "zh-CN", alias = "zh")]
    ZhCn,
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    ZhCn,
    En,
}

impl Locale {
    pub fn system() -> Self {
        let tag = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
        if tag.starts_with("zh") {
            Self::ZhCn
        } else {
            Self::En
        }
    }

    pub fn is_zh(self) -> bool {
        matches!(self, Self::ZhCn)
    }

    pub fn pick<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::ZhCn => zh,
            Self::En => en,
        }
    }

    pub fn t<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        self.pick(zh, en)
    }
}

impl LanguagePref {
    pub const ALL: [Self; 3] = [Self::Auto, Self::ZhCn, Self::En];

    pub fn resolve(self) -> Locale {
        match self {
            Self::Auto => Locale::system(),
            Self::ZhCn => Locale::ZhCn,
            Self::En => Locale::En,
        }
    }

    pub fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::Auto => locale.pick("跟随系统", "System Default"),
            Self::ZhCn => "简体中文",
            Self::En => "English",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_pref_resolves_explicit_locales() {
        assert_eq!(LanguagePref::ZhCn.resolve(), Locale::ZhCn);
        assert_eq!(LanguagePref::En.resolve(), Locale::En);
        assert_eq!(Locale::ZhCn.pick("终端", "Terminal"), "终端");
        assert_eq!(Locale::En.pick("终端", "Terminal"), "Terminal");
    }
}
