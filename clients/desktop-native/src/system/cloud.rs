//! Cloud account form, relay region presets, and async Cloud operations (`Login`, `Logout`,
//! `SendDeleteCode`, `DeleteAccount`), aligned with `clients/desktop-web/src/settings/cloud.rs`
//! and `clients/desktop-web/src/cloud_ops.rs`.

use std::path::PathBuf;

use gateterm_cloud_client::{
    Credentials,
    ops::{self, OpsError},
};

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
            Self::China => locale.t("中国大陆地区", "Mainland China"),
            Self::NorthAmerica => locale.t("北美地区", "North America"),
            Self::Global => locale.t("其它全球", "Global"),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudAccountStatus {
    LoggedOut,
    LoggedIn { email: String, url: String },
}

pub fn load_local_cloud_creds() -> Option<Credentials> {
    let path = gateterm_cloud_client::credentials::default_path()?;
    gateterm_cloud_client::credentials::load(&path)
        .ok()
        .flatten()
}

pub fn status_from_local_creds() -> CloudAccountStatus {
    match load_local_cloud_creds() {
        Some(c) => CloudAccountStatus::LoggedIn {
            email: c.email,
            url: c.url,
        },
        None => CloudAccountStatus::LoggedOut,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudJob {
    BrowserLogin { url: String },
    Logout,
    RequestDeleteCode,
    DeleteAccount { password: String, code: String },
}

pub fn prepare_cloud_job(
    region: RelayRegion,
    password: &str,
    code: &str,
    confirm_delete: bool,
    status: &CloudAccountStatus,
    action: CloudAction,
    locale: Locale,
) -> Result<CloudJob, String> {
    use CloudAction::*;
    let logged_in = matches!(status, CloudAccountStatus::LoggedIn { .. });
    if matches!(action, Login | CancelLogin) == logged_in {
        return Err(if logged_in {
            locale
                .t("已登录，请先退出登录", "Already signed in. Sign out first.")
                .into()
        } else {
            locale.t("当前未登录", "Not signed in").into()
        });
    }
    Ok(match action {
        Login => CloudJob::BrowserLogin {
            url: region.url().into(),
        },
        CancelLogin => {
            return Err(locale
                .t("没有正在进行的登录", "No sign-in is in progress")
                .into());
        }
        Logout => CloudJob::Logout,
        SendDeleteCode => CloudJob::RequestDeleteCode,
        DeleteAccount => {
            if !confirm_delete {
                return Err(locale
                    .t(
                        "请先勾选「我明白删除后无法恢复」",
                        "Confirm that you understand deletion cannot be undone.",
                    )
                    .into());
            }
            if password.is_empty() {
                return Err(locale.t("请填写密码", "Enter your password").into());
            }
            let code = code.trim();
            if code.len() != 6 || !code.bytes().all(|c| c.is_ascii_digit()) {
                return Err(locale
                    .t(
                        "请填写邮件里的 6 位验证码",
                        "Enter the six-digit code from your email",
                    )
                    .into());
            }
            CloudJob::DeleteAccount {
                password: password.to_string(),
                code: code.into(),
            }
        }
    })
}

fn format_ops_error(e: OpsError, locale: Locale) -> String {
    match e {
        OpsError::Cloud(err) => err.to_string(),
        OpsError::NotLoggedIn => locale
            .t("当前未登录 Cloud", "Not signed in to Cloud")
            .into(),
        OpsError::Local(msg) => msg,
    }
}

pub async fn run_cloud_job(job: CloudJob, path: PathBuf, locale: Locale) -> Result<String, String> {
    let label = gateterm_cloud_client::default_label();
    match job {
        CloudJob::BrowserLogin { url } => {
            let login = super::browser_login::BrowserLogin::bind(&url, &label, locale).await?;
            open::that_detached(login.url()).map_err(|e| {
                format!(
                    "{}: {e}",
                    locale.t("无法打开浏览器", "Could not open the browser")
                )
            })?;
            let c = login.complete(&path).await?;
            Ok(format!(
                "{} {}",
                locale.t("已登录为", "Signed in as"),
                c.email
            ))
        }
        CloudJob::Logout => match ops::logout(&path)
            .await
            .map_err(|e| format_ops_error(e, locale))?
        {
            None => Ok(locale.t("已退出登录", "Signed out").into()),
            Some(e) => Ok(format!(
                "{} ({e})",
                locale.t(
                    "已在本机退出登录",
                    "Signed out locally (could not notify Cloud)"
                )
            )),
        },
        CloudJob::RequestDeleteCode => {
            let email = ops::request_delete_code(&path)
                .await
                .map_err(|e| format_ops_error(e, locale))?;
            Ok(format!(
                "{} {email}",
                locale.t("确认码已发送到", "Confirmation code sent to")
            ))
        }
        CloudJob::DeleteAccount { password, code } => {
            ops::delete_account(&path, &password, &code)
                .await
                .map_err(|e| format_ops_error(e, locale))?;
            Ok(locale.t("账号已删除", "Account deleted").into())
        }
    }
}
