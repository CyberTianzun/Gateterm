//! 在 tokio runtime 上执行设置窗口发起的 Cloud 操作,返回给用户看的一句话。

use std::path::PathBuf;

use octoterm_cloud_client::ops::{self, OpsError};
use octoterm_cloud_client::{ClientKind, CloudClient, CodePurpose};

use crate::settings::cloud::CloudJob;

fn msg(e: OpsError) -> String {
    e.user_message()
}

pub async fn run(job: CloudJob, path: PathBuf, label: String) -> Result<String, String> {
    match job {
        CloudJob::RequestCode { url, email, purpose } => {
            CloudClient::new(&url).request_code(&email, purpose).await.map_err(|e| e.user_message())?;
            Ok(match purpose {
                CodePurpose::Register => format!("验证码已发送到 {email},10 分钟内有效"),
                CodePurpose::Reset => "如果该邮箱已注册,验证码已发送(10 分钟内有效)".into(),
            })
        }
        CloudJob::Register { url, email, code, password } => {
            let c = ops::register(&path, &url, &email, &code, &password, ClientKind::Server, &label).await.map_err(msg)?;
            Ok(format!("注册成功,已登录为 {}", c.email))
        }
        CloudJob::Login { url, email, password } => {
            let c = ops::login(&path, &url, &email, &password, ClientKind::Server, &label).await.map_err(msg)?;
            Ok(format!("已登录为 {}", c.email))
        }
        CloudJob::ResetPassword { url, email, code, password } => {
            ops::reset_password(&path, &url, &email, &code, &password).await.map_err(msg)?;
            Ok("密码已重置;所有设备都需要重新登录".into())
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
