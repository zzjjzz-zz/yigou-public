use super::AppState;
use crate::{backups, security::redact};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigArgs {
    pub commands: Vec<String>,
    pub save: bool,
    #[serde(default)]
    pub checks: Vec<Verification>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Verification {
    pub query: String,
    #[serde(default)]
    pub contains: Vec<String>,
    #[serde(default)]
    pub not_contains: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigApproval {
    pub reviewed: bool,
    pub backup_ack: bool,
    pub rollback_ack: bool,
    pub phrase: String,
}

fn config_hash(args: &ConfigArgs) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(args)?)))
}

// A stable hash of the actual redacted running-config response, not a model summary.
// Empty or truncated observations cannot authorize a change.
pub(crate) fn config_snapshot_hash(result: &Value) -> Result<String> {
    let output = result["output"]
        .as_str()
        .context("CONFIG_BASELINE_MISSING: 未取得当前配置")?;
    if output.trim().is_empty() || output.len() > 48000 || result["truncated"] == true {
        bail!("CONFIG_BASELINE_INCOMPLETE: 当前配置为空、过长或已截断，不能生成或执行计划");
    }
    Ok(format!("{:x}", Sha256::digest(output.as_bytes())))
}

/// Autonomous execution is limited to non-disruptive changes whose intent can
/// be validated by a small deterministic grammar, not by a model's self-rating.
fn validate_autonomous_scope(args: &ConfigArgs) -> Result<()> {
    if args.commands.len() > 12 {
        bail!("AUTONOMY_SCOPE_DENIED: 自主执行最多允许 12 条配置命令，超出需人工审批");
    }
    let mut context = "";
    let mut vlan_needs_name = false;
    let mut changes = Vec::new();
    for command in &args.commands {
        let trimmed = command.trim();
        let lower = trimmed.to_ascii_lowercase();
        let (kind, rest) = lower.split_once(' ').unwrap_or(("", ""));
        let safe_identifier = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
        };
        let safe_description = |s: &str| {
            !s.is_empty()
                && s.len() <= 80
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || " _-./".contains(c))
        };
        match kind {
            "interface"
                if safe_identifier(rest)
                    && [
                        "ge",
                        "gigabitethernet",
                        "ten-gigabitethernet",
                        "tengigabitethernet",
                        "ethernet",
                        "xgigabitethernet",
                        "fortygige",
                        "hundredgige",
                    ]
                    .iter()
                    .any(|prefix| {
                        rest.strip_prefix(*prefix).is_some_and(|suffix| {
                            !suffix.is_empty()
                                && suffix.chars().all(|c| c.is_ascii_digit() || c == '/')
                        })
                    }) =>
            {
                if vlan_needs_name {
                    bail!("AUTONOMY_SCOPE_DENIED: 自主创建 VLAN 必须同时设置可回读的名称");
                }
                context = "interface"
            }
            "vlan" if rest.parse::<u16>().is_ok_and(|v| (2..=4094).contains(&v)) => {
                if vlan_needs_name {
                    bail!("AUTONOMY_SCOPE_DENIED: 自主创建 VLAN 必须同时设置可回读的名称");
                }
                context = "vlan";
                vlan_needs_name = true;
                changes.push(lower.clone());
            }
            "description" if context == "interface" && safe_description(rest) => {
                changes.push(lower.clone())
            }
            "name" if context == "vlan" && safe_description(rest) => {
                vlan_needs_name = false;
                changes.push(lower.clone());
            }
            _ => bail!("AUTONOMY_SCOPE_DENIED: 此命令超出低风险自主执行范围，请人工审阅"),
        }
    }
    if vlan_needs_name
        || changes.is_empty()
        || changes.iter().any(|expected| {
            !args.checks.iter().any(|check| {
                check.query == "config"
                    && check
                        .contains
                        .iter()
                        .any(|actual| actual.to_ascii_lowercase().contains(expected))
            })
        })
    {
        bail!("AUTONOMY_SCOPE_DENIED: 自主执行的配置回读必须包含每项实际修改值");
    }
    Ok(())
}
impl ConfigArgs {
    pub fn validate(&self) -> Result<()> {
        if self.commands.is_empty()
            || self.commands.len() > 256
            || self
                .commands
                .iter()
                .any(|c| c.trim().is_empty() || c.len() > 4096 || c.chars().any(char::is_control))
            || self.commands.iter().map(String::len).sum::<usize>() > 65536
        {
            bail!("CONFIG_INVALID: 配置需为 1 至 256 条单行命令，总长度不超过 64 KiB");
        }
        if self.commands.iter().any(|command| {
            let lower = command.trim().to_ascii_lowercase();
            let first = lower.split_whitespace().next().unwrap_or("");
            // Reject abbreviations too: these operations belong to the driver or
            // require an explicit manual terminal, never a generated batch.
            command.chars().any(|c| matches!(c, ';' | '|' | '&' | '`'))
                || [
                    "quit",
                    "exit",
                    "end",
                    "return",
                    "save",
                    "write",
                    "system-view",
                    "configure",
                    "reboot",
                    "reload",
                    "reset",
                    "erase",
                    "delete",
                    "format",
                    "shell",
                    "python",
                    "tclsh",
                    "boot",
                    "startup",
                    "copy",
                    "do",
                    "run",
                    "enable",
                    "disable",
                ]
                .iter()
                .any(|word| word.starts_with(first))
        }) {
            bail!("CONFIG_INVALID: 模式控制、保存、破坏性或组合命令不得混入配置批次");
        }
        // Plan commands and verification strings are persisted in local SQLite
        // and may be shown to the model/UI. Secrets belong in the manual SSH PTY.
        if self.commands.iter().any(|c| redact(c) == "[REDACTED]")
            || self
                .checks
                .iter()
                .flat_map(|check| check.contains.iter().chain(&check.not_contains))
                .any(|v| redact(v) == "[REDACTED]")
        {
            bail!("CONFIG_SENSITIVE_DENIED: 自动配置计划不得包含密码、口令或密钥，请使用人工终端");
        }
        if self.checks.len() > 16
            || self.checks.iter().any(|c| {
                !matches!(
                    c.query.as_str(),
                    "version" | "ports" | "config" | "neighbors" | "startup"
                ) || c.contains.len() > 32
                    || c.not_contains.len() > 32
                    || (c.contains.is_empty() && c.not_contains.is_empty())
                    || c.contains
                        .iter()
                        .chain(&c.not_contains)
                        .any(|s| s.is_empty() || s.len() > 4096)
            })
        {
            bail!("CONFIG_INVALID: 核验条件格式无效");
        }
        // Structural gate mirroring the worker: without a config/startup read-back
        // condition, no evidence path could ever prove the user's goal.
        if !self
            .checks
            .iter()
            .any(|c| c.query == "config" || c.query == "startup")
        {
            bail!("CONFIG_INVALID: 核验不充分：缺少对运行或启动配置的回读条件");
        }
        Ok(())
    }
}

