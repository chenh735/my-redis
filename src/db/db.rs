use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Instant, interval, Duration};

#[derive(Clone, Debug)]
struct Entry {
    value: String,
    expires_at: Option<Instant>,
}

#[derive(Clone)]
pub struct Db {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl Entry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

impl Db {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_key(&self, key: &str) -> Option<String> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                None
            }
            Some(entry) => Some(entry.value.clone()),
            None => None,
        }
    }

    pub async fn set_key(&self, key: &str, value: String, expires_at: Option<Instant>) {
        let value = Entry { value, expires_at };
        self.inner.write().await.insert(key.to_string(), value);
    }

    pub async fn del_key(&self, keys: Vec<&str>) -> u32 {
        let mut count = 0u32;
        let mut write = self.inner.write().await;

        for key in keys {
            if let Some(entry) = write.remove(key) {
                if !entry.is_expired() {
                    count += 1;
                }
            }
        }

        count
    }

    pub async fn exists(&self, keys: Vec<&str>) -> u32 {
        let mut count = 0u32;
        let mut write = self.inner.write().await;

        for key in keys {
            if let Some(entry) = write.get(key) {
                if entry.is_expired() {
                    write.remove(key);
                } else {
                    count += 1;
                }
            }
        }

        count
    }

    async fn clean_up_keys(&self) {
        let mut db = self.inner.write().await;
        db.retain(|_key, entry| {
           !entry.is_expired()
        });
    }

    pub fn start_clean_up_keys(&self) {
        let mut tick = interval(Duration::from_secs(1));
        let db = self.clone();

        tokio::spawn({
            async move {
                loop {
                    tick.tick().await;
                    db.clean_up_keys().await;
                }
            }
        });
    }
}



#[cfg(test)]
mod tests {
    use super::Db;
    use tokio::time::{Duration, Instant, sleep};

    #[tokio::test]
    async fn get_key_returns_value_before_expiration() {
        let db = Db::new();
        db.set_key(
            "name",
            "redis".to_string(),
            Some(Instant::now() + Duration::from_secs(10)),
        )
        .await;

        assert_eq!(db.get_key("name").await, Some("redis".to_string()));
    }

    #[tokio::test]
    async fn get_key_removes_expired_key() {
        let db = Db::new();
        db.set_key(
            "name",
            "redis".to_string(),
            Some(Instant::now() + Duration::from_millis(10)),
        )
        .await;

        sleep(Duration::from_millis(20)).await;

        assert_eq!(db.get_key("name").await, None);
        assert_eq!(db.exists(vec!["name"]).await, 0);
    }

    #[tokio::test]
    async fn exists_counts_multiple_live_keys() {
        let db = Db::new();
        db.set_key("a", "1".to_string(), None).await;
        db.set_key("b", "2".to_string(), None).await;

        assert_eq!(db.exists(vec!["a", "b", "c"]).await, 2);
    }

    #[tokio::test]
    async fn del_key_removes_only_existing_live_keys() {
        let db = Db::new();
        db.set_key("a", "1".to_string(), None).await;
        db.set_key("b", "2".to_string(), None).await;

        assert_eq!(db.del_key(vec!["a", "missing"]).await, 1);
        assert_eq!(db.exists(vec!["a", "b"]).await, 1);
    }
}
