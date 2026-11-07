//! Local scripted SSH + Rig streaming + real Python worker. No vendor hardware or online model.
use crate::{
    devices::{
        AppState, NewDevice,
        configure::{ConfigApproval, ConfigArgs},
        worker::WorkerClient,
    },
    security::{Credential, Vault, tests::MemoryVault},
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

struct Lab {
    process: Child,
    _dir: tempfile::TempDir,
    state: Arc<AppState>,
    vault: Arc<MemoryVault>,
    events: Arc<Mutex<Vec<Value>>>,
    id: String,
    base: String,
}
impl Lab {
    async fn new(post_backup_failure: bool) -> Self {
        Self::new_with_backup_mismatch(post_backup_failure, false).await
    }

    async fn new_with_backup_mismatch(post_backup_failure: bool, backup_mismatch: bool) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let python = root.join("worker/.venv/bin/python");
        let mut process = Command::new(&python)
            .arg(root.join("tests/lab_server.py"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut output = BufReader::new(process.stdout.take().unwrap());
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        let info: Value = serde_json::from_str(&line).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let worker_args = if post_backup_failure {
            vec!["-u".into(),"-c".into(),"from yigou_worker.__main__ import Worker,main\nimport pathlib,sys\noriginal=Worker.dispatch\ndef dispatch(self,request):\n result=original(self,request)\n if request.get('op')=='configure':pathlib.Path(sys.argv[1]).write_text('fixture Git contention')\n return result\nWorker.dispatch=dispatch\nmain()".into(),dir.path().join("backups/.git/index.lock").to_string_lossy().into_owned()]
        } else if backup_mismatch {
            // The third backup RPC is the pre-write encrypted snapshot; the
            // earlier reads and subsequent live check still see the baseline.
            let code = "from yigou_worker.__main__ import Worker,main\noriginal=Worker.dispatch\nreads=0\ndef dispatch(self,request):\n global reads\n result=original(self,request)\n if request.get('op')=='backup':\n  reads += 1\n  if reads==3: result['content']=result['content'].replace('sysname LAB','sysname TRANSIENT-EDIT')\n return result\nWorker.dispatch=dispatch\nmain()";
            vec!["-u".into(), "-c".into(), code.into()]
        } else {
            vec!["-m".into(), "yigou_worker".into()]
        };
        let vault = Arc::new(MemoryVault::default());
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let state = AppState::new(
            dir.path().into(),
            WorkerClient::new(python, worker_args),
            vault.clone(),
            Arc::new(move |e| captured.lock().unwrap().push(e)),
        )
        .unwrap();
        let device = state
            .add(
                NewDevice {
                    name: "CONFIG-LAB".into(),
                    host: "127.0.0.1".into(),
                    port: info["port"].as_u64().unwrap() as u16,
                    vendor: "huawei".into(),
                    username: "lab".into(),
                    auto_backup: false,
                    backup_minutes: 5,
                },
                Credential {
                    kind: "password".into(),
                    secret: "fixture-only".into(),
                    key_path: None,
                },
            )
            .await
            .unwrap();
        let id = device["id"].as_str().unwrap().to_string();
        state
            .trust(
                &id,
                info["algorithm"].as_str().unwrap(),
                info["key"].as_str().unwrap(),
                info["fingerprint"].as_str().unwrap(),
            )
            .await
            .unwrap();
        state.connect(&id).await.unwrap();
        Self {
            process,
            _dir: dir,
            state,
            vault,
            events,
            id,
            base: info["model_base"].as_str().unwrap().to_string(),
        }
    }
    async fn finish(mut self) {
        self.state.disconnect(&self.id).await.unwrap();
        drop(self.process.stdin.take());
        self.process.wait().await.unwrap();
    }
}
fn args(marker: &str) -> ConfigArgs {
    serde_json::from_value(json!({"commands":["interface GE0/0/1",format!("description {marker}")],"save":true,"checks":[{"query":"config","contains":[format!("description {marker}")],"not_contains":[]}]})).unwrap()
}
fn approval() -> ConfigApproval {
    ConfigApproval {
        reviewed: true,
        backup_ack: true,
        rollback_ack: true,
        phrase: "确认执行".into(),
    }
}
async fn approved(state: &AppState, id: &str, args: ConfigArgs) -> (Value, Value) {
    let plan = state.create_config_plan(id, args).await.unwrap();
    let result = state
        .execute_config_plan(
            id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap();
    (plan, result)
}

#[tokio::test]
async fn user_opt_in_allows_one_low_risk_autonomous_change_with_readback() {
    let lab = Lab::new(false).await;
    lab.state
        .model_save(
            &format!("{}/configure/v1", lab.base),
            "fixture-model",
            "fixture-key",
        )
        .unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let prompt = "将 GE0/0/1 描述改为 AI-INTEGRATION 并保存配置";
    assert!(
        lab.state
            .autonomous_chat(&lab.id, prompt, &request, false)
            .await
            .unwrap_err()
            .to_string()
            .contains("CONSENT_REQUIRED")
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    let result = lab
        .state
        .autonomous_chat(&lab.id, prompt, &request, true)
        .await
        .unwrap();
    assert_eq!(result["configuration"]["status"], "succeeded", "{result}");
    assert_eq!(result["configuration"]["approval"]["mode"], "autonomous");
    assert_eq!(result["configuration"]["save_status"], "verified");
    assert_eq!(result["executed"], true);
    assert_eq!(result["plan"]["status"], "consumed");
    assert!(
        lab.events.lock().unwrap().iter().any(|event| {
            event["kind"] == "ai_config_result"
                && event["request_id"] == result["plan"]["id"]
                && event["source_request_id"] == request
        }),
        "执行记录编号必须保留计划 ID，同时关联原始对话请求"
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 2);
    assert!(
        lab.state
            .autonomous_chat(&lab.id, prompt, &request, true)
            .await
            .unwrap_err()
            .to_string()
            .contains("AUTONOMY_REQUEST_REPLAY")
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 2);
    lab.finish().await;
}

#[tokio::test]
async fn autonomous_scope_rejects_disruptive_commands_before_backup() {
    let lab = Lab::new(false).await;
    let request = uuid::Uuid::new_v4().to_string();
    for commands in [
        vec!["interface GE0/0/1", "shutdown"],
        vec!["interface GE0/0/1", "ip address 10.0.0.1 255.255.255.0"],
    ] {
        let plan = lab
            .state
            .create_ai_config_plan(
                &lab.id,
                &request,
                ConfigArgs {
                    commands: commands.into_iter().map(str::to_owned).collect(),
                    save: false,
                    checks: vec![crate::devices::configure::Verification {
                        query: "config".into(),
                        contains: vec!["shutdown".into()],
                        not_contains: vec![],
                    }],
                },
            )
            .await
            .unwrap();
        assert!(
            lab.state
                .execute_autonomous_plan(&lab.id, plan["id"].as_str().unwrap(), &request)
                .await
                .unwrap_err()
                .to_string()
                .contains("AUTONOMY_SCOPE_DENIED")
        );
    }
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    lab.finish().await;
}

#[tokio::test]
async fn auto_plan_must_be_bound_to_its_own_user_request() {
    let lab = Lab::new(false).await;
    let request = uuid::Uuid::new_v4().to_string();
    let plan = lab
        .state
        .create_ai_config_plan(&lab.id, &request, args("AI-INTEGRATION"))
        .await
        .unwrap();
    let err = lab
        .state
        .execute_autonomous_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            &uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("AUTONOMY_SOURCE_MISMATCH"), "{err}");
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    lab.finish().await;
}

#[tokio::test]
async fn expired_or_session_changed_plan_never_reaches_device() {
    let lab = Lab::new(false).await;
    let before = lab.state.store.backups(&lab.id).unwrap().len();
    let invalid = ConfigArgs {
        commands: vec!["vlan 120".into(), "quit".into()],
        save: false,
        checks: vec![crate::devices::configure::Verification {
            query: "config".into(),
            contains: vec!["vlan 120".into()],
            not_contains: vec![],
        }],
    };
    assert!(
        lab.state
            .create_config_plan(&lab.id, invalid)
            .await
            .unwrap_err()
            .to_string()
            .contains("CONFIG_INVALID")
    );
    let plan = lab
        .state
        .create_config_plan(&lab.id, args("EXPIRE-TEST"))
        .await
        .unwrap();
    let id = plan["id"].as_str().unwrap();
    let hash = plan["request_hash"].as_str().unwrap();
    let original_session = lab.state.session(&lab.id).await.unwrap();
    lab.state
        .sessions
        .lock()
        .await
        .insert(lab.id.clone(), "changed-session".into());
    assert!(
        lab.state
            .execute_config_plan(&lab.id, id, hash, approval())
            .await
            .unwrap_err()
            .to_string()
            .contains("SESSION_CHANGED")
    );
    lab.state
        .sessions
        .lock()
        .await
        .insert(lab.id.clone(), original_session);
    lab.state
        .store
        .0
        .lock()
        .unwrap()
        .execute(
            "UPDATE config_plans SET body=json_set(body,'$.expires_at',0) WHERE id=?1",
            [id],
        )
        .unwrap();
    assert!(
        lab.state
            .execute_config_plan(&lab.id, id, hash, approval())
            .await
            .unwrap_err()
            .to_string()
            .contains("PLAN_EXPIRED")
    );
    assert!(lab.state.store.config_run(id).unwrap().is_none());
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), before);
    lab.finish().await;
}

#[tokio::test]
async fn cancelled_configuration_is_durably_unknown_and_is_never_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("write-received");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let code = "import sys,json,pathlib,time,hashlib\nfor line in sys.stdin:\n r=json.loads(line)\n if r['op']=='configure':\n  pathlib.Path(sys.argv[1]).write_text('received');time.sleep(30)\n out={'content':'#\\nsysname LAB\\nreturn','redacted_sha256':hashlib.sha256(b'#\\nsysname LAB\\nreturn').hexdigest()} if r['op']=='backup' else {'output':'Huawei fixture'}\n print(json.dumps({'id':r['id'],'ok':True,'result':out}),flush=True)";
    let state = AppState::new(
        dir.path().into(),
        WorkerClient::new(
            root.join("worker/.venv/bin/python"),
            vec![
                "-u".into(),
                "-c".into(),
                code.into(),
                marker.to_string_lossy().into_owned(),
            ],
        ),
        Arc::new(MemoryVault::default()),
        Arc::new(|_| {}),
    )
    .unwrap();
    state
        .store
        .save_device(&json!({"id":"dev","vendor":"huawei"}))
        .unwrap();
    // This worker is intentionally scripted and never opens SSH; give the
    // fixture a stable trust record so the test reaches cancellation logic.
    std::fs::write(state.root.join("hosts/dev"), b"fixture-only-trust").unwrap();
    state.rpc(json!({"op":"ping"})).await.unwrap();
    state
        .sessions
        .lock()
        .await
        .insert("dev".into(), "fixture-session".into());
    let plan = state
        .create_config_plan("dev", args("CANCELLED"))
        .await
        .unwrap();
    let request = plan["id"].as_str().unwrap().to_string();
    let hash = plan["request_hash"].as_str().unwrap().to_string();
    let (running, request_clone) = (state.clone(), request.clone());
    let task = tokio::spawn(async move {
        running
            .execute_config_plan("dev", &request_clone, &hash, approval())
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !marker.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let record = state.store.config_run(&request).unwrap().unwrap();
    assert_eq!(record["status"], "unknown");
    assert_eq!(record["write_attempted"], true);
    assert_eq!(
        state.store.tasks(Some(&request)).unwrap()[0]["status"],
        "failed"
    );
    assert!(!state.worker.lock().await.is_started());
    assert_eq!(
        state
            .execute_config_plan(
                "dev",
                &request,
                plan["request_hash"].as_str().unwrap(),
                approval()
            )
            .await
            .unwrap(),
        record
    );
    assert!(!state.worker.lock().await.is_started());
}

#[tokio::test]
async fn ai_writes_backs_up_verifies_and_preview_has_no_write_tool() {
    let lab = Lab::new(false).await;
    lab.state
        .model_save(
            &format!("{}/configure/v1", lab.base),
            "fixture-model",
            "fixture-key",
        )
        .unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let response = lab
        .state
        .chat(
            &lab.id,
            "将 GE0/0/1 描述改为 AI-INTEGRATION 并保存配置",
            &request,
            false,
        )
        .await
        .unwrap();
    // Chat has no write tool, even if the user asks for a configuration change.
    assert_eq!(response["executed"], false, "{response}");
    assert!(response["plan"].is_null());
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    let preview = lab
        .state
        .chat(
            &lab.id,
            "将 GE0/0/1 描述改为 AI-INTEGRATION 并保存配置",
            &uuid::Uuid::new_v4().to_string(),
            true,
        )
        .await
        .unwrap();
    let plan = &preview["plan"];
    assert!(plan.is_object(), "{preview}");
    assert_eq!(plan["status"], "pending");
    assert!(!plan["config"]["commands"].as_array().unwrap().is_empty());
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    let request = plan["id"].as_str().unwrap().to_string();
    // Omitted acknowledgements and wrong hash are rejected before backup or traffic.
    assert!(
        lab.state
            .execute_config_plan(
                &lab.id,
                &request,
                plan["request_hash"].as_str().unwrap(),
                ConfigApproval {
                    reviewed: true,
                    backup_ack: true,
                    rollback_ack: false,
                    phrase: "确认执行".into()
                }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("APPROVAL_REQUIRED")
    );
    assert!(
        lab.state
            .execute_config_plan(&lab.id, &request, "tampered", approval())
            .await
            .unwrap_err()
            .to_string()
            .contains("HASH_MISMATCH")
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    let executed = lab
        .state
        .execute_config_plan(
            &lab.id,
            &request,
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap();
    assert_eq!(executed["status"], "succeeded", "{executed}");
    assert_eq!(executed["save_status"], "verified");
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 2);
    let before = &executed["before_backup"];
    let after = &executed["after_backup"];
    let before =
        crate::backups::verify(&lab.state.root.join("backups"), lab.vault.as_ref(), before)
            .unwrap();
    let after =
        crate::backups::verify(&lab.state.root.join("backups"), lab.vault.as_ref(), after).unwrap();
    assert!(
        !before["preview"]
            .as_str()
            .unwrap()
            .contains("AI-INTEGRATION")
    );
    assert!(
        after["preview"]
            .as_str()
            .unwrap()
            .contains("AI-INTEGRATION")
    );
    let duplicate = lab
        .state
        .execute_config_plan(
            &lab.id,
            &request,
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate, executed);
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 2);
    assert!(
        lab.state
            .execute_config_plan(&lab.id, &request, "wrong-hash", approval())
            .await
            .unwrap_err()
            .to_string()
            .contains("HASH_MISMATCH")
    );
    // A malicious fixture attempts an unregistered write tool even in preview mode.
    lab.state
        .model_save(
            &format!("{}/force-configure/v1", lab.base),
            "fixture-model",
            "",
        )
        .unwrap();
    let preview = lab
        .state
        .chat(
            &lab.id,
            "只预览，不执行",
            &uuid::Uuid::new_v4().to_string(),
            true,
        )
        .await;
    if let Ok(preview) = preview {
        assert_eq!(preview["executed"], false);
        assert_eq!(preview["preview_only"], true);
    }
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 2);
    let audit = serde_json::to_string(&lab.state.store.tasks(None).unwrap()).unwrap();
    let events = serde_json::to_string(&*lab.events.lock().unwrap()).unwrap();
    for text in [&audit, &events] {
        for secret in [
            "fixture-only",
            "fixture-sensitive",
            "fixture-key",
            "after_content",
        ] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
    }
    assert!(
        lab.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "ai_config_result" && e["result"]["status"] == "succeeded")
    );
    lab.finish().await;
}

#[tokio::test]
async fn ai_decoupled_checks_never_report_success() {
    let lab = Lab::new(false).await;
    lab.state
        .model_save(
            &format!("{}/weak-configure/v1", lab.base),
            "fixture-model",
            "fixture-key",
        )
        .unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    let response = lab
        .state
        .chat(
            &lab.id,
            "将 GE0/0/1 描述改为 AI-WEAK 并保存配置",
            &request,
            true,
        )
        .await
        .unwrap();
    let plan = &response["plan"];
    assert!(plan.is_object(), "{response}");
    let record = lab
        .state
        .execute_config_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap();
    assert_eq!(record["status"], "applied_unverified", "{record}");
    assert!(
        record["error"]
            .as_str()
            .unwrap()
            .contains("VERIFY_INCOMPLETE")
    );
    assert_eq!(record["save_status"], "acknowledged");
    // The written value did land on the device; the weak evidence still cannot
    // certify it, and the visible text must not claim success.
    assert!(
        lab.state.query(&lab.id, "config").await.unwrap()["output"]
            .as_str()
            .unwrap()
            .contains("AI-WEAK")
    );
    assert_eq!(record["write_attempted"], true);
    // Retrying with the same reviewed plan only returns the durable result.
    assert_eq!(
        lab.state
            .execute_config_plan(
                &lab.id,
                plan["id"].as_str().unwrap(),
                plan["request_hash"].as_str().unwrap(),
                approval()
            )
            .await
            .unwrap(),
        record
    );
    lab.finish().await;
}

#[tokio::test]
async fn rust_gate_rejects_checks_without_config_readback_before_any_backup() {
    // No config/startup condition: the request must be rejected client-side and
    // no backup must be taken, no worker session touched.
    let lab = Lab::new(false).await;
    let before = lab.state.store.backups(&lab.id).unwrap().len();
    let weak = serde_json::from_value::<ConfigArgs>(json!({
        "commands": ["interface GE0/0/1", "description NEVER-SENT"],
        "save": true,
        "checks": [{"query": "version", "contains": ["Huawei"], "not_contains": []}]
    }))
    .unwrap();
    let err = lab
        .state
        .create_config_plan(&lab.id, weak)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("核验不充分"), "{err}");
    assert!(
        !lab.state.query(&lab.id, "config").await.unwrap()["output"]
            .as_str()
            .unwrap()
            .contains("NEVER-SENT")
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), before);
    lab.finish().await;
}