/// Dropping a write future must leave an honest, durable outcome, never a retryable success.
struct RunGuard<'a> {
    state: &'a AppState,
    id: String,
    finished: bool,
}
impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if !self.finished
            && let Ok(Some(mut record)) = self.state.store.config_run(&self.id)
        {
            record["status"] = json!(if record["write_attempted"] == true {
                "unknown"
            } else {
                "not_started"
            });
            record["error"] = json!("CONFIG_INTERRUPTED: 任务已取消或超时，未自动重放配置");
            let _ = self.state.store.update_config(&record, true);
            self.state
                .emit(json!({"kind":"ai_config_result","request_id":self.id,
                "source_request_id":record["source_request_id"],
                "device_id":record["device_id"],"result":record}));
        }
    }
}

impl AppState {
    fn plan_host_hash(&self, id: &str) -> Result<String> {
        let bytes = std::fs::read(self.root.join("hosts").join(id))
            .context("HOST_KEY_UNAVAILABLE: 缺少已核实主机密钥")?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    // Read the complete validated running configuration through the existing
    // backup RPC and retain only its digest. Redacted UI output cannot detect
    // a changed password or other sensitive configuration line.
    async fn full_config_digest(&self, id: &str) -> Result<String> {
        let session = self.session(id).await?;
        let response = self.rpc(json!({"op":"backup","session":session})).await?;
        let content = response["content"]
            .as_str()
            .context("CONFIG_BASELINE_INCOMPLETE: 设备未返回完整配置")?;
        if content.trim().is_empty() {
            bail!("CONFIG_BASELINE_INCOMPLETE: 配置内容为空");
        }
        Ok(format!("{:x}", Sha256::digest(content.as_bytes())))
    }

    /// Create a sealed, session-bound proposal. No worker configure operation is possible here.
    pub async fn create_config_plan(&self, id: &str, args: ConfigArgs) -> Result<Value> {
        self.create_config_plan_with_origin(id, args, None, None, None, None)
            .await
    }

    pub async fn create_ai_config_plan(
        &self,
        id: &str,
        request_id: &str,
        args: ConfigArgs,
    ) -> Result<Value> {
        uuid::Uuid::parse_str(request_id).context("REQUEST_ID_INVALID")?;
        self.create_config_plan_with_origin(id, args, Some(request_id), None, None, None)
            .await
    }

    pub(crate) async fn create_bound_ai_config_plan(
        &self,
        id: &str,
        request_id: &str,
        args: ConfigArgs,
        session: &str,
        observed_config_sha256: &str,
        observed_raw_sha256: &str,
    ) -> Result<Value> {
        uuid::Uuid::parse_str(request_id).context("REQUEST_ID_INVALID")?;
        self.create_config_plan_with_origin(
            id,
            args,
            Some(request_id),
            Some(session),
            Some(observed_config_sha256),
            Some(observed_raw_sha256),
        )
        .await
    }

    async fn create_config_plan_with_origin(
        &self,
        id: &str,
        args: ConfigArgs,
        origin: Option<&str>,
        expected_session: Option<&str>,
        observed_config_sha256: Option<&str>,
        observed_raw_sha256: Option<&str>,
    ) -> Result<Value> {
        args.validate()?;
        let device = self.store.device(id)?;
        if !super::vendor_allows_configure(device["vendor"].as_str().context("DEVICE_VENDOR")?) {
            bail!("OPERATION_UNSUPPORTED: 该厂商仅支持查询和人工终端，不允许自动配置");
        }
        let _config = self.config_lock.lock().await;
        let session = self.session(id).await?;
        if expected_session.is_some_and(|expected| expected != session) {
            bail!("SESSION_CHANGED: 生成计划期间连接已变化");
        }
        let host_hash = self.plan_host_hash(id)?;
        // One validated worker read yields both the redacted proposal evidence
        // and the raw baseline digest. Two independent reads could bind an AI
        // observation to a different configuration after a concurrent edit.
        let snapshot = self.rpc(json!({"op":"backup","session":session})).await?;
        let content = snapshot["content"]
            .as_str()
            .context("CONFIG_BASELINE_INCOMPLETE: 缺少完整配置快照")?;
        let visible_snapshot = snapshot["redacted_sha256"]
            .as_str()
            .context("CONFIG_BASELINE_INCOMPLETE: 缺少同源脱敏配置摘要")?;
        if visible_snapshot.len() != 64 || !visible_snapshot.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("CONFIG_BASELINE_INCOMPLETE: 脱敏配置摘要格式无效");
        }
        if observed_config_sha256.is_some_and(|observed| observed != visible_snapshot) {
            bail!("CONFIG_BASELINE_CHANGED: 查询证据与计划绑定的配置不同，请重新检查设备");
        }
        if content.trim().is_empty() {
            bail!("CONFIG_BASELINE_INCOMPLETE: 配置内容为空");
        }
        let baseline = format!("{:x}", Sha256::digest(content.as_bytes()));
        if observed_raw_sha256.is_some_and(|observed| observed != baseline) {
            bail!("CONFIG_BASELINE_CHANGED: 敏感配置等原始内容自模型查询后已变化，请重新生成计划");
        }
        if self.session(id).await? != session || self.plan_host_hash(id)? != host_hash {
            bail!("SESSION_CHANGED: 读取配置期间会话或主机信任记录已变化");
        }
        let hash = config_hash(&args)?;
        let record = json!({
            "id":uuid::Uuid::new_v4().to_string(),"device_id":id,"request_hash":hash,
            "config":args,"session_sha256":format!("{:x}",Sha256::digest(session.as_bytes())),
            "created_at":chrono::Utc::now().to_rfc3339(),
            "source_request_id":origin,"host_sha256":host_hash,
            "config_sha256":baseline,
            "expires_at":(chrono::Utc::now()+chrono::Duration::minutes(15)).timestamp(),
        });
        self.store.save_config_plan(&record)?;
        let mut public = record;
        public.as_object_mut().unwrap().remove("session_sha256");
        public.as_object_mut().unwrap().remove("host_sha256");
        public.as_object_mut().unwrap().remove("config_sha256");
        public["status"] = json!("pending");
        Ok(public)
    }

