use crate::cmd::cmd::dispatch;
use crate::db::Db;
use crate::persist::parse::parse_array;
use crate::resp::resp::encode_request;
use anyhow::{Context, Result, bail};
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};

pub struct Aof {
    file: File,
}

impl Aof {
    pub async fn open(path: &str) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .with_context(|| format!("open AOF file failed: {path}"))?;

        Ok(Self { file })
    }

    pub async fn append(&mut self, args: &[String]) -> Result<()> {
        let content = encode_request(args.to_vec())?.context("encode empty AOF request")?;
        self.file.write_all(content.as_bytes()).await?;
        Ok(())
    }

    pub async fn load(path: &str, db: Db) -> Result<()> {
        let mut file = match File::open(path).await {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err).with_context(|| format!("open AOF file failed: {path}")),
        };

        let mut content = Vec::new();
        file.read_to_end(&mut content).await?;

        for args in parse_array(&content)? {
            if args.is_empty() {
                bail!("AOF request must not be empty");
            }
            if !is_write_command(&args) {
                bail!("AOF only supports write commands: {}", args[0]);
            }

            let response = dispatch(db.clone(), args).await;
            if response.starts_with('-') {
                bail!("replay AOF command failed: {}", response.trim_end());
            }
        }

        Ok(())
    }

    pub async fn flush(&mut self) -> Result<()> {
        self.file.flush().await?;
        Ok(())
    }
}

pub fn is_write_command(args: &[String]) -> bool {
    args.first()
        .is_some_and(|cmd| matches!(cmd.to_ascii_lowercase().as_str(), "set" | "del"))
}

pub fn tick_flush(sec: u64, aof: Arc<Mutex<Aof>>) {
    let mut interval = interval(Duration::from_secs(sec));
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            if let Err(e) = aof.lock().await.flush().await {
                eprintln!("AOF flush error: {e}");
            };
        }
    });
}
#[cfg(test)]
mod tests {
    use super::{Aof, is_write_command};
    use crate::db::Db;
    use crate::resp::resp::encode_request;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::fs;

    fn test_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("my-redis-{name}-{millis}.aof"))
    }

    #[test]
    fn is_write_command_accepts_set_and_del_only() {
        assert!(is_write_command(&["SET".to_string(), "name".to_string()]));
        assert!(is_write_command(&["del".to_string(), "name".to_string()]));
        assert!(!is_write_command(&["GET".to_string(), "name".to_string()]));
        assert!(!is_write_command(&[]));
    }

    #[tokio::test]
    async fn append_writes_resp_array() {
        let path = test_path("append");
        let mut aof = Aof::open(path.to_str().unwrap()).await.unwrap();

        aof.append(&["SET".to_string(), "name".to_string(), "redis".to_string()])
            .await
            .unwrap();
        drop(aof);

        let content = fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n");

        let _ = fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn load_replays_resp_commands() {
        let path = test_path("load");
        let mut content = String::new();
        content.push_str(
            &encode_request(vec!["SET".into(), "name".into(), "redis".into()])
                .unwrap()
                .unwrap(),
        );
        content.push_str(
            &encode_request(vec!["SET".into(), "age".into(), "18".into()])
                .unwrap()
                .unwrap(),
        );
        content.push_str(
            &encode_request(vec!["DEL".into(), "age".into()])
                .unwrap()
                .unwrap(),
        );
        fs::write(&path, content).await.unwrap();

        let db = Db::new();
        Aof::load(path.to_str().unwrap(), db.clone()).await.unwrap();

        assert_eq!(db.get_key("name").await, Some("redis".to_string()));
        assert_eq!(db.get_key("age").await, None);

        let _ = fs::remove_file(path).await;
    }
}
