use crate::cmd::cmd::{dispatch, validate_command};
use crate::db::Db;
use crate::persist::{Aof, is_bgsave_command, is_write_command, save_hybrid_snapshot};
use crate::resp::resp::{error, raw_array, simple, syntax_error};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default, Debug)]
pub struct Transaction {
    queued: Option<Vec<Vec<String>>>,
    dirty: bool,
    dirty_cmd: Vec<String>,
}

#[derive(Clone)]
pub struct TransactionPersistence {
    rdb_path: String,
    aof_path: String,
    aof_incr_path: String,
}

impl TransactionPersistence {
    pub fn new(rdb_path: String, aof_path: String, aof_incr_path: String) -> Self {
        Self {
            rdb_path,
            aof_path,
            aof_incr_path,
        }
    }
}

impl Transaction {
    pub async fn handle(
        &mut self,
        db: Db,
        aof: Arc<Mutex<Aof>>,
        persistence: TransactionPersistence,
        args: Vec<String>,
    ) -> Option<String> {
        let command = args.first()?.to_ascii_lowercase();

        match command.as_str() {
            "multi" => Some(self.begin(args)),
            "discard" => Some(self.discard(args)),
            "exec" => Some(self.exec(args, db, aof, persistence).await),
            _ if self.is_active() => Some(self.queue(args)),
            _ => None,
        }
    }

    fn begin(&mut self, args: Vec<String>) -> String {
        if args.len() != 1 {
            return syntax_error();
        }
        if self.is_active() {
            return error("MULTI calls can not be nested".to_string());
        }

        self.queued = Some(Vec::new());
        simple("OK".to_string())
    }

    fn discard(&mut self, args: Vec<String>) -> String {
        if args.len() != 1 {
            return syntax_error();
        }
        if !self.is_active() {
            return error("DISCARD without MULTI".to_string());
        }

        self.clear();
        simple("OK".to_string())
    }

    fn queue(&mut self, args: Vec<String>) -> String {
        if self.dirty {
            return error("Command parsing error".to_string());
        }

        if let Err(err) = validate_command(&args) {
            self.dirty = true;
            self.dirty_cmd = args;
            return error(err);
        }

        self.queued
            .as_mut()
            .expect("transaction is active")
            .push(args);
        simple("QUEUED".to_string())
    }

    async fn exec(
        &mut self,
        args: Vec<String>,
        db: Db,
        aof: Arc<Mutex<Aof>>,
        persistence: TransactionPersistence,
    ) -> String {
        if args.len() != 1 {
            return syntax_error();
        }

        if self.dirty {
            let dirty_cmd = std::mem::take(&mut self.dirty_cmd);
            self.clear();
            return error(format!(
                "EXECABORT Transaction discarded because of previous errors: {:?}",
                dirty_cmd
            ));
        }

        let Some(commands) = self.queued.take() else {
            return error("EXEC without MULTI".to_string());
        };

        execute_commands(commands, db, aof, persistence).await
    }

    fn is_active(&self) -> bool {
        self.queued.is_some()
    }

    fn clear(&mut self) {
        self.queued = None;
        self.dirty = false;
        self.dirty_cmd.clear();
    }
}

async fn execute_commands(
    commands: Vec<Vec<String>>,
    db: Db,
    aof: Arc<Mutex<Aof>>,
    persistence: TransactionPersistence,
) -> String {
    let mut responses = Vec::with_capacity(commands.len());
    let mut bgsave_count = 0;

    {
        let mut aof = aof.lock().await;

        for command in commands {
            let mut response = dispatch(db.clone(), command.clone()).await;

            if !response.starts_with('-') {
                if is_bgsave_command(&command) {
                    bgsave_count += 1;
                } else if is_write_command(&command) {
                    if let Err(e) = aof.append(&command).await {
                        eprintln!("AOF append error:{e}");
                        response = error("persistence error".to_string());
                    }
                }
            }

            responses.push(response);
        }
    }

    for _ in 0..bgsave_count {
        let save_db = db.clone();
        let save_aof = aof.clone();
        let save_persistence = persistence.clone();
        tokio::spawn(async move {
            if let Err(e) = save_hybrid_snapshot(
                &save_persistence.rdb_path,
                &save_persistence.aof_path,
                &save_persistence.aof_incr_path,
                save_db,
                save_aof,
            )
            .await
            {
                eprintln!("hybrid snapshot error:{e}");
            }
        });
    }

    raw_array(responses)
}