    /// The sole automatic-write entry. Commands are loaded from the exact reviewed plan,
    /// never supplied anew by an AI response, frontend, or retry.
    pub async fn execute_config_plan(
        &self,
        id: &str,
        plan_id: &str,
        reviewed_hash: &str,
        approval: ConfigApproval,
    ) -> Result<Value> {
        if !(approval.reviewed
            && approval.backup_ack
            && approval.rollback_ack
            && approval.phrase == "确认执行")
        {
            bail!("APPROVAL_REQUIRED: 请审阅命令、确认备份及无自动回滚风险，并输入“确认执行”");
        }
        self.execute_config_plan_inner(id, plan_id, reviewed_hash, None)
            .await
    }

    /// Internal-only entry: a single, explicitly authorized AI request can run
    /// its own low-risk proposal. No Tauri IPC command exposes this shortcut.
    pub(crate) async fn execute_autonomous_plan(
        &self,
        id: &str,
        plan_id: &str,
        request_id: &str,
    ) -> Result<Value> {
        let plan = self.store.config_plan(plan_id)?.context("PLAN_NOT_FOUND")?;
        let hash = plan["request_hash"]
            .as_str()
            .context("PLAN_HASH_MISSING")?
            .to_string();
        self.execute_config_plan_inner(id, plan_id, &hash, Some(request_id))
            .await
    }

