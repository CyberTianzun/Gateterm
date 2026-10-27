//! Gateterm Cloud 客户端:HTTP API、本地凭据文件、以及两者组合起来的账号操作。
//! server CLI 和 desktop 设置窗口都只是这里的薄壳。

pub mod api;
pub mod credentials;
pub mod ops;

pub use api::{
    AccountInfo, AuthOk, ClientInfo, ClientKind, CloudClient, CloudError, CodePurpose, DeviceEntry, Notify,
    ServerEntry, default_label,
};
pub use credentials::Credentials;
