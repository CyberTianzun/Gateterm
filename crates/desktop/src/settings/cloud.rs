//! 「Cloud 账号」区的表单与校验。和 state.rs 一样不认识 egui,全部可单测。

use octoterm_cloud_client::{CodePurpose, Credentials};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudForm {
    pub url: String,
    pub email: String,
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
        Some(c) => CloudStatus::LoggedIn { email: c.email, url: c.url },
        None => CloudStatus::LoggedOut,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudAction {
    SendRegisterCode,
    Register,
    Login,
    SendResetCode,
    ResetPassword,
    Logout,
    SendDeleteCode,
    DeleteAccount,
}

impl CloudAction {
    /// 发码只是中间一步,用户接下来还要用刚填的密码 —— 这时不清空表单里的密码。
    pub fn keeps_secrets(self) -> bool {
        matches!(self, CloudAction::SendRegisterCode | CloudAction::SendResetCode | CloudAction::SendDeleteCode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudJob {
    RequestCode { url: String, email: String, purpose: CodePurpose },
    Register { url: String, email: String, code: String, password: String },
    Login { url: String, email: String, password: String },
    ResetPassword { url: String, email: String, code: String, password: String },
    Logout,
    RequestDeleteCode,
    DeleteAccount { password: String, code: String },
}

fn url(form: &CloudForm) -> Result<String, String> {
    let u = form.url.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        Ok(u.to_string())
    } else {
        Err("Cloud 地址必须以 http:// 或 https:// 开头".into())
    }
}

fn email(form: &CloudForm) -> Result<String, String> {
    let e = form.email.trim().to_lowercase();
    if e.contains('@') { Ok(e) } else { Err("请填写邮箱".into()) }
}

fn password(form: &CloudForm) -> Result<String, String> {
    let n = form.password.chars().count();
    if (8..=128).contains(&n) { Ok(form.password.clone()) } else { Err("密码长度需为 8–128 个字符".into()) }
}

fn code(form: &CloudForm) -> Result<String, String> {
    let c = form.code.trim();
    if c.len() == 6 && c.chars().all(|ch| ch.is_ascii_digit()) {
        Ok(c.to_string())
    } else {
        Err("请填写邮件里的 6 位验证码".into())
    }
}

pub fn prepare(form: &CloudForm, status: &CloudStatus, action: CloudAction) -> Result<CloudJob, String> {
    use CloudAction::*;
    let logged_in = matches!(status, CloudStatus::LoggedIn { .. });
    let needs_login = matches!(action, Logout | SendDeleteCode | DeleteAccount);
    if needs_login != logged_in {
        return Err(if logged_in { "已登录;请先退出登录".into() } else { "当前未登录".into() });
    }
    Ok(match action {
        SendRegisterCode => CloudJob::RequestCode { url: url(form)?, email: email(form)?, purpose: CodePurpose::Register },
        SendResetCode => CloudJob::RequestCode { url: url(form)?, email: email(form)?, purpose: CodePurpose::Reset },
        Register => CloudJob::Register { url: url(form)?, email: email(form)?, code: code(form)?, password: password(form)? },
        Login => {
            if form.password.is_empty() {
                return Err("请填写密码".into());
            }
            CloudJob::Login { url: url(form)?, email: email(form)?, password: form.password.clone() }
        }
        ResetPassword => {
            CloudJob::ResetPassword { url: url(form)?, email: email(form)?, code: code(form)?, password: password(form)? }
        }
        Logout => CloudJob::Logout,
        SendDeleteCode => CloudJob::RequestDeleteCode,
        DeleteAccount => {
            if !form.confirm_delete {
                return Err("请先勾选「我明白删除后无法恢复」".into());
            }
            if form.password.is_empty() {
                return Err("请填写密码".into());
            }
            CloudJob::DeleteAccount { password: form.password.clone(), code: code(form)? }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> CloudForm {
        CloudForm {
            url: " https://cloud.example.com ".into(),
            email: " A@Example.com ".into(),
            password: "pw-pw-pw".into(),
            code: " 012345 ".into(),
            confirm_delete: false,
        }
    }

    fn logged_in() -> CloudStatus {
        CloudStatus::LoggedIn { email: "a@example.com".into(), url: "https://cloud.example.com".into() }
    }

    #[test]
    fn register_job_is_normalized() {
        assert_eq!(
            prepare(&form(), &CloudStatus::LoggedOut, CloudAction::Register),
            Ok(CloudJob::Register {
                url: "https://cloud.example.com".into(),
                email: "a@example.com".into(),
                code: "012345".into(),
                password: "pw-pw-pw".into(),
            })
        );
    }

    #[test]
    fn validation_errors() {
        let mut f = form();
        f.url = "cloud.example.com".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::Login).is_err());
        let mut f = form();
        f.code = "12a456".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::Register).is_err());
        let mut f = form();
        f.password = "short".into();
        assert!(prepare(&f, &CloudStatus::LoggedOut, CloudAction::ResetPassword).is_err());
    }

    #[test]
    fn actions_require_matching_login_state() {
        assert!(prepare(&form(), &logged_in(), CloudAction::Login).is_err());
        assert!(prepare(&form(), &CloudStatus::LoggedOut, CloudAction::Logout).is_err());
        assert_eq!(prepare(&form(), &logged_in(), CloudAction::Logout), Ok(CloudJob::Logout));
    }

    #[test]
    fn delete_requires_confirmation() {
        assert!(prepare(&form(), &logged_in(), CloudAction::DeleteAccount).is_err());
        let mut f = form();
        f.confirm_delete = true;
        assert_eq!(
            prepare(&f, &logged_in(), CloudAction::DeleteAccount),
            Ok(CloudJob::DeleteAccount { password: "pw-pw-pw".into(), code: "012345".into() })
        );
    }

    #[test]
    fn send_code_keeps_secrets() {
        assert!(CloudAction::SendRegisterCode.keeps_secrets());
        assert!(!CloudAction::Register.keeps_secrets());
    }
}
