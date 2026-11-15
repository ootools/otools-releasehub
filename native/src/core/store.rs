use super::models::{now, DownloadRecord, Project, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Mutex};

pub struct Store {
    db: Mutex<Connection>,
}
fn storage_error(_: impl std::fmt::Display) -> String {
    "本地数据读写失败，请检查数据目录权限和空间".into()
}
impl Store {
    pub fn new(path: &Path) -> Result<Self, String> {
        let db = Connection::open(path).map_err(storage_error)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(storage_error)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS downloads (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS preferences (id TEXT PRIMARY KEY, json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS download_digests (id TEXT PRIMARY KEY, digest TEXT);
            PRAGMA user_version=1;",
        )
        .map_err(storage_error)?;
        let store = Self { db: Mutex::new(db) };
        for mut d in store.downloads()? {
            if ["queued", "downloading"].contains(&d.status.as_str()) {
                d.status = "failed".into();
                d.error = Some("上次下载被中断，请重试".into());
                d.updated_at = now();
                store.put_download(&d)?;
            } else if d.status == "installing" {
                // Installation began only from an already completed, verified local download.
                // Keep that artifact recoverable, but never infer that installation succeeded.
                d.status = "completed".into();
                d.error = Some("上次安装被中断，下载文件保留，请重新确认安装".into());
                d.updated_at = now();
                store.put_download(&d)?;
            }
        }
        for mut p in store.projects()? {
            if p.status == "checking" {
                p.status = "notChecked".into();
                p.error = None;
                store.put_project(&p)?;
            }
        }
        Ok(store)
    }
    fn list<T: DeserializeOwned>(&self, table: &str) -> Result<Vec<T>, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let mut stmt = db
            .prepare(&format!("SELECT json FROM {} ORDER BY rowid DESC", table))
            .map_err(storage_error)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(storage_error)?;
        rows.map(|r| serde_json::from_str(&r.map_err(storage_error)?).map_err(storage_error))
            .collect()
    }
    fn get<T: DeserializeOwned>(&self, table: &str, id: &str) -> Result<T, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json: Option<String> = db
            .query_row(
                &format!("SELECT json FROM {} WHERE id=?1", table),
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        serde_json::from_str(&json.ok_or_else(|| "记录不存在，可能已被删除".to_string())?)
            .map_err(storage_error)
    }
    fn put<T: Serialize>(&self, table: &str, id: &str, value: &T) -> Result<(), String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json = serde_json::to_string(value).map_err(storage_error)?;
        db.execute(&format!("INSERT INTO {} (id,json) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json", table), params![id, json]).map_err(storage_error)?;
        Ok(())
    }
    pub fn mutate<T: Serialize + DeserializeOwned>(
        &self,
        table: &str,
        id: &str,
        f: impl FnOnce(&mut T) -> Result<(), String>,
    ) -> Result<T, String> {
        if !["projects", "downloads"].contains(&table) {
            return Err("数据类型无效".into());
        }
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let json: Option<String> = tx
            .query_row(
                &format!("SELECT json FROM {} WHERE id=?1", table),
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let mut value: T =
            serde_json::from_str(&json.ok_or_else(|| "记录不存在，可能已被删除".to_string())?)
                .map_err(storage_error)?;
        f(&mut value)?;
        tx.execute(
            &format!("UPDATE {} SET json=?1 WHERE id=?2", table),
            params![serde_json::to_string(&value).map_err(storage_error)?, id],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(value)
    }
    pub fn projects(&self) -> Result<Vec<Project>, String> {
        self.list("projects")
    }
    pub fn project(&self, id: &str) -> Result<Project, String> {
        self.get("projects", id)
    }
    pub fn put_project(&self, p: &Project) -> Result<(), String> {
        self.put("projects", &p.id, p)
    }
    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        self.project(id)?;
        self.db
            .lock()
            .map_err(storage_error)?
            .execute("DELETE FROM projects WHERE id=?1", [id])
            .map_err(storage_error)?;
        Ok(())
    }
    pub fn downloads(&self) -> Result<Vec<DownloadRecord>, String> {
        self.list("downloads")
    }
    pub fn download(&self, id: &str) -> Result<DownloadRecord, String> {
        self.get("downloads", id)
    }
    pub fn put_download(&self, d: &DownloadRecord) -> Result<(), String> {
        self.put("downloads", &d.id, d)
    }
    pub fn digest(&self, id: &str) -> Result<Option<String>, String> {
        self.db
            .lock()
            .map_err(storage_error)?
            .query_row(
                "SELECT digest FROM download_digests WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)
            .map(Option::flatten)
    }
    pub fn set_digest(&self, id: &str, digest: Option<&str>) -> Result<(), String> {
        self.db.lock().map_err(storage_error)?.execute("INSERT INTO download_digests(id,digest) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET digest=excluded.digest", params![id, digest]).map_err(storage_error)?;
        Ok(())
    }
    pub fn settings(&self) -> Result<Settings, String> {
        let db = self.db.lock().map_err(storage_error)?;
        let json: Option<String> = db
            .query_row(
                "SELECT json FROM preferences WHERE id='settings'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let settings: Settings = match json {
            Some(j) => serde_json::from_str(&j).map_err(storage_error)?,
            None => Settings::default(),
        };
        settings.validate()?;
        Ok(settings)
    }
    pub fn save_settings(&self, s: &Settings) -> Result<(), String> {
        s.validate()?;
        self.put("preferences", "settings", s)
    }
    pub fn insert_project_unique(&self, p: &Project) -> Result<bool, String> {
        let mut db = self.db.lock().map_err(storage_error)?;
        let tx = db.transaction().map_err(storage_error)?;
        let exists: i64 = tx.query_row("SELECT count(*) FROM projects WHERE lower(json_extract(json,'$.owner'))=lower(?1) AND lower(json_extract(json,'$.repo'))=lower(?2)", params![p.owner, p.repo], |r| r.get(0)).map_err(storage_error)?;
        if exists > 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO projects(id,json) VALUES(?1,?2)",
            params![p.id, serde_json::to_string(p).map_err(storage_error)?],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new(&dir.path().join("db.sqlite")).unwrap();
        let mut settings = s.settings().unwrap();
        settings.auto_download = true;
        s.save_settings(&settings).unwrap();
        assert!(s.settings().unwrap().auto_download);
    }
    #[test]
    fn restart_keeps_interrupted_work_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let store = Store::new(&path).unwrap();
        for status in ["queued", "downloading", "installing", "completed"] {
            let time = now();
            store
                .put_download(&DownloadRecord {
                    id: status.into(),
                    project_id: "project".into(),
                    project_name: "app".into(),
                    asset_id: 1,
                    file_name: "app.zip".into(),
                    tag: "v1".into(),
                    url: "https://github.com/owner/app/releases/download/v1/app.zip".into(),
                    kind: "portable".into(),
                    total_bytes: 10,
                    received_bytes: 4,
                    status: status.into(),
                    path: None,
                    sha256: None,
                    verified: false,
                    error: None,
                    created_at: time.clone(),
                    updated_at: time,
                })
                .unwrap();
        }
        drop(store);
        let reopened = Store::new(&path).unwrap();
        for id in ["queued", "downloading"] {
            let record = reopened.download(id).unwrap();
            assert_eq!(record.status, "failed");
            assert!(record.error.unwrap().contains("中断"));
        }
        let interrupted = reopened.download("installing").unwrap();
        assert_eq!(interrupted.status, "completed");
        assert!(interrupted.error.unwrap().contains("重新确认安装"));
        assert_eq!(reopened.download("completed").unwrap().status, "completed");
    }
}
