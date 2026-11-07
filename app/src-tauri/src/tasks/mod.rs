use anyhow::{Result, bail};
use chrono::Utc;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{path::Path, sync::Mutex};
use uuid::Uuid;

pub struct Store(pub Mutex<Connection>, pub crate::security::FileLease);
/// Mark cancelled asynchronous work as failed even when its future is dropped.
pub struct TaskGuard<'a> {
    store: &'a Store,
    id: String,
    finished: bool,
}
impl<'a> TaskGuard<'a> {
    pub fn new(store: &'a Store, id: &str) -> Self {
        Self {
            store,
            id: id.into(),
            finished: false,
        }
    }
    pub fn finish(mut self, error: Option<&str>) -> Result<()> {
        self.store.finish(&self.id, error)?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for TaskGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .store
                .finish(&self.id, Some("TASK_CANCELLED: 任务被取消或已超过总时限"));
        }
    }
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        // Acquire ownership before opening SQLite or recovering orphaned tasks.
        let lease = crate::security::lock_file(&path.with_extension("instance.lock"), false)?;
        let db = Connection::open(path)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS devices(id TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS tasks(id TEXT PRIMARY KEY, device_id TEXT, operation TEXT NOT NULL, status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT, error TEXT);
            CREATE TABLE IF NOT EXISTS backups(id TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES devices(id), body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS config_runs(id TEXT PRIMARY KEY, device_id TEXT NOT NULL, request_hash TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS config_plans(id TEXT PRIMARY KEY, device_id TEXT NOT NULL, status TEXT NOT NULL, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS autonomy_requests(id TEXT PRIMARY KEY, device_id TEXT NOT NULL, prompt_hash TEXT NOT NULL, created_at TEXT NOT NULL);
            UPDATE config_runs SET body=json_set(body,'$.status',CASE WHEN json_extract(body,'$.write_attempted') THEN 'unknown' ELSE 'not_started' END,'$.error','CONFIG_INTERRUPTED: 应用退出，未自动重放配置') WHERE json_extract(body,'$.status')='running';
            UPDATE tasks SET status='interrupted', finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now'), error='应用退出导致任务中断' WHERE status='running';")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self(Mutex::new(db), lease))
    }
    pub fn save_device(&self, value: &Value) -> Result<()> {
        self.0.lock().unwrap().execute("INSERT INTO devices(id,body) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body", params![value["id"].as_str(), value.to_string()])?;
        Ok(())
    }
    pub fn devices(&self) -> Result<Vec<Value>> {
        let db = self.0.lock().unwrap();
        let mut stmt = db.prepare("SELECT body FROM devices ORDER BY rowid")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|v| Ok(serde_json::from_str(&v?)?)).collect()
    }
    pub fn device(&self, id: &str) -> Result<Value> {
        self.devices()?
            .into_iter()
            .find(|x| x["id"] == id)
            .ok_or_else(|| anyhow::anyhow!("DEVICE_NOT_FOUND: 设备不存在"))
    }
    pub fn setting(&self, key: &str) -> Result<Option<Value>> {
        use rusqlite::OptionalExtension;
        let value: Option<String> = self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT body FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub fn save_setting(&self, key: &str, v: &Value) -> Result<()> {
        self.0.lock().unwrap().execute("INSERT INTO settings(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body", params![key, v.to_string()])?;
        Ok(())
    }
    pub fn start(&self, device: Option<&str>, operation: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        self.0.lock().unwrap().execute("INSERT INTO tasks(id,device_id,operation,status,started_at) VALUES(?1,?2,?3,'running',?4)", params![id, device, operation, Utc::now().to_rfc3339()])?;
        Ok(id)
    }
    pub fn finish(&self, id: &str, error: Option<&str>) -> Result<()> {
        let count = self.0.lock().unwrap().execute(
            "UPDATE tasks SET status=?1,finished_at=?2,error=?3 WHERE id=?4 AND status='running'",
            params![
                if error.is_some() {
                    "failed"
                } else {
                    "succeeded"
                },
                Utc::now().to_rfc3339(),
                error,
                id
            ],
        )?;
        if count != 1 {
            bail!("TASK_STATE: 无效任务状态迁移");
        }
        Ok(())
    }
    pub fn tasks(&self, id: Option<&str>) -> Result<Vec<Value>> {
        let db = self.0.lock().unwrap();
        let mut stmt = db.prepare("SELECT t.id,t.device_id,operation,status,started_at,finished_at,error,c.body FROM tasks t LEFT JOIN config_runs c ON c.id=t.id WHERE (?1 IS NULL OR t.id=?1) ORDER BY t.rowid DESC LIMIT 100")?;
        Ok(stmt.query_map([id], |r| Ok(json!({"id":r.get::<_,String>(0)?,"device_id":r.get::<_,Option<String>>(1)?,"operation":r.get::<_,String>(2)?,"status":r.get::<_,String>(3)?,"started_at":r.get::<_,String>(4)?,"finished_at":r.get::<_,Option<String>>(5)?,"error":r.get::<_,Option<String>>(6)?,"result":r.get::<_,Option<String>>(7)?.and_then(|v|serde_json::from_str::<Value>(&v).ok())})))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
    /// A reused autonomous request id must never generate a second executable plan,
    /// including after a crash, error, or an already-completed device write.
    pub fn reserve_autonomy_request(
        &self,
        id: &str,
        device_id: &str,
        prompt_hash: &str,
    ) -> Result<()> {
        let db = self.0.lock().unwrap();
        if db.execute("INSERT OR IGNORE INTO autonomy_requests(id,device_id,prompt_hash,created_at) VALUES(?1,?2,?3,?4)",
            params![id,device_id,prompt_hash,Utc::now().to_rfc3339()])? !=1 {
            bail!("AUTONOMY_REQUEST_REPLAY: 本次自主执行请求编号已使用，不自动重放");
        }
        Ok(())
    }
    pub fn save_config_plan(&self, plan: &Value) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO config_plans(id,device_id,status,body) VALUES(?1,?2,'pending',?3)",
            params![
                plan["id"].as_str(),
                plan["device_id"].as_str(),
                plan.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn config_plan(&self, id: &str) -> Result<Option<Value>> {
        use rusqlite::OptionalExtension;
        let found: Option<(String, String)> = self
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT body,status FROM config_plans WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        found
            .map(|(body, status)| {
                let mut plan: Value = serde_json::from_str(&body)?;
                plan["status"] = json!(status);
                Ok(plan)
            })
            .transpose()
    }
    /// Reserve execution and consume the reviewed plan in one transaction.
    /// A crash cannot turn a consumed plan into another executable request.
    pub fn reserve_approved_config(&self, plan_id: &str, record: &Value) -> Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        if tx.execute(
            "UPDATE config_plans SET status='consumed' WHERE id=?1 AND device_id=?2 AND status='pending'",
            params![plan_id,record["device_id"].as_str()],
        )? != 1 { bail!("PLAN_ALREADY_USED: 此计划已执行或无法再审批"); }
        tx.execute(
            "INSERT INTO config_runs(id,device_id,request_hash,body) VALUES(?1,?2,?3,?4)",
            params![
                record["request_id"].as_str(),
                record["device_id"].as_str(),
                record["request_hash"].as_str(),
                record.to_string()
            ],
        )?;
        tx.execute(
            "INSERT INTO tasks(id,device_id,operation,status,started_at) VALUES(?1,?2,'configure','running',?3)",
            params![record["request_id"].as_str(),record["device_id"].as_str(),Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn config_run(&self, id: &str) -> Result<Option<Value>> {
        use rusqlite::OptionalExtension;
        let body: Option<String> = self
            .0
            .lock()
            .unwrap()
            .query_row("SELECT body FROM config_runs WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        body.map(|body| serde_json::from_str(&body).map_err(Into::into))
            .transpose()
    }
    pub fn reserve_config(&self, record: &Value) -> Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        let id = record["request_id"].as_str();
        let device = record["device_id"].as_str();
        tx.execute(
            "INSERT INTO config_runs(id,device_id,request_hash,body) VALUES(?1,?2,?3,?4)",
            params![
                id,
                device,
                record["request_hash"].as_str(),
                record.to_string()
            ],
        )?;
        tx.execute("INSERT INTO tasks(id,device_id,operation,status,started_at) VALUES(?1,?2,'configure','running',?3)",params![id,device,Utc::now().to_rfc3339()])?;
        tx.commit()?;
        Ok(())
    }
    pub fn update_config(&self, record: &Value, finished: bool) -> Result<()> {
        let mut db = self.0.lock().unwrap();
        let tx = db.transaction()?;
        let id = record["request_id"].as_str();
        if tx.execute(
            "UPDATE config_runs SET body=?1 WHERE id=?2",
            params![record.to_string(), id],
        )? != 1
        {
            bail!("CONFIG_RECORD_MISSING");
        }
        if finished {
            tx.execute("UPDATE tasks SET status=?1,finished_at=?2,error=?3 WHERE id=?4 AND status='running'",params![if record["status"]=="succeeded" {"succeeded"}else{"failed"},Utc::now().to_rfc3339(),record["error"].as_str(),id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn save_backup(&self, record: &Value) -> Result<()> {
        self.0.lock().unwrap().execute(
            "INSERT INTO backups(id,device_id,body) VALUES(?1,?2,?3)",
            params![
                record["id"].as_str(),
                record["device_id"].as_str(),
                record.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn backups(&self, device: &str) -> Result<Vec<Value>> {
        let db = self.0.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT body FROM backups WHERE device_id=?1 ORDER BY json_extract(body,'$.created_at') DESC")?;
        stmt.query_map([device], |r| r.get::<_, String>(0))?
            .map(|x| Ok(serde_json::from_str(&x?)?))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn second_instance_cannot_interrupt_live_work_and_lease_releases_on_exit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let first = Store::open(&path).unwrap();
        let task = first.start(None, "backup").unwrap();
        assert!(
            Store::open(&path)
                .err()
                .unwrap()
                .to_string()
                .contains("INSTANCE_RUNNING")
        );
        assert_eq!(first.tasks(Some(&task)).unwrap()[0]["status"], "running");
        first.finish(&task, None).unwrap();
        let id = Uuid::new_v4().to_string();
        first.reserve_config(&json!({"request_id":id,"device_id":"dev","request_hash":"hash","status":"running","write_attempted":true})).unwrap();
        drop(first);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(
            reopened.tasks(Some(&task)).unwrap()[0]["status"],
            "succeeded"
        );
        assert_eq!(
            reopened.config_run(&id).unwrap().unwrap()["status"],
            "unknown"
        );
    }
    #[test]
    fn cancelled_task_does_not_remain_running() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db")).unwrap();
        let id = store.start(None, "query").unwrap();
        {
            let _guard = TaskGuard::new(&store, &id);
        }
        assert_eq!(store.tasks(Some(&id)).unwrap()[0]["status"], "failed");
    }
    #[test]
    fn unfinished_tasks_become_interrupted_and_finish_is_single_use() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let store = Store::open(&path).unwrap();
        let id = store.start(None, "backup").unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.tasks(Some(&id)).unwrap()[0]["status"], "interrupted");
        assert!(store.finish(&id, None).is_err());
        let next = store.start(None, "query").unwrap();
        store.finish(&next, Some("QUERY_FAILED")).unwrap();
        assert_eq!(store.tasks(Some(&next)).unwrap()[0]["status"], "failed");
    }
}
