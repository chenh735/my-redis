use crate::db::{Db, Entry};
use crate::persist::Aof;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};

pub const RDB_PATH: &str = "dump.rdb";
pub const AOF_PATH: &str = "appendonly.aof";
pub const AOF_INCR_PATH: &str = "appendonly.aof.incr";

pub struct Rdb;

impl Rdb {
    pub async fn save(path: &str, db: Db) -> Result<()> {
        let snapshot = db.snapshot().await;
        Self::save_snapshot(path, snapshot).await
    }

    pub(crate) async fn save_snapshot(path: &str, snapshot: HashMap<String, Entry>) -> Result<()> {
        let content = serde_json::to_vec_pretty(&snapshot).context("encode RDB snapshot failed")?;
        let path = Path::new(path);
        let tmp_path = tmp_path(path);

        fs::write(&tmp_path, content)
            .await
            .with_context(|| format!("write RDB temp file failed: {}", tmp_path.display()))?;

        let bak_path = bak_path(path);
        remove_if_exists(&bak_path).await?;

        let had_old_rdb = match fs::rename(path, &bak_path).await {
            Ok(()) => true,
            Err(err) if err.kind() == ErrorKind::NotFound => false,
            Err(err) => {
                remove_if_exists(&tmp_path).await?;
                return Err(err)
                    .with_context(|| format!("backup old RDB file failed: {}", path.display()));
            }
        };

        if let Err(err) = fs::rename(&tmp_path, path).await {
            if had_old_rdb {
                let _ = fs::rename(&bak_path, path).await;
            }
            return Err(err)
                .with_context(|| format!("replace RDB file failed: {}", path.display()));
        }

        if had_old_rdb {
            remove_if_exists(&bak_path).await?;
        }

        Ok(())
    }

    pub async fn load(path: &str, db: Db) -> Result<()> {
        let content = match fs::read(path).await {
            Ok(content) => content,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).with_context(|| format!("read RDB file failed: {path}")),
        };

        let snapshot: HashMap<String, Entry> =
            serde_json::from_slice(&content).context("decode RDB snapshot failed")?;
        db.replace_snapshot(snapshot).await;
        Ok(())
    }

    pub async fn load_hybrid(
        rdb_path: &str,
        base_aof_path: &str,
        incr_aof_path: &str,
        db: Db,
    ) -> Result<String> {
        Self::load(rdb_path, db.clone()).await?;

        let has_base_aof = exists(base_aof_path).await?;
        let has_incr_aof = exists(incr_aof_path).await?;
        let active_aof_path = if has_incr_aof && !has_base_aof {
            incr_aof_path
        } else {
            base_aof_path
        };

        Aof::load(active_aof_path, db).await?;
        Ok(active_aof_path.to_string())
    }
}

pub async fn save_hybrid_snapshot(
    rdb_path: &str,
    base_aof_path: &str,
    incr_aof_path: &str,
    db: Db,
    aof: Arc<Mutex<Aof>>,
) -> Result<()> {
    let mut aof = aof.lock().await;
    let snapshot = db.snapshot().await;

    aof.switch_to(incr_aof_path, true).await?;

    if let Err(err) = Rdb::save_snapshot(rdb_path, snapshot).await {
        let _ = aof.switch_to(base_aof_path, false).await;
        let _ = remove_if_exists(Path::new(incr_aof_path)).await;
        return Err(err);
    }

    remove_if_exists(Path::new(base_aof_path)).await?;
    aof.rename_current_to(base_aof_path).await?;
    Ok(())
}

pub fn tick_hybrid_snapshot(sec: u64, db: Db, aof: Arc<Mutex<Aof>>) {
    let mut interval = interval(Duration::from_secs(sec));

    tokio::spawn(async move {
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(e) =
                save_hybrid_snapshot(RDB_PATH, AOF_PATH, AOF_INCR_PATH, db.clone(), aof.clone())
                    .await
            {
                eprintln!("hybrid snapshot error: {e}");
            }
        }
    });
}

async fn exists(path: &str) -> Result<bool> {
    match fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("read file metadata failed: {path}")),
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp_path = path.to_path_buf();
    tmp_path.set_extension("rdb.tmp");
    tmp_path
}

