use crate::{
    security::{Vault, lock_file, private_write, redact},
    tasks::Store,
};
use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, AeadCore, OsRng, Payload},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command};
use uuid::Uuid;
use zeroize::Zeroizing;

fn cipher(vault: &dyn Vault, id: &str) -> Result<Aes256Gcm> {
    if id != "backup-master-key"
        && !(id.starts_with("backup-master-key:")
            && id.len() == 82
            && id[18..].chars().all(|c| c.is_ascii_hexdigit()))
    {
        bail!("BACKUP_KEY_INVALID: 无效备份密钥标识");
    }
    let stored = vault
        .get(id)?
        .context("BACKUP_KEY_UNAVAILABLE: 系统凭据库中的备份密钥缺失")?;
    let key = Zeroizing::new(STANDARD.decode(Zeroizing::new(stored).as_bytes())?);
    Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow::anyhow!("BACKUP_KEY_INVALID"))
}
fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=Yigou",
            "-c",
            "user.email=local@yigou.invalid",
        ])
        .args(args)
        .output()
        .context("BACKUP_GIT_UNAVAILABLE: 需要 Git 保存配置历史")?;
    if !output.status.success() {
        bail!("BACKUP_GIT_FAILED: 配置历史保存失败，备份未标记成功");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
pub fn save(
    root: &Path,
    store: &Store,
    vault: &dyn Vault,
    device: &str,
    content: String,
) -> Result<Value> {
    let content = Zeroizing::new(content);
    let _lease = lock_file(&root.join(".backup.lock"), true)?;
    // The authenticated envelope carries its key ID, so moved backups still use
    // their original key. New directories never overwrite another directory's key.
    let key_id = format!(
        "backup-master-key:{:x}",
        Sha256::digest(root.canonicalize()?.to_string_lossy().as_bytes())
    );
    if vault.get(&key_id)?.is_none() {
        for entry in root.read_dir()? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "ygb") {
                continue;
            }
            let (record, _) = header(&fs::read(path)?)?;
            let old_id = record["key_id"].as_str().unwrap_or("backup-master-key");
            if old_id == key_id || vault.get(old_id)?.is_none() {
                bail!("BACKUP_KEY_UNAVAILABLE: 已有备份的密钥缺失，不能静默生成替代密钥");
            }
        }
        let value = Zeroizing::new(STANDARD.encode(Aes256Gcm::generate_key(OsRng)));
        vault.set(&key_id, &value)?;
    }
    let id = Uuid::new_v4().to_string();
    let name = format!("{id}.ygb");
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let mut record = json!({"id":id,"device_id":device,"file":name,"sha256":digest,"bytes":content.len(),"created_at":Utc::now().to_rfc3339(),"encrypted":true,"key_id":key_id});
    let header = serde_json::to_vec(&record)?;
    let cipher = cipher(vault, &key_id)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let encrypted = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: content.as_bytes(),
                aad: &header,
            },
        )
        .map_err(|_| anyhow::anyhow!("BACKUP_ENCRYPT_FAILED"))?;
    let mut bytes = b"YIGOU01\n".to_vec();
    bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend_from_slice(&nonce);
    bytes.extend_from_slice(&encrypted);
    let temporary = root.join(format!(".{id}.tmp"));
    private_write(&temporary, &bytes)?;
    fs::rename(&temporary, root.join(&name))?;
    #[cfg(unix)]
    fs::File::open(root)?.sync_all()?;
    // Commit only the new encrypted object, never all files in this directory.
    if !root.join(".git").exists() {
        git(root, &["init", "-b", "main"])?;
    }
    git(root, &["add", "--", &name])?;
    git(
        root,
        &[
            "commit",
            "--only",
            "-m",
            &format!("backup {device} {id}"),
            "--",
            &name,
        ],
    )?;
    let commit = git(root, &["rev-parse", "HEAD"])?;
    record["commit"] = json!(commit);
    // Validate the persisted bytes, authentication tag and original SHA before success.
    verify(root, vault, &record)?;
    store.save_backup(&record)?;
    Ok(record)
}
pub fn verify(root: &Path, vault: &dyn Vault, record: &Value) -> Result<Value> {
    let id = record["id"].as_str().context("BACKUP_ID")?;
    Uuid::parse_str(id)?;
    let bytes = fs::read(root.join(format!("{id}.ygb")))?;
    let (metadata, offset) = header(&bytes)?;
    for key in [
        "id",
        "device_id",
        "sha256",
        "bytes",
        "created_at",
        "file",
        "key_id",
    ] {
        if metadata[key] != record[key] {
            bail!("BACKUP_CORRUPT: 备份元数据与索引不匹配");
        }
    }
    let cipher = cipher(
        vault,
        metadata["key_id"].as_str().unwrap_or("backup-master-key"),
    )?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                (&bytes[offset..offset + 12]).into(),
                Payload {
                    msg: &bytes[offset + 12..],
                    aad: &bytes[12..offset],
                },
            )
            .map_err(|_| anyhow::anyhow!("BACKUP_CORRUPT: 解密校验失败"))?,
    );
    let digest = format!("{:x}", Sha256::digest(&plain));
    if record["sha256"] != digest || record["bytes"].as_u64() != Some(plain.len() as u64) {
        bail!("BACKUP_CORRUPT: 配置完整性校验失败");
    }
    Ok(
        json!({"id":id,"verified":true,"sha256":digest,"bytes":plain.len(),"preview":redact(&String::from_utf8_lossy(&plain))}),
    )
}

