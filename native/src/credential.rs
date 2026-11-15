//! Plugin-scoped OS credential entry. No tokens are written to plugin files.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn validate_token(value: &str) -> Result<(), String> {
    if value.len() < 8
        || value.len() > 4096
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err("GitHub 令牌格式无效，请不要包含前缀、链接或空白。".into());
    }
    Ok(())
}
pub fn credential_user(root: &str) -> String {
    format!("github-{:x}", Sha256::digest(root.as_bytes()))
}
fn entry(root: &Path) -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        "OTools ReleaseHub",
        &credential_user(&root.to_string_lossy()),
    )
    .map_err(|_| "系统凭据库不可用。".into())
}
pub fn load(root: &Path) -> Result<Option<String>, String> {
    match entry(root)?.get_password() {
        Ok(token) => {
            validate_token(&token)?;
            Ok(Some(token))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("无法读取系统凭据库，请重新连接 GitHub。".into()),
    }
}
pub fn save(root: &Path, token: &str) -> Result<(), String> {
    validate_token(token)?;
    entry(root)?
        .set_password(token)
        .map_err(|_| "无法安全保存到系统凭据库，本次登录未保存。".into())
}
pub fn clear(root: &Path) -> Result<(), String> {
    match entry(root)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("无法删除本插件的系统凭据。".into()),
    }
}
pub fn auth_directory(root: &Path) -> Result<PathBuf, String> {
    let base = dirs::data_local_dir().ok_or("无法定位本机授权目录。")?;
    Ok(base
        .join("OTools-ReleaseHub-auth")
        .join(credential_user(&root.to_string_lossy())))
}