#[cfg(test)]
mod tests {
    use super::{Transaction, TransactionPersistence};
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
        std::env::temp_dir().join(format!("my-redis-{name}-{millis}.aof"))
    }

    fn persistence(path: &std::path::Path) -> TransactionPersistence {
        TransactionPersistence::new(
            path.with_extension("rdb").to_str().unwrap().to_string(),
            path.to_str().unwrap().to_string(),
            path.with_extension("incr.aof")
                .to_str()
                .unwrap()
                .to_string(),
        )
    }

    #[tokio::test]
    async fn exec_runs_queued_commands_and_appends_writes_only() {
        let path = test_path("transaction-exec");
        let persistence = persistence(&path);
        let db = Db::new();
        let aof = Arc::new(Mutex::new(Aof::open(path.to_str().unwrap()).await.unwrap()));
        let mut tx = Transaction::default();

        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["MULTI".into()]
            )
            .await
            .unwrap(),
            "+OK\r\n"
        );
        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["SET".into(), "name".into(), "redis".into()]
            )
            .await
            .unwrap(),
            "+QUEUED\r\n"
        );
        assert_eq!(db.get_key("name").await, None);
        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["GET".into(), "name".into()]
            )
            .await
            .unwrap(),
            "+QUEUED\r\n"
        );

        let response = tx
            .handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["EXEC".into()],
            )
            .await
            .unwrap();

        assert_eq!(response, "*2\r\n+OK\r\n$5\r\nredis\r\n");
        assert_eq!(db.get_key("name").await, Some("redis".to_string()));

        aof.lock().await.flush().await.unwrap();
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("SET"));
        assert!(!content.contains("GET"));

        drop(aof);
        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn discard_clears_queued_commands() {
        let path = test_path("transaction-discard");
        let persistence = persistence(&path);
        let db = Db::new();
        let aof = Arc::new(Mutex::new(Aof::open(path.to_str().unwrap()).await.unwrap()));
        let mut tx = Transaction::default();

        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["MULTI".into()],
        )
        .await
        .unwrap();
        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["SET".into(), "name".into(), "redis".into()],
        )
        .await
        .unwrap();

        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["DISCARD".into()]
            )
            .await
            .unwrap(),
            "+OK\r\n"
        );
        assert_eq!(db.get_key("name").await, None);
        assert!(
            tx.handle(db, aof.clone(), persistence.clone(), vec!["EXEC".into()])
                .await
                .unwrap()
                .starts_with("-ERR EXEC without MULTI")
        );

        drop(aof);
        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn exec_triggers_queued_bgsave() {
        let path = test_path("transaction-bgsave");
        let rdb_path = path.with_extension("rdb");
        let incr_aof_path = path.with_extension("incr.aof");
        let persistence = persistence(&path);
        let db = Db::new();
        let aof = Arc::new(Mutex::new(Aof::open(path.to_str().unwrap()).await.unwrap()));
        let mut tx = Transaction::default();

        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["MULTI".into()],
        )
        .await
        .unwrap();
        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["BGSAVE".into()]
            )
            .await
            .unwrap(),
            "+QUEUED\r\n"
        );

        let response = tx
            .handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["EXEC".into()],
            )
            .await
            .unwrap();

        assert_eq!(response, "*1\r\n+OK\r\n");

        for _ in 0..20 {
            if fs::metadata(&rdb_path).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        assert!(fs::metadata(&rdb_path).await.is_ok());

        drop(aof);
        let _ = fs::remove_file(path).await;
        let _ = fs::remove_file(rdb_path).await;
        let _ = fs::remove_file(incr_aof_path).await;
    }

    #[tokio::test]
    async fn exec_aborts_and_clears_dirty_transaction() {
        let path = test_path("transaction-dirty");
        let persistence = persistence(&path);
        let db = Db::new();
        let aof = Arc::new(Mutex::new(Aof::open(path.to_str().unwrap()).await.unwrap()));
        let mut tx = Transaction::default();

        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["MULTI".into()],
        )
        .await
        .unwrap();
        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["SET".into(), "name".into(), "redis".into()],
        )
        .await
        .unwrap();

        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["GET".into()]
            )
            .await
            .unwrap(),
            "-ERR syntax error\r\n"
        );

        assert!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["EXEC".into()]
            )
            .await
            .unwrap()
            .starts_with("-ERR EXECABORT")
        );
        assert_eq!(db.get_key("name").await, None);

        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["MULTI".into()]
            )
            .await
            .unwrap(),
            "+OK\r\n"
        );

        drop(aof);
        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn unknown_command_marks_transaction_dirty() {
        let path = test_path("transaction-unknown-command");
        let persistence = persistence(&path);
        let db = Db::new();
        let aof = Arc::new(Mutex::new(Aof::open(path.to_str().unwrap()).await.unwrap()));
        let mut tx = Transaction::default();

        tx.handle(
            db.clone(),
            aof.clone(),
            persistence.clone(),
            vec!["MULTI".into()],
        )
        .await
        .unwrap();
        assert_eq!(
            tx.handle(
                db.clone(),
                aof.clone(),
                persistence.clone(),
                vec!["UNKNOWN".into(), "name".into()]
            )
            .await
            .unwrap(),
            "-ERR unknown command\r\n"
        );

        assert!(
            tx.handle(db, aof.clone(), persistence.clone(), vec!["EXEC".into()])
                .await
                .unwrap()
                .starts_with("-ERR EXECABORT")
        );

        drop(aof);
        let _ = fs::remove_file(path).await;
    }
}