fn header(bytes: &[u8]) -> Result<(Value, usize)> {
    if bytes.len() < 40 || bytes.len() > 16 * 1024 * 1024 || &bytes[..8] != b"YIGOU01\n" {
        bail!("BACKUP_CORRUPT: 无效备份文件");
    }
    let size = u32::from_be_bytes(bytes[8..12].try_into()?) as usize;
    if size > 4096 || bytes.len() < 12 + size + 28 {
        bail!("BACKUP_CORRUPT: 无效备份头");
    }
    Ok((serde_json::from_slice(&bytes[12..12 + size])?, 12 + size))
}

/// Rebuild missing SQLite backup rows from authenticated, Git-tracked envelopes.
pub fn reindex(root: &Path, store: &Store, vault: &dyn Vault, device: &str) -> Result<Value> {
    let _lease = lock_file(&root.join(".backup.lock"), true)?;
    store.device(device)?;
    let existing = store.backups(device)?;
    let mut recovered = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "ygb") {
            continue;
        }
        let (mut record, _) = header(&fs::read(&path)?)?;
        if record["device_id"] != device {
            continue;
        }
        verify(root, vault, &record)?;
        if existing.iter().any(|r| r["id"] == record["id"]) {
            continue;
        }
        let name = record["file"].as_str().context("BACKUP_FILE")?;
        let commit = git(root, &["log", "-1", "--format=%H", "--", name])?;
        if commit.is_empty() {
            bail!("BACKUP_GIT_MISSING: 加密文件未提交，不能恢复为成功备份");
        }
        record["commit"] = json!(commit);
        store.save_backup(&record)?;
        recovered.push(record);
    }
    Ok(json!({"recovered_count":recovered.len(),"records":recovered}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{private_dir, tests::MemoryVault};
    #[test]
    fn concurrent_backups_share_one_key_and_moving_a_directory_keeps_old_keys() {
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("backups");
        private_dir(&root).unwrap();
        let store = Arc::new(Store::open(&dir.path().join("db")).unwrap());
        store.save_device(&json!({"id":"dev"})).unwrap();
        let vault = Arc::new(MemoryVault::default());
        let barrier = Arc::new(Barrier::new(4));
        let handles: Vec<_> = (0..4)
            .map(|n| {
                let (root, store, vault, barrier) =
                    (root.clone(), store.clone(), vault.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    save(
                        &root,
                        &store,
                        vault.as_ref(),
                        "dev",
                        format!("#\nsysname TEST{n}\nreturn"),
                    )
                    .unwrap()
                })
            })
            .collect();
        let records: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(vault.0.lock().unwrap().len(), 1);
        for record in &records {
            assert_eq!(
                verify(&root, vault.as_ref(), record).unwrap()["verified"],
                true
            );
        }
        let moved = dir.path().join("moved");
        fs::rename(&root, &moved).unwrap();
        let next = save(
            &moved,
            &store,
            vault.as_ref(),
            "dev",
            "#\nsysname NEXT\nreturn".into(),
        )
        .unwrap();
        assert_ne!(next["key_id"], records[0]["key_id"]);
        for record in &records {
            assert_eq!(
                verify(&moved, vault.as_ref(), record).unwrap()["verified"],
                true
            );
        }
        assert_eq!(
            verify(&moved, vault.as_ref(), &next).unwrap()["verified"],
            true
        );
    }
    #[test]
    fn legacy_envelopes_remain_readable_without_replacing_the_legacy_key() {
        let dir = tempfile::tempdir().unwrap();
        let vault = MemoryVault::default();
        vault
            .set("backup-master-key", &STANDARD.encode([7u8; 32]))
            .unwrap();
        let id = Uuid::new_v4().to_string();
        let text = b"#\nsysname LEGACY\nreturn";
        let record = json!({"id":id,"device_id":"dev","file":format!("{id}.ygb"),"sha256":format!("{:x}",Sha256::digest(text)),"bytes":text.len(),"created_at":"legacy","encrypted":true});
        let header = serde_json::to_vec(&record).unwrap();
        let cipher = cipher(&vault, "backup-master-key").unwrap();
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: text,
                    aad: &header,
                },
            )
            .unwrap();
        let mut bytes = b"YIGOU01\n".to_vec();
        bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
        bytes.extend(header);
        bytes.extend(nonce);
        bytes.extend(ciphertext);
        fs::write(dir.path().join(format!("{id}.ygb")), bytes).unwrap();
        assert_eq!(
            verify(dir.path(), &vault, &record).unwrap()["verified"],
            true
        );
        assert_eq!(
            vault.get("backup-master-key").unwrap().unwrap(),
            STANDARD.encode([7u8; 32])
        );
    }
    #[test]
    fn backup_encrypts_full_configuration_and_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("backups");
        private_dir(&root).unwrap();
        let db = Store::open(&dir.path().join("db")).unwrap();
        db.save_device(&json!({"id":"dev"})).unwrap();
        let vault = MemoryVault::default();
        let content = "#\nsysname SW01\npassword cipher top-secret\nreturn";
        let record = save(&root, &db, &vault, "dev", content.into()).unwrap();
        let path = root.join(record["file"].as_str().unwrap());
        let mut bytes = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("top-secret"));
        let recovered = verify(&root, &vault, &record).unwrap();
        assert_eq!(recovered["bytes"], content.len());
        assert!(
            !recovered["preview"]
                .as_str()
                .unwrap()
                .contains("top-secret")
        );
        let end = bytes.len() - 1;
        bytes[end] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(verify(&root, &vault, &record).is_err());
    }
    #[test]
    fn lost_index_is_rebuilt_and_missing_key_never_gets_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("backups");
        private_dir(&root).unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        store.save_device(&json!({"id":"dev"})).unwrap();
        let vault = MemoryVault::default();
        let record = save(
            &root,
            &store,
            &vault,
            "dev",
            "#\nsysname LAB\nreturn".into(),
        )
        .unwrap();
        store
            .0
            .lock()
            .unwrap()
            .execute("DELETE FROM backups", [])
            .unwrap();
        assert_eq!(
            reindex(&root, &store, &vault, "dev").unwrap()["recovered_count"],
            1
        );
        assert_eq!(store.backups("dev").unwrap()[0], record);
        let key_id = record["key_id"].as_str().unwrap();
        vault.delete(key_id).unwrap();
        assert!(verify(&root, &vault, &record).is_err());
        assert!(save(&root, &store, &vault, "dev", "new config".into()).is_err());
        assert!(vault.get(key_id).unwrap().is_none());
    }
}
