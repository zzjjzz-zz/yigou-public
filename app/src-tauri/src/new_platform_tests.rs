//! Regression for batch-1 read-only platforms: NewDevice validation and the
//! Rust-side capability gates on backup / configure. No device traffic.

use crate::{
    devices::{
        AppState, NewDevice, vendor_allows_backup, vendor_allows_configure, vendor_exists,
        vendor_queries, worker::WorkerClient,
    },
    security::{Credential, tests::MemoryVault},
};
use std::sync::{Arc, Mutex};

const NEW_PLATFORMS: &[(&str, &[&str])] = &[
    (
        "cisco_ios",
        &["version", "ports", "config", "neighbors", "startup"],
    ),
    (
        "cisco_xe",
        &["version", "ports", "config", "neighbors", "startup"],
    ),
    ("maipu", &["version", "config", "startup"]),
    ("raisecom_ros", &["version", "config", "startup"]),
    ("raisecom_roap", &["version"]),
    ("huawei_olt", &["version", "config"]),
    ("huawei_smartax", &["version", "config"]),
    ("zte_zxros", &["version"]),
];

fn device(vendor: &str, auto_backup: bool) -> NewDevice {
    NewDevice {
        name: "NEW-LAB".into(),
        host: "127.0.0.1".into(),
        port: 22,
        vendor: vendor.into(),
        username: "lab".into(),
        auto_backup,
        backup_minutes: 60,
    }
}

#[test]
fn all_new_vendors_are_enrollable_and_flagged_read_only() {
    for (vendor, queries) in NEW_PLATFORMS {
        assert!(vendor_exists(vendor), "{vendor} missing");
        assert!(
            !vendor_allows_backup(vendor),
            "{vendor} must not allow backup"
        );
        assert!(
            !vendor_allows_configure(vendor),
            "{vendor} must not allow configure"
        );
        assert_eq!(
            &vendor_queries(vendor).to_vec(),
            queries,
            "{vendor} whitelist mismatch"
        );
        device(vendor, false).validate().unwrap();
        // auto_backup is rejected at validation time, not only in the UI.
        let error = device(vendor, true).validate().unwrap_err().to_string();
        assert!(error.contains("不支持自动备份"), "{vendor}: {error}");
    }
    for legacy in ["huawei", "ruijie", "h3c"] {
        assert!(vendor_allows_backup(legacy) && vendor_allows_configure(legacy));
        device(legacy, true).validate().unwrap();
    }
}

#[test]
fn unknown_vendor_still_rejected() {
    let error = device("no-such-platform", false)
        .validate()
        .unwrap_err()
        .to_string();
    assert!(error.contains("无效"));
}

async fn state() -> (tempfile::TempDir, Arc<AppState>) {
    let dir = tempfile::tempdir().unwrap();
    let python = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("worker/.venv/bin/python");
    let events: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let state = AppState::new(
        dir.path().to_path_buf(),
        WorkerClient::new(python, vec!["-m".into(), "yigou_worker".into()]),
        Arc::new(MemoryVault::default()),
        Arc::new(move |event| events.lock().unwrap().push(event)),
    )
    .unwrap();
    (dir, state)
}

#[tokio::test]
async fn read_only_platform_rejects_backup_and_configure_before_any_work() {
    for (vendor, _) in NEW_PLATFORMS {
        let (_dir, state) = state().await;
        let added = state
            .add(
                device(vendor, false),
                Credential {
                    kind: "password".into(),
                    secret: "fixture-only".into(),
                    key_path: None,
                },
            )
            .await
            .unwrap();
        let id = added["id"].as_str().unwrap().to_string();
        let backup = state.backup(&id).await.unwrap_err().to_string();
        assert!(
            backup.contains("OPERATION_UNSUPPORTED"),
            "{vendor}: {backup}"
        );
        let configure = state
            .create_config_plan(
                &id,
                crate::devices::configure::ConfigArgs {
                    commands: vec!["description probe".into()],
                    save: false,
                    checks: vec![crate::devices::configure::Verification {
                        query: "config".into(),
                        contains: vec!["probe".into()],
                        not_contains: vec![],
                    }],
                },
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            configure.contains("OPERATION_UNSUPPORTED"),
            "{vendor}: {configure}"
        );
        // Denied queries fail closed with no device session needed.
        for kind in ["version", "ports", "config", "neighbors", "startup"] {
            let outcome = state.query(&id, kind).await.unwrap_err().to_string();
            if vendor_queries(vendor).contains(&kind) {
                assert!(
                    outcome.contains("NOT_CONNECTED"),
                    "{vendor}/{kind}: {outcome}"
                );
            } else {
                assert!(
                    outcome.contains("QUERY_DENIED"),
                    "{vendor}/{kind}: {outcome}"
                );
            }
        }
    }
}