    async fn execute_config_plan_inner(
        &self,
        id: &str,
        plan_id: &str,
        reviewed_hash: &str,
        auto_request: Option<&str>,
    ) -> Result<Value> {
        uuid::Uuid::parse_str(plan_id).context("PLAN_ID_INVALID")?;
        let _config = self.config_lock.lock().await;
        let plan = self
            .store
            .config_plan(plan_id)?
            .context("PLAN_NOT_FOUND: 计划不存在或已失效")?;
        if plan["device_id"] != id {
            bail!("PLAN_DEVICE_MISMATCH: 计划设备与当前设备不一致");
        }
        let args: ConfigArgs = serde_json::from_value(plan["config"].clone())?;
        args.validate()?;
        let hash = config_hash(&args)?;
        if plan["request_hash"] != hash || reviewed_hash != hash {
            bail!("PLAN_HASH_MISMATCH: 计划已变化，请重新生成并审阅");
        }
        if let Some(source) = auto_request {
            if plan["source_request_id"] != source {
                bail!("AUTONOMY_SOURCE_MISMATCH: 只能执行本次 AI 请求产生的计划");
            }
            validate_autonomous_scope(&args)?;
        }
        if let Some(record) = self.store.config_run(plan_id)? {
            return Ok(record); // Single-use approval: return evidence, never transmit twice.
        }
        // A PTY write can be accepted by SSH before its device-side command has
        // finished. A mutex on new input alone cannot rule out delayed writes.
        if self
            .terminals
            .lock()
            .await
            .values()
            .any(|terminal| terminal.device_id == id)
        {
            bail!("TERMINAL_ACTIVE: 请先关闭该设备的人工终端，再审批自动配置");
        }
        if plan["status"] != "pending" {
            bail!("PLAN_ALREADY_USED: 计划已使用，不能重复执行");
        }
        if plan["expires_at"].as_i64().unwrap_or(0) <= chrono::Utc::now().timestamp() {
            bail!("PLAN_EXPIRED: 计划超过十五分钟，请重新生成");
        }
        let vendor = self.store.device(id)?["vendor"]
            .as_str()
            .context("DEVICE_VENDOR")?
            .to_string();
        if !super::vendor_allows_configure(&vendor) {
            bail!("OPERATION_UNSUPPORTED: 该厂商暂不支持自动配置下发");
        }
        let session = self.session(id).await?;
        if plan["session_sha256"] != format!("{:x}", Sha256::digest(session.as_bytes())) {
            bail!("SESSION_CHANGED: 设备会话已变化，请重新生成计划");
        }
        if plan["host_sha256"] != self.plan_host_hash(id)? {
            bail!("HOST_KEY_CHANGED: 主机信任记录已变化，请重新生成计划");
        }
        let baseline = plan["config_sha256"]
            .as_str()
            .context("CONFIG_BASELINE_MISSING: 旧计划没有配置基线，必须重新生成")?;
        if baseline.len() != 64 || !baseline.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("CONFIG_BASELINE_INVALID: 计划基线摘要无效，必须重新生成");
        }
        if self.full_config_digest(id).await? != baseline {
            bail!("CONFIG_BASELINE_CHANGED: 设备完整配置与计划生成时不同，禁止执行旧计划");
        }
        if self.session(id).await? != session || plan["host_sha256"] != self.plan_host_hash(id)? {
            bail!("SESSION_CHANGED: 配置预检期间设备会话或主机信任记录已变化");
        }
        let request = serde_json::to_value(&args)?;
        let mut record = json!({"request_id":plan_id,"plan_id":plan_id,"device_id":id,"request_hash":hash,
            "source_request_id":plan["source_request_id"],
            "status":"running","phase":"backup","write_attempted":false,
            "approval":{"mode":if auto_request.is_some(){"autonomous"}else{"manual"},"source_request_id":auto_request,
                "reviewed":auto_request.is_none(),"at":chrono::Utc::now().to_rfc3339()},
            "request":serde_json::from_str::<Value>(&redact_request(&request))?,
            "before_backup":null,"after_backup":null,"error":null});
        self.store.reserve_approved_config(plan_id, &record)?;
        let request_id = plan_id;
        let mut guard = RunGuard {
            state: self,
            id: request_id.into(),
            finished: false,
        };
        self.emit(json!({"kind":"ai_config_start","request_id":request_id,
            "source_request_id":plan["source_request_id"],"device_id":id}));
        let preparation = async {
            let version = self.query(id, "version").await?;
            record["device_evidence"] = version;
            record["before_backup"] = self.backup(id).await?;
            // A saved backup is not enough: authenticate its encrypted bytes and digest.
            let verified_backup = backups::verify(
                &self.root.join("backups"),
                self.vault.as_ref(),
                &record["before_backup"],
            )?;
            // Both checks are necessary: a snapshot can capture a transient
            // third-party edit that disappears before the final live readback.
            if verified_backup["sha256"].as_str() != Some(baseline) {
                bail!("CONFIG_BASELINE_CHANGED: 执行前备份与获批配置基线不一致，禁止下发");
            }
            if self.session(id).await? != session
                || plan["host_sha256"] != self.plan_host_hash(id)?
            {
                bail!("SESSION_CHANGED: 设备连接已变化，配置未发送");
            }
            if plan["expires_at"].as_i64().unwrap_or(0) <= chrono::Utc::now().timestamp() {
                bail!("PLAN_EXPIRED: 备份期间计划已过期，配置未发送");
            }
            if self.full_config_digest(id).await? != baseline {
                bail!("CONFIG_BASELINE_CHANGED: 备份期间设备完整配置变化，停止发送命令");
            }
            if self.session(id).await? != session
                || plan["host_sha256"] != self.plan_host_hash(id)?
            {
                bail!("SESSION_CHANGED: 最终发送前设备连接或主机信任记录已变化");
            }
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(error) = preparation {
            record["status"] = json!("not_started");
            record["error"] = json!(format!(
                "CONFIG_NOT_STARTED: {}",
                redact(&error.to_string())
            ));
        } else {
            record["phase"] = json!("sending");
            record["write_attempted"] = json!(true);
            // Persist before crossing the process/device boundary; never infer no writes after a timeout.
            self.store.update_config(&record, false)?;
            // UI status is driven by persisted execution transitions, not simulated timers.
            // Emit only the phase and boolean evidence, never device config or credentials.
            self.emit(json!({"kind":"ai_config_phase","request_id":request_id,
                "source_request_id":plan["source_request_id"],
                "device_id":id,"phase":"sending","before_backup":true}));
            let result = self
                .rpc(json!({"op":"configure","session":session,
                "commands":args.commands,"save":args.save,"checks":args.checks}))
                .await;
            if result.is_ok() {
                self.emit(json!({"kind":"ai_config_phase","request_id":request_id,
                    "source_request_id":plan["source_request_id"],
                    "device_id":id,"phase":"finalizing","before_backup":true}));
            }
            match result {
                Ok(mut outcome) => {
                    let content = outcome
                        .as_object_mut()
                        .and_then(|o| o.remove("after_content"));
                    if outcome["status"] == "unknown" {
                        self.sessions.lock().await.remove(id);
                    }
                    for (key, value) in outcome.as_object().context("CONFIG_PROTOCOL")? {
                        record[key] = value.clone();
                    }
                    if record["status"] == "succeeded" {
                        let checks = record["checks"].as_array();
                        let verified = checks.is_some_and(|items| {
                            !items.is_empty()
                                && items.iter().all(|item| item["passed"] == true)
                                && items.iter().any(|item| item["query"] == "config")
                                && (!args.save
                                    || items.iter().any(|item| item["query"] == "startup"))
                        });
                        if !verified
                            || (args.save && record["save_status"] != "verified")
                            || record["confirmed_commands"].as_u64()
                                != Some(args.commands.len() as u64)
                            || !record["error"].is_null()
                        {
                            record["status"] = json!("applied_unverified");
                            record["error"] =
                                json!("CONFIG_PROTOCOL: 执行程序未提供完整回读或保存证据");
                        }
                    }
                    if let Some(Value::String(content)) = content {
                        let _backup = self.backup_lock.lock().await;
                        match backups::save(
                            &self.root.join("backups"),
                            &self.store,
                            self.vault.as_ref(),
                            id,
                            content,
                        ) {
                            Ok(backup) => record["after_backup"] = backup,
                            Err(error) => {
                                record["backup_error"] = json!(redact(&error.to_string()));
                                if record["status"] == "succeeded" {
                                    record["status"] = json!("applied_backup_failed");
                                    record["error"] =
                                        json!("POST_BACKUP_FAILED: 配置已核验，但执行后备份失败");
                                }
                            }
                        }
                    } else if record["status"] == "succeeded" {
                        record["status"] = json!("applied_backup_failed");
                        record["error"] = json!("POST_BACKUP_FAILED: 缺少执行后配置备份内容");
                    }
                }
                Err(error) => {
                    record["status"] = json!("unknown");
                    record["error"] = json!(format!(
                        "CONFIG_UNKNOWN: {}；未自动重试",
                        redact(&error.to_string())
                    ));
                    self.sessions.lock().await.remove(id);
                }
            }
        }
        record["phase"] = json!("finished");
        record["finished_at"] = json!(chrono::Utc::now().to_rfc3339());
        self.store.update_config(&record, true)?;
        guard.finished = true;
        self.emit(json!({"kind":"ai_config_result","request_id":request_id,
            "source_request_id":plan["source_request_id"],"device_id":id,"result":record}));
        self.emit(json!({"kind":"task","id":request_id,"device_id":id,"operation":"configure","status":if record["status"]=="succeeded" {"succeeded"} else {"failed"}}));
        Ok(record)
    }
}

