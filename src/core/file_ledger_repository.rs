use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

pub struct LedgerFileSave {
    pub file_path: String,
    pub fingerprint: String,
}

pub struct LedgerFile {
    pub file_path: String,
    pub fingerprint: String,
    pub lock_count: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct FileLedgerRepository {
    db_file_path: String,
}

impl FileLedgerRepository {
    pub fn new(db_file_path: String) -> Self {
        Self { db_file_path }
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;

        conn.execute(
            "
            CREATE SEQUENCE IF NOT EXISTS file_ledger_serial START WITH 1;
            CREATE TABLE IF NOT EXISTS file_ledger (
                id UINT64 PRIMARY KEY,
                file_path TEXT NOT NULL,
                fingerprint TEXT NOT NULL,
                lock_count UINT64 NOT NULL,
                updated_at TIMESTAMP_MS NOT NULL,
                UNIQUE(file_path)
            );
        ",
            params![],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    pub fn lock_file(&self, file_path: &str) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;
        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp = timestamp.format(TIMESTAMP_FORMAT).to_string();

        conn.execute(
            "
            UPDATE file_ledger
                SET updated_at = ?,
                    lock_count = lock_count + 1
            WHERE file_path = ?
            ",
            params![&timestamp, &file_path],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    pub fn unlock_file(&self, file_path: &str) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;

        // remove on the count
        conn.execute(
            "
            UPDATE file_ledger 
                SET lock_count = lock_count - 1
            WHERE file_path = ? AND lock_count > 0
            ",
            params![&file_path],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;
        Ok(())
    }

    pub fn is_file_locked(&self, file_path: &str) -> Result<bool> {
        // TODO: should count instead
        let conn = Connection::open(&self.db_file_path)?;

        let mut stmt =
            conn.prepare("SELECT id FROM file_ledger WHERE file_path=? AND lock_count > 0")?;
        let found_iter = stmt.query_map([file_path], |_row| Ok(true))?;
        let count = found_iter.count();

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(count > 0)
    }

    pub fn save_file(&self, file: LedgerFileSave) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;

        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp = timestamp.format(TIMESTAMP_FORMAT).to_string();

        conn.execute(
            "
            INSERT INTO file_ledger (id, lock_count, file_path, fingerprint, updated_at) 
                VALUES (nextval('file_ledger_serial'), 0, ?1, ?2, ?3) 
            ON CONFLICT (file_path) DO UPDATE 
                SET fingerprint = EXCLUDED.fingerprint,
                    updated_at = EXCLUDED.updated_at
            ",
            params![&file.file_path, &file.fingerprint, &timestamp],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    pub fn get_file(&self, file_path: &str) -> Result<Option<LedgerFile>> {
        let conn = Connection::open(&self.db_file_path)?;

        let mut stmt = conn.prepare(
            "SELECT fingerprint, lock_count, updated_at FROM file_ledger WHERE file_path=?",
        )?;
        let mut rows = stmt.query([file_path])?;
        if let Some(row) = rows.next()? {
            let timestamp: DateTime<Utc> = row.get(2)?;

            return Ok(Some(LedgerFile {
                file_path: file_path.to_owned(),
                fingerprint: row.get(0)?,
                lock_count: row.get(1)?,
                updated_at: timestamp,
            }));
        }

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(None)
    }

    pub fn is_file_sync(&self, file: LedgerFile) -> Result<bool> {
        let timestamp = file.updated_at.format(TIMESTAMP_FORMAT).to_string();

        // TODO: should count instead
        let conn = Connection::open(&self.db_file_path)?;
        let mut stmt = conn.prepare(
            "SELECT id FROM file_ledger WHERE file_path=? AND fingerprint=? AND updated_at>=?",
        )?;
        let found_iter = stmt
            .query_map([&file.file_path, &file.fingerprint, &timestamp], |_row| {
                Ok(true)
            })?;
        let count = found_iter.count();
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};
    use super::*;

    #[test]
    fn test_new() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_new_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        assert_eq!(repo.db_file_path, tmp_db_path_str);

        Ok(())
    }

    #[test]
    fn test_migrate() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_migrate_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        // check if the table exists
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SHOW TABLES")?;
        let mut rows = stmt.query([])?;

        let mut found_file_ledger = false;

        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            if name == "file_ledger" {
                found_file_ledger = true;
                continue;
            }
        }
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        assert!(found_file_ledger);

        Ok(())
    }

    #[test]
    fn test_lock_file() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_lock_file_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let file_path = format!("file_{rnd}");
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: "1".to_owned(),
        })
        .unwrap();