#[tokio::test]
async fn pre_backup_failure_never_sends_configuration_and_does_not_replay() {
    let lab = Lab::new(false).await;
    let backup = lab.state.backup(&lab.id).await.unwrap();
    lab.vault
        .delete(backup["key_id"].as_str().unwrap())
        .unwrap();
    let (plan, result) = approved(&lab.state, &lab.id, args("MUST-NOT-APPLY")).await;
    let request = plan["id"].as_str().unwrap().to_string();
    assert_eq!(result["status"], "not_started");
    assert_eq!(result["write_attempted"], false);
    let query = lab.state.query(&lab.id, "config").await.unwrap();
    assert!(!query["output"].as_str().unwrap().contains("MUST-NOT-APPLY"));
    assert_eq!(
        lab.state
            .execute_config_plan(
                &lab.id,
                &request,
                plan["request_hash"].as_str().unwrap(),
                approval()
            )
            .await
            .unwrap(),
        result
    );
    assert_eq!(
        lab.state.store.tasks(Some(&request)).unwrap()[0]["status"],
        "failed"
    );
    lab.finish().await;
}

#[tokio::test]
async fn post_backup_failure_reports_applied_configuration_without_false_success() {
    let lab = Lab::new(true).await;
    let (plan, result) = approved(&lab.state, &lab.id, args("POST-BACKUP-FAIL")).await;
    let request = plan["id"].as_str().unwrap().to_string();
    assert_eq!(result["status"], "applied_backup_failed", "{result}");
    assert_eq!(result["write_attempted"], true);
    assert!(result["before_backup"].is_object());
    assert!(result["after_backup"].is_null());
    assert!(
        lab.state.query(&lab.id, "config").await.unwrap()["output"]
            .as_str()
            .unwrap()
            .contains("POST-BACKUP-FAIL")
    );
    assert_eq!(
        lab.state.store.tasks(Some(&request)).unwrap()[0]["status"],
        "failed"
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 1);
    lab.finish().await;
}

