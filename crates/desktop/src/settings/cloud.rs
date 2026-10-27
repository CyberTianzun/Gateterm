//! Cloud account form and relay presets, independent of egui.

use gateterm_cloud_client::Credentials;
use crate::i18n::Locale;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RelayRegion {
    #[default]
    China,
    NorthAmerica,
    Global,
}

impl RelayRegion {
    pub const ALL: [Self; 3] = [Self::China, Self::NorthAmerica, Self::Global];

    pub fn label(self, locale: Locale) -> &'static str {
        match self {
            Self::China => locale.text("中国大陆地区", "Mainland China"),
            Self::NorthAmerica => locale.text("北美地区", "North America"),
            Self::Global => locale.text("其它全球", "Global"),
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Self::China => "https://gateterm.signal.top/",
            Self::NorthAmerica => "https://gateterm.eclosion.ai/",
            Self::Global => "https://global.gateterm.eclosion.ai/",
        }
    }

    pub fn from_url(url: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|r| r.url().trim_end_matches('/') == url.trim().trim_end_matches('/'))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudForm {
    pub region: RelayRegion,
    pub password: String,
    pub code: String,
    pub confirm_delete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudStatus {
    LoggedOut,
    LoggedIn { email: String, url: String },
}

pub fn status_from(creds: Option<Credentials>) -> CloudStatus {
    match creds {
        Some(c) => CloudStatus::LoggedIn {
            email: c.email,
            url: c.url,
        },
        None => CloudStatus::LoggedOut,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudAction {
    Login,
    CancelLogin,
    Logout,
    SendDeleteCode,
    DeleteAccount,
}

impl CloudAction {
    pub fn keeps_secrets(self) -> bool {
        self == Self::SendDeleteCode
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudJob {
    BrowserLogin { url: String },
    Logout,
    RequestDeleteCode,
    DeleteAccount { password: String, code: String },
}

pub fn prepare(
    form: &CloudForm,
    status: &CloudStatus,
    action: CloudAction,
    locale: Locale,
) -> Result<CloudJob, String> {
    use CloudAction::*;
    let logged_in = matches!(status, CloudStatus::LoggedIn { .. });
    if matches!(action, Login | CancelLogin) == logged_in {
        return Err(if logged_in {
            locale.text("已登录，请先退出登录", "Already signed in. Sign out first.").into()
        } else {
            locale.text("当前未登录", "Not signed in").into()
        });
    }
    Ok(match action {
        Login => CloudJob::BrowserLogin {
            url: form.region.url().into(),
        },
        CancelLogin => return Err(locale.text("没有正在进行的登录", "No sign-in is in progress").into()),
        Logout => CloudJob::Logout,
        SendDeleteCode => CloudJob::RequestDeleteCode,
        DeleteAccount => {
            if !form.confirm_delete {
                return Err(locale.text("请先勾选「我明白删除后无法恢复」", "Confirm that you understand deletion cannot be undone.").into());
            }
            if form.password.is_empty() {
                return Err(locale.text("请填写密码", "Enter your password").into());
            }
            let code = form.code.trim();
            if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
                return Err(locale.text("请填写邮件里的 6 位验证码", "Enter the six-digit code from your email").into());
            }
            CloudJob::DeleteAccount {
                password: form.password.clone(),
                code: code.into(),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_uses_presets_and_needs_no_account_fields() {
        let mut form = CloudForm::default();
        assert_eq!(form.region, RelayRegion::China);
        for region in RelayRegion::ALL {
            form.region = region;
            assert_eq!(
                prepare(&form, &CloudStatus::LoggedOut, CloudAction::Login, Locale::En),
                Ok(CloudJob::BrowserLogin {
                    url: region.url().into()
                })
            );
            assert_eq!(
                RelayRegion::from_url(region.url().trim_end_matches('/')),
                region
            );
        }
    }

    #[test]
    fn actions_require_matching_login_state() {
        let form = CloudForm::default();
        let status = CloudStatus::LoggedIn {
            email: "a@example.com".into(),
            url: RelayRegion::China.url().into(),
        };
        assert!(prepare(&form, &status, CloudAction::Login, Locale::En).is_err());
        assert!(prepare(&form, &CloudStatus::LoggedOut, CloudAction::Logout, Locale::En).is_err());
        assert_eq!(
            prepare(&form, &status, CloudAction::Logout, Locale::En),
            Ok(CloudJob::Logout)
        );
        assert!(prepare(&form, &status, CloudAction::DeleteAccount, Locale::En).is_err());
    }
}
