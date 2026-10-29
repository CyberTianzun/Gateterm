//! 在 tokio runtime 上执行设置窗口发起的 Cloud 操作,返回给用户看的一句话。

use std::path::PathBuf;

use gateterm_cloud_client::ops::{self, OpsError};

use crate::settings::cloud::CloudJob;
use crate::i18n::Locale;
use crate::tr;

fn msg(e: OpsError, locale: Locale) -> String {
    match e {
        OpsError::Cloud(error) => locale.cloud_error(&error),
        OpsError::NotLoggedIn => locale.text("当前未登录 Cloud", "Not signed in to Cloud").into(),
        OpsError::Local(message) => {
            let detail = message.strip_prefix("读写 cloud.json 失败:").unwrap_or(&message);
            tr!(locale, "读写 cloud.json 失败:{detail}", "Could not read or write cloud.json: {detail}")
        }
    }
}

pub async fn run(job: CloudJob, path: PathBuf, label: String, locale: Locale) -> Result<String, String> {
    match job {
        CloudJob::BrowserLogin { url } => {
            let login = crate::browser_login::BrowserLogin::bind(&url, &label, locale).await?;
            open::that_detached(login.url()).map_err(|e| tr!(locale, "无法打开浏览器：{e}", "Could not open the browser: {e}"))?;
            let c = login.complete(&path).await?;
            Ok(tr!(locale, "已登录为 {}", "Signed in as {}", c.email))
        }
        CloudJob::Logout => match ops::logout(&path).await.map_err(|e| msg(e, locale))? {
            None => Ok(locale.text("已退出登录", "Signed out").into()),
            Some(e) => Ok(tr!(locale, "已在本机退出登录(未能通知 Cloud:{})", "Signed out locally (could not notify Cloud: {})", locale.cloud_error(&e))),
        },
        CloudJob::RequestDeleteCode => {
            let email = ops::request_delete_code(&path).await.map_err(|e| msg(e, locale))?;
            Ok(tr!(locale, "确认码已发送到 {email}", "Confirmation code sent to {email}"))
        }
        CloudJob::DeleteAccount { password, code } => {
            ops::delete_account(&path, &password, &code).await.map_err(|e| msg(e, locale))?;
            Ok(locale.text("账号已删除", "Account deleted").into())
        }
    }
}
