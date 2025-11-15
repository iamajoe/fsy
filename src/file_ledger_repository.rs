use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use duckdb::{Connection, params};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

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
            CREATE SEQUENCE IF NOT EXISTS file_pulls_serial START WITH 1;
            CREATE TABLE IF NOT EXISTS file_pulls (
                id INTEGER PRIMARY KEY,
                target_id TEXT, 
                file_path TEXT, 
                timestamp TIMESTAMP_MS,
                UNIQUE(target_id, file_path)
            );

            CREATE SEQUENCE IF NOT EXISTS file_push_locks_serial START WITH 1;
            CREATE TABLE IF NOT EXISTS file_push_locks (
                id INTEGER PRIMARY KEY,
                file_path TEXT,
                updated_at TIMESTAMP_MS,
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
            INSERT INTO file_push_locks (id, file_path, updated_at) 
            VALUES (nextval('file_push_locks_serial'), ?1, ?2) 
            ON CONFLICT (file_path) DO UPDATE 
            SET updated_at = EXCLUDED.updated_at
            ",
            params![&file_path, &timestamp],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    pub fn unlock_file(&self, file_path: &str) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;
        conn.execute(
            "DELETE FROM file_push_locks WHERE file_path = ?",
            params![&file_path],
        )?;
        conn.close().map_err(|(_, err)| anyhow!(err))?;
        Ok(())
    }

    pub fn is_file_locked(&self, file_path: &str) -> Result<bool> {
        // TODO: should count instead
        let conn = Connection::open(&self.db_file_path)?;

        let mut stmt = conn.prepare("SELECT id FROM file_push_locks WHERE file_path=?")?;
        let found_iter = stmt.query_map([file_path], |_row| Ok(true))?;
        let count = found_iter.count();

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(count > 0)
    }

    pub fn save_pull_file(
        &self,
        target_id: &str,
        file_path: &str,
        timestamp: &DateTime<Utc>,
    ) -> Result<()> {
        let conn = Connection::open(&self.db_file_path)?;
        let timestamp = timestamp.format(TIMESTAMP_FORMAT).to_string();

        conn.execute(
            "
            INSERT INTO file_pulls (id, target_id, file_path, timestamp) 
            VALUES (nextval('file_pulls_serial'), ?1, ?2, ?3) 
            ON CONFLICT (target_id, file_path) DO UPDATE 
            SET timestamp = EXCLUDED.timestamp
            ",
            params![&target_id, &file_path, &timestamp],
        )?;

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    pub fn get_pull_file(
        &self,
        target_id: &str,
        file_path: &str,
    ) -> Result<Option<(String, String, DateTime<Utc>)>> {
        let conn = Connection::open(&self.db_file_path)?;

        let mut stmt =
            conn.prepare("SELECT timestamp FROM file_pulls WHERE target_id=? AND file_path=?")?;
        let mut rows = stmt.query([target_id, file_path])?;
        if let Some(row) = rows.next()? {
            let timestamp: DateTime<Utc> = row.get(0)?;
            return Ok(Some((
                target_id.to_owned(),
                file_path.to_owned(),
                timestamp,
            )));
        }

        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(None)
    }

    pub fn is_pull_file_updated(
        &self,
        target_id: &str,
        file_path: &str,
        update_timestamp: &DateTime<Utc>,
    ) -> Result<bool> {
        let timestamp = update_timestamp.format(TIMESTAMP_FORMAT).to_string();

        // TODO: should count instead
        let conn = Connection::open(&self.db_file_path)?;
        let mut stmt = conn.prepare(
            "SELECT id FROM file_pulls WHERE file_path=? AND target_id=? AND timestamp>=?",
        )?;
        let found_iter = stmt.query_map([file_path, target_id, &timestamp], |_row| Ok(true))?;
        let count = found_iter.count();
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{Duration, NaiveDateTime};

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

        let mut found_file_pulls = false;
        let mut found_file_push_locks = false;

        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            if name == "file_pulls" {
                found_file_pulls = true;
                continue;
            }

            if name == "file_push_locks" {
                found_file_push_locks = true;
                continue;
            }
        }
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        assert!(found_file_pulls);
        assert!(found_file_push_locks);

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

        // save with no row in
        repo.lock_file(&file_path).unwrap();

        // check if row exists as expected
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SELECT file_path FROM file_push_locks")?;
        let mut rows = stmt.query([])?;
        let mut rows_count = 0;
        while let Some(row) = rows.next()? {
            let row_file_path: String = row.get(0)?;
            assert_eq!(row_file_path, file_path);
            rows_count += 1;
        }
        assert_eq!(rows_count, 1);
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        // save with update
        repo.lock_file(&file_path).unwrap();

        // check if row exists as expected after update
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SELECT file_path FROM file_push_locks")?;
        let mut rows = stmt.query([])?;
        let mut rows_count = 0;
        while let Some(row) = rows.next()? {
            let row_file_path: String = row.get(0)?;
            assert_eq!(row_file_path, file_path);
            // TODO: should test the timestamp as well for the updated at
            rows_count += 1;
        }
        assert_eq!(rows_count, 1);
        conn.close().map_err(|(_, err)| anyhow!(err))?;

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

        // save row
        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
        let conn = Connection::open(&tmp_db_path_str)?;
        conn.execute(
            "
            INSERT INTO file_push_locks (id, file_path, updated_at) 
            VALUES (nextval('file_push_locks_serial'), ?1, ?2) 
                ",
            params![&file_path, &timestamp_format],
        )?;
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        // proceed with the unlock
        repo.unlock_file(&file_path).unwrap();

        // check if row exists as expected
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SELECT file_path FROM file_push_locks")?;
        let mut rows = stmt.query([])?;
        let mut rows_count = 0;
        while let Some(row) = rows.next()? {
            let row_file_path: String = row.get(0)?;
            assert_eq!(row_file_path, file_path);
            rows_count += 1;
        }
        assert_eq!(rows_count, 0);
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    #[test]
    fn test_is_file_locked() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_is_file_locked_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let file_path = format!("file_{rnd}");

        // test without a file on the db
        let is = repo.is_file_locked(&file_path).unwrap();
        assert!(!is);

        // save row
        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
        let conn = Connection::open(&tmp_db_path_str)?;
        conn.execute(
            "
            INSERT INTO file_push_locks (id, file_path, updated_at) 
            VALUES (nextval('file_push_locks_serial'), ?1, ?2) 
                ",
            params![&file_path, &timestamp_format],
        )?;
        conn.close().map_err(|(_, err)| anyhow!(err))?;

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

        let target_id = format!("target_{rnd}");
        let file_path = format!("file_{rnd}");

        // save with no row in
        let timestamp: DateTime<Utc> = Utc::now();
        repo.save_pull_file(&target_id, &file_path, &timestamp)
            .unwrap();

        // check if row exists as expected
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SELECT target_id, file_path, timestamp FROM file_pulls")?;
        let mut rows = stmt.query([])?;
        let mut rows_count = 0;
        while let Some(row) = rows.next()? {
            let row_target_id: String = row.get(0)?;
            let row_file_path: String = row.get(1)?;
            let row_timestamp_raw: NaiveDateTime = row.get(2)?;
            let row_timestamp: String = row_timestamp_raw.format(TIMESTAMP_FORMAT).to_string();

            assert_eq!(row_target_id, target_id);
            assert_eq!(row_file_path, file_path);

            let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
            assert_eq!(row_timestamp, timestamp_format);

            rows_count += 1;
        }
        assert_eq!(rows_count, 1);
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        // save with update
        let timestamp: DateTime<Utc> = Utc::now();
        repo.save_pull_file(&target_id, &file_path, &timestamp)
            .unwrap();

        // check if row exists as expected after update
        let conn = Connection::open(&tmp_db_path_str)?;
        let mut stmt = conn.prepare("SELECT target_id, file_path, timestamp FROM file_pulls")?;
        let mut rows = stmt.query([])?;
        let mut rows_count = 0;
        while let Some(row) = rows.next()? {
            let row_target_id: String = row.get(0)?;
            let row_file_path: String = row.get(1)?;
            let row_timestamp_raw: NaiveDateTime = row.get(2)?;
            let row_timestamp: String = row_timestamp_raw.format(TIMESTAMP_FORMAT).to_string();

            assert_eq!(row_target_id, target_id);
            assert_eq!(row_file_path, file_path);

            let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
            assert_eq!(row_timestamp, timestamp_format);

            rows_count += 1;
        }
        assert_eq!(rows_count, 1);
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        Ok(())
    }

    #[test]
    fn test_get_pull_file() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_get_pull_file_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let target_id = format!("target_{rnd}");
        let file_path = format!("file_{rnd}");

        // save row
        let timestamp: DateTime<Utc> = Utc::now();
        let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
        let conn = Connection::open(&tmp_db_path_str)?;
        conn.execute(
            "
                INSERT INTO file_pulls (id, target_id, file_path, timestamp) 
                VALUES (nextval('file_pulls_serial'), ?1, ?2, ?3) 
                ON CONFLICT (target_id, file_path) DO UPDATE 
                SET timestamp = EXCLUDED.timestamp
                ",
            params![&target_id, &file_path, &timestamp_format],
        )?;
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        // check timestamp
        let (_, _, file_timestamp) = repo.get_pull_file(&target_id, &file_path).unwrap().unwrap();

        assert_eq!(
            file_timestamp.timestamp_millis(),
            timestamp.timestamp_millis()
        );

        Ok(())
    }

    #[test]
    fn test_is_pull_file_updated() -> Result<()> {
        // prepare test
        let rnd = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let tmp_db_path = std::env::temp_dir().join(format!("fsy_repo_test_is_pull_file_upd_{rnd}.db"));
        let tmp_db_path_str = tmp_db_path.to_str().unwrap().to_owned();

        let repo = FileLedgerRepository::new(tmp_db_path_str.clone());
        repo.migrate().unwrap();

        let target_id = format!("target_{rnd}");
        let file_path = format!("file_{rnd}");
        let timestamp: DateTime<Utc> = Utc::now();

        // check before any pull file is in
        let is = repo
            .is_pull_file_updated(&target_id, &file_path, &timestamp)
            .unwrap();
        assert!(!is);

        // save row
        let timestamp_format = timestamp.format(TIMESTAMP_FORMAT).to_string();
        let conn = Connection::open(&tmp_db_path_str)?;
        conn.execute(
            "
                INSERT INTO file_pulls (id, target_id, file_path, timestamp) 
                VALUES (nextval('file_pulls_serial'), ?1, ?2, ?3) 
                ON CONFLICT (target_id, file_path) DO UPDATE 
                SET timestamp = EXCLUDED.timestamp
                ",
            params![&target_id, &file_path, &timestamp_format],
        )?;
        conn.close().map_err(|(_, err)| anyhow!(err))?;

        // check timestamp before
        let timestamp_mod = timestamp - Duration::seconds(1);
        let is = repo
            .is_pull_file_updated(&target_id, &file_path, &timestamp_mod)
            .unwrap();
        assert!(is);

        // check timestamp after
        let timestamp_mod = timestamp + Duration::seconds(1);
        let is = repo
            .is_pull_file_updated(&target_id, &file_path, &timestamp_mod)
            .unwrap();
        assert!(!is);

        Ok(())
    }
}
