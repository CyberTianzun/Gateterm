//! 把 `octoterm-cloud-client::ops` 用 UniFFI 导出给移动端(Android 现在、iOS 将来)。
//!
//! 只做类型转换:账号逻辑、吊销规则、凭据文件都在 `ops` 里,三端行为因此一致。
//! 错误只给错误码,文案由 App 按自己的语言生成。

uniffi::setup_scaffolding!();

use std::path::PathBuf;
use std::sync::Arc;

use octoterm_cloud_client::ops::{self, OpsError};
use octoterm_cloud_client::{ClientKind, CloudClient, CloudError, CodePurpose};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CloudFfiError {
    #[error("not logged in")]
    NotLoggedIn,
    #[error("unauthorized")]
    Unauthorized,
    #[error("rate limited")]
    RateLimited { retry_after: Option<u64> },
    #[error("api error: {code}")]
    Api { code: String },
    #[error("unreachable: {message}")]
    Unreachable { message: String },
    #[error("local error: {message}")]
    Local { message: String },
}

impl From<CloudError> for CloudFfiError {
    fn from(e: CloudError) -> Self {
        match e {
            CloudError::Unauthorized => Self::Unauthorized,
            CloudError::RateLimited { retry_after } => Self::RateLimited { retry_after },
            CloudError::Api { code, .. } => Self::Api { code },
            CloudError::Unreachable(message) => Self::Unreachable { message },
        }
    }
}

impl From<OpsError> for CloudFfiError {
    fn from(e: OpsError) -> Self {
        match e {
            OpsError::NotLoggedIn => Self::NotLoggedIn,
            OpsError::Local(message) => Self::Local { message },
            OpsError::Cloud(e) => e.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum Purpose {
    Register,
    Reset,
}

impl From<Purpose> for CodePurpose {
    fn from(p: Purpose) -> Self {
        match p {
            Purpose::Register => CodePurpose::Register,
            Purpose::Reset => CodePurpose::Reset,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CloudStatus {
    pub email: String,
    pub url: String,
}

/// 一台设备上的 Cloud 账号。`cred_path` 由 App 给(Android:`filesDir/cloud.json`)。
#[derive(uniffi::Object)]
pub struct CloudAccount {
    path: PathBuf,
    label: String,
}

#[uniffi::export(async_runtime = "tokio")]
impl CloudAccount {
    #[uniffi::constructor]
    pub fn new(cred_path: String, label: String) -> Arc<Self> {
        Arc::new(Self { path: PathBuf::from(cred_path), label })
    }

    /// 只读本地凭据文件,不联网。
    pub fn status(&self) -> Option<CloudStatus> {
        ops::current(&self.path).ok().map(|c| CloudStatus { email: c.email, url: c.url })
    }

    pub async fn request_code(&self, url: String, email: String, purpose: Purpose) -> Result<(), CloudFfiError> {
        CloudClient::new(&url).request_code(&ops::normalize_email(&email), purpose.into()).await?;
        Ok(())
    }

    pub async fn register(&self, url: String, email: String, code: String, password: String) -> Result<CloudStatus, CloudFfiError> {
        let c = ops::register(&self.path, &url, &email, &code, &password, ClientKind::Device, &self.label).await?;
        Ok(CloudStatus { email: c.email, url: c.url })
    }

    pub async fn login(&self, url: String, email: String, password: String) -> Result<CloudStatus, CloudFfiError> {
        let c = ops::login(&self.path, &url, &email, &password, ClientKind::Device, &self.label).await?;
        Ok(CloudStatus { email: c.email, url: c.url })
    }

    pub async fn reset_password(&self, url: String, email: String, code: String, new_password: String) -> Result<(), CloudFfiError> {
        ops::reset_password(&self.path, &url, &email, &code, &new_password).await?;
        Ok(())
    }

    /// 本地登录态一定清掉。返回 true 表示 Cloud 上的凭据也已撤销。
    pub async fn logout(&self) -> Result<bool, CloudFfiError> {
        Ok(ops::logout(&self.path).await?.is_none())
    }

    pub async fn request_delete_code(&self) -> Result<String, CloudFfiError> {
        Ok(ops::request_delete_code(&self.path).await?)
    }

    pub async fn delete_account(&self, password: String, code: String) -> Result<(), CloudFfiError> {
        ops::delete_account(&self.path, &password, &code).await?;
        Ok(())
    }

    pub async fn register_push_target(&self, platform: String, token: String) -> Result<(), CloudFfiError> {
        ops::register_push_target(&self.path, &platform, &token).await?;
        Ok(())
    }
}