fn redact_request(value: &Value) -> String {
    fn visit(value: &Value) -> Value {
        match value {
            Value::String(s) => json!(redact(s)),
            Value::Array(a) => Value::Array(a.iter().map(visit).collect()),
            Value::Object(o) => {
                Value::Object(o.iter().map(|(k, v)| (k.clone(), visit(v))).collect())
            }
            other => other.clone(),
        }
    }
    visit(value).to_string()
}

#[cfg(test)]
mod autonomy_scope_tests {
    use super::*;
    #[test]
    fn control_commands_and_logical_interfaces_fail_closed() {
        for command in [
            "conf t",
            "SY",
            "q",
            "save force",
            "reload in 1",
            "reboot",
            "description ok; reboot",
        ] {
            let args = ConfigArgs {
                commands: vec![command.into()],
                save: false,
                checks: vec![Verification {
                    query: "config".into(),
                    contains: vec!["value".into()],
                    not_contains: vec![],
                }],
            };
            assert!(args.validate().is_err(), "{command}");
        }
        let args = ConfigArgs {
            commands: vec!["interface Loopback123".into(), "description VALUE".into()],
            save: false,
            checks: vec![Verification {
                query: "config".into(),
                contains: vec!["description VALUE".into()],
                not_contains: vec![],
            }],
        };
        assert!(validate_autonomous_scope(&args).is_err());
    }

