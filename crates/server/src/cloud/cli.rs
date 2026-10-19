//! `octoterm-server cloud <子命令>`:在没有 GUI 的机器上登录 Cloud。
//! 逻辑都在 `octoterm_cloud_client::ops`,这里只管问答与输出。

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use octoterm_cloud_client::ops::{self, OpsError};
use octoterm_cloud_client::{CloudClient, CloudError, CodePurpose, credentials, default_label};

#[derive(clap::Subcommand, Debug, Clone, Copy)]
pub enum CloudCmd {
    /// 注册 Cloud 账号并让本机登录
    Register,
    /// 登录已有账号
    Login,
    /// 退出登录(撤销本机凭据)
    Logout,
    /// 查看登录状态
    Status,
    /// 通过邮件验证码重置密码(会让所有设备退出登录)
    ResetPassword,
    /// 永久删除 Cloud 账号及其全部数据
    DeleteAccount,
}

pub fn run(cmd: CloudCmd, config_path: Option<PathBuf>) -> i32 {
    let default_url = match crate::config::Config::load(config_path) {
        Ok(c) => c.cloud.url,
        Err(e) => {
            eprintln!("读取配置失败:{e:#}");
            return 1;
        }
    };
    let Some(path) = credentials::default_path() else {
        eprintln!("无法确定配置目录");
        return 1;
    };
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("无法创建 tokio runtime:{e}");
            return 1;
        }
    };
    match rt.block_on(dispatch(cmd, &path, default_url)) {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

fn ask(label: &str) -> Result<String, String> {
    print!("{label}");
    io::stdout().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim().to_string())
}

/// 终端里不回显;stdin 被重定向时(脚本、冒烟测试)按普通行读。
fn ask_secret(label: &str) -> Result<String, String> {
    if io::stdin().is_terminal() {
        rpassword::prompt_password(label).map_err(|e| e.to_string())
    } else {
        ask(label)
    }
}

fn ask_url(default_url: Option<String>) -> Result<String, String> {
    if let Some(u) = default_url.filter(|u| !u.trim().is_empty()) {
        return Ok(u);
    }
    let u = ask("Cloud 地址(如 https://cloud.example.com):")?;
    if u.starts_with("http://") || u.starts_with("https://") { Ok(u) } else { Err("Cloud 地址必须以 http:// 或 https:// 开头".into()) }
}

/// 重置密码时该问哪个 Cloud 地址:已登录的那个优先于配置里的默认地址
/// (两者多数时候相同,但配置改过、或者本机登录的是别的 Cloud 时不能弄混)。
fn resolve_reset_url(logged_in_url: Option<String>, default_url: Option<String>) -> Option<String> {
    logged_in_url.or(default_url)
}

fn ask_new_password() -> Result<String, String> {
    let a = ask_secret("密码(8–128 位):")?;
    let b = ask_secret("再输一次:")?;
    if a != b {
        return Err("两次输入的密码不一致".into());
    }
    Ok(a)
}

fn msg(e: OpsError) -> String {
    e.user_message()
}

async fn dispatch(cmd: CloudCmd, path: &Path, default_url: Option<String>) -> Result<(), String> {
    match cmd {
        CloudCmd::Register => {
            if let Ok(c) = ops::current(path) {
                return Err(format!("本机已登录为 {};请先 `octoterm-server cloud logout`", c.email));
            }
            let url = ask_url(default_url)?;
            let email = ops::normalize_email(&ask("邮箱:")?);
            CloudClient::new(&url)
                .request_code(&email, CodePurpose::Register)
                .await
                .map_err(|e| e.user_message())?;
            println!("验证码已发送到 {email}(10 分钟内有效)");
            let code = ask("验证码:")?;
            let password = ask_new_password()?;
            let c = ops::register(path, &url, &email, &code, &password, &default_label()).await.map_err(msg)?;
            println!("注册成功,本机已登录为 {}", c.email);
        }
        CloudCmd::Login => {
            if let Ok(c) = ops::current(path) {
                return Err(format!("本机已登录为 {};请先 `octoterm-server cloud logout`", c.email));
            }
            let url = ask_url(default_url)?;
            let email = ask("邮箱:")?;
            let password = ask_secret("密码:")?;
            let c = ops::login(path, &url, &email, &password, &default_label()).await.map_err(msg)?;
            println!("已登录为 {}", c.email);
        }
        CloudCmd::Logout => match ops::logout(path).await {
            Ok(None) => println!("已退出登录"),
            Ok(Some(e)) => println!("已在本机退出登录(未能通知 Cloud:{})", e.user_message()),
            Err(OpsError::NotLoggedIn) => println!("当前未登录"),
            Err(e) => return Err(e.user_message()),
        },
        CloudCmd::Status => {
            let c = match ops::current(path) {
                Ok(c) => c,
                Err(OpsError::NotLoggedIn) => {
                    println!("未登录");
                    return Ok(());
                }
                Err(e) => return Err(e.user_message()),
            };
            println!("Cloud:{}\n账号:{}\nserver_id:{}", c.url, c.email, c.server_id.as_deref().unwrap_or("-"));
            match ops::account_status(path).await {
                Ok(info) => println!("状态:有效({} 台 server,{} 台设备)", info.servers.len(), info.devices.len()),
                Err(OpsError::Cloud(CloudError::Unauthorized)) => println!("状态:凭据已失效,已在本机退出登录"),
                Err(e) => println!("状态:{}", e.user_message()),
            }
        }
        CloudCmd::ResetPassword => {
            let url = ask_url(resolve_reset_url(ops::current(path).ok().map(|c| c.url), default_url))?;
            let email = ops::normalize_email(&ask("邮箱:")?);
            CloudClient::new(&url)
                .request_code(&email, CodePurpose::Reset)
                .await
                .map_err(|e| e.user_message())?;
            println!("如果该邮箱已注册,验证码已发送(10 分钟内有效)");
            let code = ask("验证码:")?;
            let password = ask_new_password()?;
            ops::reset_password(path, &url, &email, &code, &password).await.map_err(msg)?;
            println!("密码已重置;所有设备都需要重新登录");
        }
        CloudCmd::DeleteAccount => {
            let c = ops::current(path).map_err(msg)?;
            println!("将永久删除 Cloud 账号 {} 及其全部数据(所有 server、设备、推送登记),无法恢复。", c.email);
            if ops::normalize_email(&ask("输入你的邮箱以确认:")?) != c.email {
                return Err("邮箱不匹配,已取消".into());
            }
            let email = ops::request_delete_code(path).await.map_err(msg)?;
            println!("确认码已发送到 {email}");
            let password = ask_secret("密码:")?;
            let code = ask("确认码:")?;
            ops::delete_account(path, &password, &code).await.map_err(msg)?;
            println!("账号已删除");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_reset_url;

    #[test]
    fn logged_in_url_wins_over_config_default() {
        assert_eq!(
            resolve_reset_url(Some("https://logged-in.example.com".into()), Some("https://config-default.example.com".into())),
            Some("https://logged-in.example.com".into())
        );
    }

    #[test]
    fn falls_back_to_config_default_when_not_logged_in() {
        assert_eq!(resolve_reset_url(None, Some("https://config-default.example.com".into())), Some("https://config-default.example.com".into()));
    }

    #[test]
    fn none_when_neither_is_set() {
        assert_eq!(resolve_reset_url(None, None), None);
    }
}