        // save with no lock in
        repo.lock_file(&file_path).unwrap();

        // check if row exists as expected
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(file.lock_count, 1);

        // save with update
        repo.lock_file(&file_path).unwrap();

        // check if row exists as expected after update
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(file.lock_count, 2);

        Ok(())
    }

    #[test]
    fn test_unlock_file() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_unlock_file_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let file_path = format!("file_{rnd}");
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: "1".to_owned(),
        })
        .unwrap();
        repo.lock_file(&file_path).unwrap();
        repo.lock_file(&file_path).unwrap();

        // proceed with the unlock
        repo.unlock_file(&file_path).unwrap();

        // is it as expected?
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(file.lock_count, 1);

        // proceed with the unlock
        repo.unlock_file(&file_path).unwrap();

        // is it as expected?
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(file.lock_count, 0);

        // it shouldn't over unlock
        repo.unlock_file(&file_path).unwrap();
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(file.lock_count, 0);

        Ok(())
    }

    #[test]
    fn test_is_file_locked() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path =
            std::env::temp_dir().join(format!("fsy_repo_test_is_file_locked_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let file_path = format!("file_{rnd}");

        // test without a file on the db
        let is = repo.is_file_locked(&file_path).unwrap();
        assert!(!is);

        // create a file on the database
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: "1".to_owned(),
        })
        .unwrap();

        // shouldn't be locked with count 0
        let is = repo.is_file_locked(&file_path).unwrap();
        assert!(!is);

        // should be locked when the count is over 0
        repo.lock_file(&file_path).unwrap();
        let is = repo.is_file_locked(&file_path).unwrap();
        assert!(is);

        Ok(())
    }

    #[test]
    fn test_save() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_save_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let file_path = format!("file_{rnd}");
        let fingerprint = "1".to_owned();

        // save with no row in
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: fingerprint.clone(),
        })
        .unwrap();

        // check if row exists as expected
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(&file.file_path, &file_path);
        assert_eq!(&file.fingerprint, &fingerprint);
        assert_eq!(file.lock_count, 0);

        // save with update
        let fingerprint = "2".to_owned();
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: fingerprint.clone(),
        })
        .unwrap();

        // check if row exists as expected
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(&file.file_path, &file_path);
        assert_eq!(&file.fingerprint, &fingerprint);
        assert_eq!(file.lock_count, 0);

        Ok(())
    }

    #[test]
    fn test_get_file() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path =
            std::env::temp_dir().join(format!("fsy_repo_test_get_pull_file_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        // save row
        let file_path = format!("file_{rnd}");
        let fingerprint = "1".to_owned();
        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp = timestamp.format(TIMESTAMP_FORMAT).to_string();
        let conn = Connection::open(&tmp_db_path_str)?;
        conn.execute(
            "
            INSERT INTO file_ledger (id, lock_count, file_path, fingerprint, updated_at) 
                VALUES (nextval('file_ledger_serial'), 0, ?1, ?2, ?3) 
            ",
            params![&file_path, &fingerprint, &timestamp],
        )?;

        // check get file
        let file = repo.get_file(&file_path).unwrap().unwrap();
        assert_eq!(&file.file_path, &file_path);
        assert_eq!(&file.fingerprint, &fingerprint);
        assert_eq!(file.lock_count, 0);

        Ok(())
    }

    #[test]
    fn test_is_file_sync() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path =
            std::env::temp_dir().join(format!("fsy_repo_test_is_pull_file_upd_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        // save file to check
        let file_path = format!("file_{rnd}");
        let fingerprint = "1".to_owned();
        let timestamp: DateTime<Utc> = Utc::now();

        // check before any file is in
        let is = repo
            .is_file_sync(LedgerFile {
                file_path: file_path.clone(),
                fingerprint: fingerprint.clone(),
                lock_count: 0,
                updated_at: timestamp,
            })
            .unwrap();
        assert!(!is);

        // check same
        repo.save_file(LedgerFileSave {
            file_path: file_path.clone(),
            fingerprint: fingerprint.clone(),
        })
        .unwrap();
        let is = repo
            .is_file_sync(LedgerFile {
                file_path: file_path.clone(),
                fingerprint: fingerprint.clone(),
                lock_count: 0,
                updated_at: timestamp,
            })
            .unwrap();
        assert!(is);

        // check different fingerprint
        let is = repo
            .is_file_sync(LedgerFile {
                file_path: file_path.clone(),
                fingerprint: "2".to_owned(),
                lock_count: 0,
                updated_at: timestamp,
            })
            .unwrap();
        assert!(!is);

        Ok(())
    }
}
