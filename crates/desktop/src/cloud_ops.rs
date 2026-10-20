//! 在 tokio runtime 上执行设置窗口发起的 Cloud 操作,返回给用户看的一句话。

use std::path::PathBuf;

use octoterm_cloud_client::ops::{self, OpsError};

use crate::settings::cloud::CloudJob;

fn msg(e: OpsError) -> String {
    e.user_message()
}

pub async fn run(job: CloudJob, path: PathBuf, label: String) -> Result<String, String> {
    match job {
        CloudJob::BrowserLogin { url } => {
            let login = crate::browser_login::BrowserLogin::bind(&url, &label).await?;
            open::that_detached(login.url()).map_err(|e| format!("无法打开浏览器：{e}"))?;
            let c = login.complete(&path).await?;
            Ok(format!("已登录为 {}", c.email))
        }
        CloudJob::Logout => match ops::logout(&path).await.map_err(msg)? {
            None => Ok("已退出登录".into()),
            Some(e) => Ok(format!("已在本机退出登录(未能通知 Cloud:{})", e.user_message())),
        },
        CloudJob::RequestDeleteCode => {
            let email = ops::request_delete_code(&path).await.map_err(msg)?;
            Ok(format!("确认码已发送到 {email}"))
        }
        CloudJob::DeleteAccount { password, code } => {
            ops::delete_account(&path, &password, &code).await.map_err(msg)?;
            Ok("账号已删除".into())
        }
    }
}
