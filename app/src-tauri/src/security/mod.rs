use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD_NO_PAD};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, sync::OnceLock};
use zeroize::Zeroize;

/// Explicit unlock also releases descriptors transiently inherited by a concurrent fork.
pub struct FileLease(fs::File);
impl Drop for FileLease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
/// OS locks are released on process exit; never unlink a live lock file.
pub fn lock_file(path: &Path, wait: bool) -> Result<FileLease> {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    if wait {
        file.lock()?;
    } else {
        file.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => anyhow::anyhow!(
                "INSTANCE_RUNNING: 此数据目录已由另一个意构客户端打开，请切换到已有窗口"
            ),
            fs::TryLockError::Error(error) => error.into(),
        })?;
    }
    Ok(FileLease(file))
}

#[derive(Deserialize, Serialize, Zeroize)]
#[zeroize(drop)]
pub struct Credential {
    pub kind: String,
    pub secret: String,
    pub key_path: Option<String>,
}

pub trait Vault: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>>;
    fn set(&self, id: &str, value: &str) -> Result<()>;
    fn delete(&self, id: &str) -> Result<()>;
}
pub struct SystemVault;
impl Vault for SystemVault {
    fn get(&self, id: &str) -> Result<Option<String>> {
        match keyring::Entry::new("io.yigou.desktop", id)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => bail!("KEYRING_UNAVAILABLE: 系统凭据库不可用或已锁定"),
        }
    }
    fn set(&self, id: &str, value: &str) -> Result<()> {
        keyring::Entry::new("io.yigou.desktop", id)?
            .set_password(value)
            .map_err(|_| anyhow::anyhow!("KEYRING_UNAVAILABLE: 凭据未保存，请解锁系统凭据库"))
    }
    fn delete(&self, id: &str) -> Result<()> {
        match keyring::Entry::new("io.yigou.desktop", id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => bail!("KEYRING_UNAVAILABLE: 无法移除凭据"),
        }
    }
}

pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn redact(text: &str) -> String {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(
        r"(?i)password|passwd|secret|community|private[-_ ]?key|cipher|authentication[- ]?(key|password)|pre[- ]?shared[- ]?key|snmp.agent\s+usm-user|snmp.server\s+user|radius.*key|tacacs.*key|key-string|token|authorization|credential|api[-_ ]?key|x[-_ ]?api[-_ ]?key|设备密码|用户密码|管理密码|密码|口令|密钥|私钥|预共享密钥|访问令牌"
    ).expect("constant regex"));
    let mut in_key = false;
    text.lines()
        .map(|line| {
            if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
                in_key = true;
            }
            let result = if in_key || pattern.is_match(line) {
                "[REDACTED]"
            } else {
                line
            };
            if line.contains("-----END ") && line.contains("PRIVATE KEY-----") {
                in_key = false;
            }
            result
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn validate_model_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).context("MODEL_URL_INVALID: 无效模型地址")?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        bail!("MODEL_URL_INVALID: 远程接口需使用 HTTPS，本机可使用 HTTP；地址不能含凭据或查询参数");
    }
    Ok(())
}
pub fn fingerprint(key: &str) -> Result<String> {
    use base64::engine::general_purpose::STANDARD;
    let decoded = STANDARD.decode(key).context("HOST_KEY_INVALID")?;
    Ok(format!(
        "SHA256:{}",
        STANDARD_NO_PAD.encode(Sha256::digest(decoded))
    ))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};
    #[derive(Default)]
    pub struct MemoryVault(pub Mutex<HashMap<String, String>>);
    impl Vault for MemoryVault {
        fn get(&self, id: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(id).cloned())
        }
        fn set(&self, id: &str, v: &str) -> Result<()> {
            self.0.lock().unwrap().insert(id.into(), v.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<()> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }
    #[test]
    fn dropping_lease_unlocks_even_with_an_inherited_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let lease = lock_file(&path, false).unwrap();
        // dup and fork share the same open file description on Unix.
        let _inherited_descriptor = lease.0.try_clone().unwrap();
        assert!(lock_file(&path, false).is_err());
        drop(lease);
        let _new_owner = lock_file(&path, false).unwrap();
    }
    #[test]
    fn redacts_multiline_key_and_password() {
        let output = redact(
            "sysname SW01\npassword cipher abc\n-----BEGIN RSA PRIVATE KEY-----\nhidden\n-----END RSA PRIVATE KEY-----\nreturn",
        );
        assert!(!output.contains("abc"));
        assert!(!output.contains("hidden"));
        assert!(output.contains("sysname SW01"));
    }
    #[test]
    fn redacts_localized_password_and_api_key_labels() {
        for line in [
            "设备密码：EXAMPLE123",
            "口令：EXAMPLE123",
            "密码：EXAMPLE123",
            "API_KEY=EXAMPLE123",
            "X-API-Key: EXAMPLE123",
            "预共享密钥：EXAMPLE123",
        ] {
            assert_eq!(redact(line), "[REDACTED]");
        }
        assert_eq!(
            redact("interface GE0/0/1\n description UPLINK"),
            "interface GE0/0/1\n description UPLINK"
        );
    }

    #[test]
    fn rejects_credential_urls_and_remote_http() {
        for bad in [
            "http://example.com/v1",
            "https://user:pass@example.com",
            "https://example.com?key=x",
            "file:///tmp/x",
        ] {
            assert!(validate_model_url(bad).is_err());
        }
        assert!(validate_model_url("http://127.0.0.1:11434/v1").is_ok());
        assert!(validate_model_url("https://example.com/v1").is_ok());
    }
}