#[tokio::test]
async fn changed_trusted_host_file_invalidates_unconsumed_plan() {
    let lab = Lab::new(false).await;
    let plan = lab
        .state
        .create_config_plan(&lab.id, args("HOST-BOUND"))
        .await
        .unwrap();
    let host_file = lab.state.root.join("hosts").join(&lab.id);
    let original = std::fs::read(&host_file).unwrap();
    let mut changed = original.clone();
    changed.push(b'\n');
    std::fs::write(&host_file, changed).unwrap();
    let error = lab
        .state
        .execute_config_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("HOST_KEY_CHANGED"), "{error}");
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    assert!(
        lab.state
            .store
            .config_run(plan["id"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    std::fs::write(&host_file, original).unwrap();
    lab.finish().await;
}

#[tokio::test]
async fn transient_change_in_pre_write_backup_blocks_execution() {
    let lab = Lab::new_with_backup_mismatch(false, true).await;
    let plan = lab
        .state
        .create_config_plan(&lab.id, args("NEVER-WRITE-TRANSIENT"))
        .await
        .unwrap();
    let record = lab
        .state
        .execute_config_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap();
    assert_eq!(record["status"], "not_started", "{record}");
    assert_eq!(record["write_attempted"], false);
    assert!(
        record["error"]
            .as_str()
            .unwrap()
            .contains("CONFIG_BASELINE_CHANGED")
    );
    assert!(record["before_backup"].is_object());
    assert!(
        !lab.state.query(&lab.id, "config").await.unwrap()["output"]
            .as_str()
            .unwrap()
            .contains("NEVER-WRITE-TRANSIENT")
    );
    lab.finish().await;
}

#[tokio::test]
async fn observed_ai_configuration_must_match_the_same_raw_plan_snapshot() {
    use crate::devices::configure::config_snapshot_hash;
    let lab = Lab::new(false).await;
    let session = lab.state.session(&lab.id).await.unwrap();
    let first = lab
        .state
        .query_with_private_evidence(&lab.id)
        .await
        .unwrap();
    let observed = config_snapshot_hash(&first).unwrap();
    let original_raw = first["raw_sha256"].as_str().unwrap().to_string();
    // A second administrator changes the device after model observation but
    // before the proposal. The new snapshot must reject the old observation.
    let changed = lab.state.rpc(json!({
        "op":"configure", "session":session,
        "commands":["interface GE0/0/1", "description EXTERNAL-BEFORE-PLAN"],
        "save":false,
        "checks":[{"query":"config","contains":["description EXTERNAL-BEFORE-PLAN"],"not_contains":[]}]
    })).await.unwrap();
    assert_eq!(changed["status"], "succeeded");
    let before = lab.state.store.backups(&lab.id).unwrap().len();
    let request = uuid::Uuid::new_v4().to_string();
    let error = lab
        .state
        .create_bound_ai_config_plan(
            &lab.id,
            &request,
            args("NOT-AUTHORIZED"),
            &session,
            &observed,
            &original_raw,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CONFIG_BASELINE_CHANGED"), "{error}");
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), before);
    lab.finish().await;
}

#[tokio::test]
async fn masked_password_only_drift_cannot_bind_a_new_ai_plan() {
    use crate::devices::configure::config_snapshot_hash;
    use sha2::{Digest, Sha256};

    let lab = Lab::new(false).await;
    let session = lab.state.session(&lab.id).await.unwrap();
    let observed = lab
        .state
        .query_with_private_evidence(&lab.id)
        .await
        .unwrap();
    let redacted = config_snapshot_hash(&observed).unwrap();
    let original_raw = observed["raw_sha256"].as_str().unwrap();
    // The ordinary query endpoint must never return the host-only raw digest.
    assert!(
        lab.state
            .query(&lab.id, "config")
            .await
            .unwrap()
            .get("raw_sha256")
            .is_none()
    );
    let snapshot = lab
        .state
        .rpc(json!({"op":"backup","session":session}))
        .await
        .unwrap();
    let before = snapshot["content"].as_str().unwrap();
    assert!(before.contains("fixture-sensitive"));
    let after = before.replace("fixture-sensitive", "other-sensitive-value");
    assert_ne!(before, after);
    assert_eq!(
        redacted,
        format!(
            "{:x}",
            Sha256::digest(crate::security::redact(&after).as_bytes())
        )
    );
    let after_raw = format!("{:x}", Sha256::digest(after.as_bytes()));
    assert_ne!(original_raw, after_raw);
    let request = uuid::Uuid::new_v4().to_string();
    let error = lab
        .state
        .create_bound_ai_config_plan(
            &lab.id,
            &request,
            args("MUST-NOT-WRITE"),
            &session,
            &redacted,
            &after_raw,
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CONFIG_BASELINE_CHANGED"), "{error}");
    assert!(lab.state.store.config_run(&request).unwrap().is_none());
    lab.finish().await;
}

#[tokio::test]
async fn changed_running_configuration_blocks_an_unconsumed_plan_without_backup() {
    let lab = Lab::new(false).await;
    let plan = lab
        .state
        .create_config_plan(&lab.id, args("PLANNED-CHANGE"))
        .await
        .unwrap();
    assert!(
        plan.get("config_sha256").is_none(),
        "基线摘要只留在可信本地记录"
    );
    // The scripted peer stands for a second administrator changing the device
    // through a different connection while the original query session survives.
    let session = lab.state.session(&lab.id).await.unwrap();
    let changed = lab.state.rpc(json!({
        "op":"configure", "session":session,
        "commands":["interface GE0/0/1", "description EXTERNAL-CHANGE"],
        "save":false,
        "checks":[{"query":"config","contains":["description EXTERNAL-CHANGE"],"not_contains":[]}]
    })).await.unwrap();
    assert_eq!(changed["status"], "succeeded");
    let error = lab
        .state
        .execute_config_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CONFIG_BASELINE_CHANGED"), "{error}");
    assert!(
        lab.state
            .store
            .config_run(plan["id"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(lab.state.store.backups(&lab.id).unwrap().len(), 0);
    lab.finish().await;
}

#[tokio::test]
async fn open_manual_terminal_blocks_automatic_write_before_reservation() {
    let lab = Lab::new(false).await;
    let plan = lab
        .state
        .create_config_plan(&lab.id, args("NO-PTY-RACE"))
        .await
        .unwrap();
    let terminal = lab.state.terminal_open(&lab.id).await.unwrap();
    let terminal_id = terminal["session"].as_str().unwrap();
    let error = lab
        .state
        .execute_config_plan(
            &lab.id,
            plan["id"].as_str().unwrap(),
            plan["request_hash"].as_str().unwrap(),
            approval(),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("TERMINAL_ACTIVE"), "{error}");
    assert!(
        lab.state
            .store
            .config_run(plan["id"].as_str().unwrap())
            .unwrap()
            .is_none()
    );
    assert!(lab.state.store.backups(&lab.id).unwrap().is_empty());
    lab.state.terminal_close(terminal_id).await.unwrap();
    lab.finish().await;
}

#[tokio::test]
async fn terminal_close_retains_ownership_until_config_lock_is_released() {
    let lab = Lab::new(false).await;
    let terminal = lab.state.terminal_open(&lab.id).await.unwrap();
    let session = terminal["session"].as_str().unwrap().to_string();
    let config_guard = lab.state.config_lock.lock().await;
    let (sent, ready) = tokio::sync::oneshot::channel();
    let state = lab.state.clone();
    let id = session.clone();
    let close = tokio::spawn(async move {
        let _ = sent.send(());
        state.terminal_close(&id).await
    });
    ready.await.unwrap();
    tokio::task::yield_now().await;
    assert!(lab.state.terminals.lock().await.contains_key(&session));
    drop(config_guard);
    close.await.unwrap().unwrap();
    assert!(!lab.state.terminals.lock().await.contains_key(&session));
    lab.finish().await;
}

#[tokio::test]
async fn cancelled_terminal_close_keeps_occupancy_until_ssh_teardown() {
    let lab = Lab::new(false).await;
    let opened = lab.state.terminal_open(&lab.id).await.unwrap();
    let session = opened["session"].as_str().unwrap().to_string();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let state = lab.state.clone();
    let id = session.clone();
    let blocking_read = tokio::spawn(async move {
        state
            .hold_terminal_worker_for_test(&id, entered_tx, release_rx)
            .await
    });
    entered_rx.await.unwrap();
    let state = lab.state.clone();
    let id = session.clone();
    let closing = tokio::spawn(async move { state.terminal_close(&id).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while lab.state.config_lock.try_lock().is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    closing.abort();
    assert!(closing.await.unwrap_err().is_cancelled());
    assert!(
        lab.state.terminals.lock().await.contains_key(&session),
        "取消关闭不能提前消除仍存在的 SSH 终端占用"
    );
    let _ = release_tx.send(());
    blocking_read.await.unwrap().unwrap();
    lab.state.terminal_close(&session).await.unwrap();
    assert!(!lab.state.terminals.lock().await.contains_key(&session));
    lab.finish().await;
}

#[tokio::test]
async fn host_key_retrust_with_terminal_does_not_deadlock() {
    let lab = Lab::new(false).await;
    let _terminal = lab.state.terminal_open(&lab.id).await.unwrap();
    let known = std::fs::read_to_string(lab.state.root.join("hosts").join(&lab.id)).unwrap();
    let fields: Vec<&str> = known.split_whitespace().collect();
    let confirmed = crate::security::fingerprint(fields[2]).unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        lab.state.trust(&lab.id, fields[1], fields[2], &confirmed),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(lab.state.terminals.lock().await.is_empty());
    lab.finish().await;
}

#[tokio::test]
async fn human_terminal_input_is_rejected_while_configuration_lock_is_held() {
    let lab = Lab::new(false).await;
    let _guard = lab.state.config_lock.lock().await;
    let error = lab
        .state
        .terminal_write("nonexistent", "")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("CONFIG_BUSY"), "{error}");
    drop(_guard);
    lab.finish().await;
}

#[test]
fn generated_config_plans_reject_sensitive_commands_before_persistence() {
    for command in [
        "local-user audit password cipher EXAMPLE123",
        "description 设备密码：EXAMPLE123",
        "description API_KEY=EXAMPLE123",
        "snmp-agent community read EXAMPLE123",
    ] {
        let args: ConfigArgs = serde_json::from_value(json!({
            "commands": [command], "save": false,
            "checks": [{"query":"config","contains":["sysname LAB"],"not_contains":[]}]
        }))
        .unwrap();
        assert!(
            args.validate()
                .unwrap_err()
                .to_string()
                .contains("CONFIG_SENSITIVE_DENIED")
        );
    }
    let safe: ConfigArgs = serde_json::from_value(json!({
        "commands":["interface GE0/0/1","description UPLINK-LAB"], "save":false,
        "checks":[{"query":"config","contains":["description UPLINK-LAB"],"not_contains":[]}]
    }))
    .unwrap();
    assert!(safe.validate().is_ok());
}

#[test]
fn config_snapshot_must_have_complete_observed_data() {
    use crate::devices::configure::config_snapshot_hash;
    let normal = json!({"output":"#\\ninterface GE0/0/1\\n description OLD\\n#"});
    assert_eq!(
        config_snapshot_hash(&normal).unwrap(),
        config_snapshot_hash(&normal).unwrap()
    );
    assert!(config_snapshot_hash(&json!({"output":"   "})).is_err());
    assert!(config_snapshot_hash(&json!({"output":"partial","truncated":true})).is_err());
}