    #[test]
    fn autonomous_vlan_must_have_a_scoped_name_to_certify() {
        let mut args = ConfigArgs {
            commands: vec!["vlan 120".into()],
            save: false,
            checks: vec![Verification {
                query: "config".into(),
                contains: vec!["vlan 120".into()],
                not_contains: vec![],
            }],
        };
        assert!(validate_autonomous_scope(&args).is_err());
        args.commands
            .extend(["interface GE0/0/1".into(), "description UPLINK".into()]);
        args.checks[0].contains.push("description UPLINK".into());
        assert!(validate_autonomous_scope(&args).is_err());
        args.commands = vec!["vlan 120".into(), "name LAB".into()];
        args.checks[0].contains = vec!["vlan 120".into(), "name LAB".into()];
        assert!(validate_autonomous_scope(&args).is_ok());
    }

    #[test]
    fn vlan_and_interface_descriptions_may_run_without_per_plan_approval() {
        let mut args = ConfigArgs {
            commands: vec!["vlan 120".into(), "name LAB_DEV".into()],
            save: false,
            checks: vec![Verification {
                query: "config".into(),
                contains: vec!["vlan 120".into(), "name LAB_DEV".into()],
                not_contains: vec![],
            }],
        };
        assert!(validate_autonomous_scope(&args).is_ok());
        args.checks[0].contains.remove(0);
        assert!(
            validate_autonomous_scope(&args)
                .unwrap_err()
                .to_string()
                .contains("AUTONOMY_SCOPE_DENIED")
        );
        args.commands = vec!["interface GE0/0/1".into(), "description UPLINK-LAB".into()];
        args.checks[0].contains = vec!["description UPLINK-LAB".into()];
        assert!(validate_autonomous_scope(&args).is_ok());
        args.commands.push("shutdown".into());
        assert!(validate_autonomous_scope(&args).is_err());
    }
}