fn bak_path(path: &Path) -> PathBuf {
    let mut bak_path = path.to_path_buf();
    bak_path.set_extension("rdb.bak");
    bak_path
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("remove file failed: {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::{Rdb, save_hybrid_snapshot};
    use crate::db::Db;
    use crate::persist::Aof;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;
    use tokio::sync::Mutex;

    fn test_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("my-redis-{name}-{millis}.rdb"))
    }

    #[tokio::test]
    async fn save_and_load_restores_string_and_collections() {
        let path = test_path("snapshot");
        let source = Db::new();
        source.set_key("name", "redis".to_string(), None).await;
        source
            .push_list("letters", &["a".into(), "b".into()], false)
            .await
            .unwrap();
        source
            .set_add("tags", &["rust".into(), "db".into()])
            .await
            .unwrap();
        source
            .hash_set(
                "user",
                &[("name".into(), "chen".into()), ("age".into(), "18".into())],
            )
            .await
            .unwrap();

        Rdb::save(path.to_str().unwrap(), source).await.unwrap();

        let restored = Db::new();
        Rdb::load(path.to_str().unwrap(), restored.clone())
            .await
            .unwrap();

        assert_eq!(restored.get_key("name").await, Some("redis".to_string()));
        assert_eq!(
            restored.list_range("letters", 0, -1).await.unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            restored.set_members("tags").await.unwrap(),
            vec!["db", "rust"]
        );
        assert_eq!(
            restored.hash_get("user", "name").await.unwrap(),
            Some("chen".to_string())
        );

        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn save_replaces_existing_snapshot() {
        let path = test_path("replace");

        let old = Db::new();
        old.set_key("name", "old".to_string(), None).await;
        Rdb::save(path.to_str().unwrap(), old).await.unwrap();

        let new = Db::new();
        new.set_key("name", "new".to_string(), None).await;
        Rdb::save(path.to_str().unwrap(), new).await.unwrap();

        let restored = Db::new();
        Rdb::load(path.to_str().unwrap(), restored.clone())
            .await
            .unwrap();

        assert_eq!(restored.get_key("name").await, Some("new".to_string()));

        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn hybrid_snapshot_recovers_rdb_plus_incremental_aof() {
        let rdb_path = test_path("hybrid");
        let base_aof_path = rdb_path.with_extension("base.aof");
        let incr_aof_path = rdb_path.with_extension("incr.aof");

        let source = Db::new();
        source
            .push_list("letters", &["a".into()], false)
            .await
            .unwrap();

        let aof = Arc::new(Mutex::new(
            Aof::open(base_aof_path.to_str().unwrap()).await.unwrap(),
        ));

        save_hybrid_snapshot(
            rdb_path.to_str().unwrap(),
            base_aof_path.to_str().unwrap(),
            incr_aof_path.to_str().unwrap(),
            source.clone(),
            aof.clone(),
        )
        .await
        .unwrap();

        assert_eq!(aof.lock().await.path(), base_aof_path.as_path());
        assert!(fs::metadata(&base_aof_path).await.is_ok());
        assert!(fs::metadata(&incr_aof_path).await.is_err());

        source
            .push_list("letters", &["b".into()], false)
            .await
            .unwrap();
        aof.lock()
            .await
            .append(&["RPUSH".into(), "letters".into(), "b".into()])
            .await
            .unwrap();

        let restored = Db::new();
        Rdb::load_hybrid(
            rdb_path.to_str().unwrap(),
            base_aof_path.to_str().unwrap(),
            incr_aof_path.to_str().unwrap(),
            restored.clone(),
        )
        .await
        .unwrap();

        assert_eq!(
            restored.list_range("letters", 0, -1).await.unwrap(),
            vec!["a", "b"]
        );

        drop(aof);
        let _ = fs::remove_file(rdb_path).await;
        let _ = fs::remove_file(base_aof_path).await;
        let _ = fs::remove_file(incr_aof_path).await;
    }

    #[tokio::test]
    async fn hybrid_snapshot_rewrites_aof_to_keep_only_post_snapshot_commands() {
        let rdb_path = test_path("hybrid-rewrite");
        let base_aof_path = rdb_path.with_extension("base.aof");
        let incr_aof_path = rdb_path.with_extension("incr.aof");

        let source = Db::new();
        source.set_key("before", "rdb".to_string(), None).await;

        let aof = Arc::new(Mutex::new(
            Aof::open(base_aof_path.to_str().unwrap()).await.unwrap(),
        ));
        aof.lock()
            .await
            .append(&["SET".into(), "before".into(), "rdb".into()])
            .await
            .unwrap();

        save_hybrid_snapshot(
            rdb_path.to_str().unwrap(),
            base_aof_path.to_str().unwrap(),
            incr_aof_path.to_str().unwrap(),
            source.clone(),
            aof.clone(),
        )
        .await
        .unwrap();

        source.set_key("after", "aof".to_string(), None).await;
        aof.lock()
            .await
            .append(&["SET".into(), "after".into(), "aof".into()])
            .await
            .unwrap();
        aof.lock().await.flush().await.unwrap();

        let aof_content = fs::read_to_string(&base_aof_path).await.unwrap();
        assert!(!aof_content.contains("before"));
        assert!(aof_content.contains("after"));
        assert!(fs::metadata(&incr_aof_path).await.is_err());

        let restored = Db::new();
        let active_aof_path = Rdb::load_hybrid(
            rdb_path.to_str().unwrap(),
            base_aof_path.to_str().unwrap(),
            incr_aof_path.to_str().unwrap(),
            restored.clone(),
        )
        .await
        .unwrap();

        assert_eq!(active_aof_path, base_aof_path.to_str().unwrap());
        assert_eq!(restored.get_key("before").await, Some("rdb".to_string()));
        assert_eq!(restored.get_key("after").await, Some("aof".to_string()));

        drop(aof);
        let _ = fs::remove_file(rdb_path).await;
        let _ = fs::remove_file(base_aof_path).await;
        let _ = fs::remove_file(incr_aof_path).await;
    }
}
