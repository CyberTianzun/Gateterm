//! 「调 Cloud + 改本地 cloud.json」的组合操作。CLI 与 desktop 共用,保证两边行为一致。

use std::path::Path;

use crate::api::{AuthOk, ClientInfo, CloudClient, CloudError};
use crate::credentials::{self, Credentials};

#[derive(Debug)]
pub enum OpsError {
    Cloud(CloudError),
    Local(String),
    NotLoggedIn,
}

impl OpsError {
    pub fn user_message(&self) -> String {
        match self {
            OpsError::Cloud(e) => e.user_message(),
            OpsError::Local(m) => m.clone(),
            OpsError::NotLoggedIn => "当前未登录 Cloud".into(),
        }
    }
}

impl std::fmt::Display for OpsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.user_message())
    }
}

impl From<CloudError> for OpsError {
    fn from(e: CloudError) -> Self {
        OpsError::Cloud(e)
    }
}

fn io(e: std::io::Error) -> OpsError {
    OpsError::Local(format!("读写 cloud.json 失败:{e}"))
}

pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn current(path: &Path) -> Result<Credentials, OpsError> {
    credentials::load(path).map_err(io)?.ok_or(OpsError::NotLoggedIn)
}

fn persist(path: &Path, url: &str, email: String, ok: AuthOk) -> Result<Credentials, OpsError> {
    let creds = Credentials {
        url: url.trim().trim_end_matches('/').to_string(),
        email,
        token: ok.token,
        server_id: ok.server_id,
    };
    credentials::save(path, &creds).map_err(io)?;
    Ok(creds)
}

pub async fn register(
    path: &Path,
    url: &str,
    email: &str,
    code: &str,
    password: &str,
    label: &str,
) -> Result<Credentials, OpsError> {
    let email = normalize_email(email);
    let ok = CloudClient::new(url).register(&email, code.trim(), password, &ClientInfo::server(label)).await?;
    persist(path, url, email, ok)
}

pub async fn login(path: &Path, url: &str, email: &str, password: &str, label: &str) -> Result<Credentials, OpsError> {
    let email = normalize_email(email);
    let ok = CloudClient::new(url).login(&email, password, &ClientInfo::server(label)).await?;
    persist(path, url, email, ok)
}

/// Cloud 会撤销该账号的全部凭据;本机若登录的正是这个邮箱,本地登录态一并清掉。
pub async fn reset_password(path: &Path, url: &str, email: &str, code: &str, new_password: &str) -> Result<(), OpsError> {
    let email = normalize_email(email);
    CloudClient::new(url).reset_password(&email, code.trim(), new_password).await?;
    if let Ok(Some(c)) = credentials::load(path)
        && c.email == email
    {
        credentials::delete(path).map_err(io)?;
    }
    Ok(())
}

/// 本地登录态一定会被删掉。返回 `Some(e)` 表示没能通知 Cloud(凭据在 Cloud 上还有效)。
pub async fn logout(path: &Path) -> Result<Option<CloudError>, OpsError> {
    let c = current(path)?;
    let r = CloudClient::new(&c.url).logout(&c.token).await;
    credentials::delete(path).map_err(io)?;
    Ok(match r {
        Ok(()) | Err(CloudError::Unauthorized) => None,
        Err(e) => Some(e),
    })
}

pub async fn request_delete_code(path: &Path) -> Result<String, OpsError> {
    let c = current(path)?;
    CloudClient::new(&c.url).request_delete_code(&c.token).await?;
    Ok(c.email)
}

pub async fn delete_account(path: &Path, password: &str, code: &str) -> Result<(), OpsError> {
    let c = current(path)?;
    CloudClient::new(&c.url).delete_account(&c.token, password, code.trim()).await?;
    credentials::delete(path).map_err(io)?;
    Ok(())
}
