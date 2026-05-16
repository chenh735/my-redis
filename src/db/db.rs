use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::interval;

#[derive(Clone, Debug)]
pub enum Value {
    String(String),
    List(VecDeque<String>),
    Set(HashSet<String>),
    Hash(HashMap<String, String>),
}

#[derive(Clone, Debug)]
struct Entry {
    value: Value,
    expires_at: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DbError {
    WrongType,
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Clone)]
pub struct Db {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl Entry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| SystemTime::now() >= expires_at)
    }
}

impl Db {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_string(&self, key: &str) -> DbResult<Option<String>> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(None)
            }
            Some(entry) => match &entry.value {
                Value::String(value) => Ok(Some(value.clone())),
                _ => Err(DbError::WrongType),
            },
            None => Ok(None),
        }
    }

    pub async fn get_key(&self, key: &str) -> Option<String> {
        self.get_string(key).await.ok().flatten()
    }

    pub async fn set_key(&self, key: &str, value: String, expires_at: Option<SystemTime>) {
        let value = Entry {
            value: Value::String(value),
            expires_at,
        };
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

    pub async fn get_str_len(&self, key: &str) -> DbResult<usize>{
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get(key) {
            Some(entry) => {
                match &entry.value {
                    Value::String(value) => {
                        Ok(value.len())
                    },
                    _ => Err(DbError::WrongType)
                }
            },
            None => Ok(0)
        }
    }

    pub async fn string_append_str(&self, key: &str, value: &str) -> DbResult<usize>{
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get_mut(key) {
            Some(entry) =>{
                match  &mut entry.value{
                    Value::String(v) =>{
                        v.push_str(value);
                        Ok(v.len())
                    },
                    _ => Err(DbError::WrongType)
                }
            }
            None => {
                write.insert(key.to_string(), Entry{
                    value: Value::String(value.to_string()),
                    expires_at: None
                });
                Ok(value.len())
            }
        }

    }

    pub async fn push_list(&self, key: &str, values: &[String], left: bool) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::List(list) => {
                    push_values(list, values, left);
                    Ok(list.len())
                }
                _ => Err(DbError::WrongType),
            },
            None => {
                let mut list = VecDeque::new();
                push_values(&mut list, values, left);
                write.insert(
                    key.to_string(),
                    Entry {
                        value: Value::List(list),
                        expires_at: None,
                    },
                );
                Ok(values.len())
            }
        }
    }

    pub async fn pop_list(&self, key: &str, left: bool) -> DbResult<Option<String>> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
            return Ok(None);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::List(list) => {
                    let value = if left {
                        list.pop_front()
                    } else {
                        list.pop_back()
                    };
                    if list.is_empty() {
                        write.remove(key);
                    }
                    Ok(value)
                }
                _ => Err(DbError::WrongType),
            },
            None => Ok(None),
        }
    }

    pub async fn list_len(&self, key: &str) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(0)
            }
            Some(entry) => match &entry.value {
                Value::List(list) => Ok(list.len()),
                _ => Err(DbError::WrongType),
            },
            None => Ok(0),
        }
    }

    pub async fn list_range(&self, key: &str, start: i64, stop: i64) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(Vec::new())
            }
            Some(entry) => match &entry.value {
                Value::List(list) => Ok(list_range(list, start, stop)),
                _ => Err(DbError::WrongType),
            },
            None => Ok(Vec::new()),
        }
    }

    pub async fn set_add(&self, key: &str, members: &[String]) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::Set(set) => Ok(members
                    .iter()
                    .filter(|member| set.insert((*member).clone()))
                    .count()),
                _ => Err(DbError::WrongType),
            },
            None => {
                let set: HashSet<_> = members.iter().cloned().collect();
                let count = set.len();
                write.insert(
                    key.to_string(),
                    Entry {
                        value: Value::Set(set),
                        expires_at: None,
                    },
                );
                Ok(count)
            }
        }
    }

    pub async fn set_remove(&self, key: &str, members: &[String]) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
            return Ok(0);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::Set(set) => Ok(members.iter().filter(|member| set.remove(*member)).count()),
                _ => Err(DbError::WrongType),
            },
            None => Ok(0),
        }
    }

    pub async fn set_is_member(&self, key: &str, member: &str) -> DbResult<bool> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(false)
            }
            Some(entry) => match &entry.value {
                Value::Set(set) => Ok(set.contains(member)),
                _ => Err(DbError::WrongType),
            },
            None => Ok(false),
        }
    }

    pub async fn set_card(&self, key: &str) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(0)
            }
            Some(entry) => match &entry.value {
                Value::Set(set) => Ok(set.len()),
                _ => Err(DbError::WrongType),
            },
            None => Ok(0),
        }
    }

    pub async fn set_members(&self, key: &str) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(Vec::new())
            }
            Some(entry) => match &entry.value {
                Value::Set(set) => {
                    let mut members: Vec<_> = set.iter().cloned().collect();
                    members.sort();
                    Ok(members)
                }
                _ => Err(DbError::WrongType),
            },
            None => Ok(Vec::new()),
        }
    }

    pub async fn hash_set(&self, key: &str, pairs: &[(String, String)]) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::Hash(hash) => Ok(pairs
                    .iter()
                    .filter(|(field, value)| hash.insert(field.clone(), value.clone()).is_none())
                    .count()),
                _ => Err(DbError::WrongType),
            },
            None => {
                let hash: HashMap<_, _> = pairs.iter().cloned().collect();
                let count = hash.len();
                write.insert(
                    key.to_string(),
                    Entry {
                        value: Value::Hash(hash),
                        expires_at: None,
                    },
                );
                Ok(count)
            }
        }
    }

    pub async fn set_get_inter(&self, keys: &[String]) -> DbResult<Vec<String>> {
        let read = self.inner.read().await;
        for key in keys{
            match read.get(key) {
                Some(entry) =>{
                    if entry.is_expired() || !matches!(entry.value, Value::Set(_)){
                        return Ok(Vec::new())
                    }
                },
                None => return Ok(Vec::new())
            }
        };

        let ans = read.get(&keys[0]).unwrap().value.clone();
        match ans {
            Value::Set(mut values) =>{
                for key in &keys[1..]{
                    match &read.get(key).unwrap().value {
                        Value::Set(cur_hash) => {
                            values = values.intersection(cur_hash).cloned().collect();
                        },
                        _ => return Err(DbError::WrongType)
                    }
                };
                Ok(values.into_iter().collect())
            },
            _ => Err(DbError::WrongType)
        }
    }

    pub async fn set_get_union(&self, keys: &[String]) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        for key in keys {
            if write.get(key).is_some_and(Entry::is_expired) {
                write.remove(key);
            }
        }

        let mut values = HashSet::new();
        for key in keys {
            match write.get(key) {
                Some(entry) => match &entry.value {
                    Value::Set(set) => values.extend(set.iter().cloned()),
                    _ => return Err(DbError::WrongType),
                },
                None => {}
            }
        }

        let mut values: Vec<_> = values.into_iter().collect();
        values.sort();
        Ok(values)
    }

    pub async fn set_get_diff(&self, keys: &[String]) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        for key in keys {
            if write.get(key).is_some_and(Entry::is_expired) {
                write.remove(key);
            }
        }

        let mut values = match write.get(&keys[0]) {
            Some(entry) => match &entry.value {
                Value::Set(set) => set.clone(),
                _ => return Err(DbError::WrongType),
            },
            None => return Ok(Vec::new()),
        };

        for key in &keys[1..] {
            match write.get(key) {
                Some(entry) => match &entry.value {
                    Value::Set(set) => values.retain(|value| !set.contains(value)),
                    _ => return Err(DbError::WrongType),
                },
                None => {}
            }
        }

        let mut values: Vec<_> = values.into_iter().collect();
        values.sort();
        Ok(values)
    }

    pub async fn hash_get_len(&self, key: &str) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
        }

        match write.get(key) {
            Some(entry) => {
                match &entry.value {
                    Value::Hash(hash) =>{
                        Ok(hash.len())
                    },
                    _ => {
                        Err(DbError::WrongType)
                    }
                }
            },
            None => Ok(0)
        }
    }

    pub async fn hash_get_keys(&self, key: &str) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired){
            write.remove(key);
        }

        drop(write);
        let read = self.inner.read().await;

        match read.get(key) {
            Some(entry) =>{
                match &entry.value {
                    Value::Hash(hash) =>{
                        Ok(hash.keys().cloned().collect())
                    },
                    _ => {
                        Err(DbError::WrongType)
                    }
                }
            },
            None => Ok(Vec::new())
        }

    }

    pub async fn hash_get_values(&self, key: &str) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired){
            write.remove(key);
        }

        drop(write);
        let read = self.inner.read().await;

        match read.get(key) {
            Some(entry) =>{
                match &entry.value {
                    Value::Hash(hash) =>{
                        Ok(hash.values().cloned().collect())
                    },
                    _ => Err(DbError::WrongType)
                }
            },
            None => Ok(Vec::new())
        }
    }

    pub async fn hash_get(&self, key: &str, field: &str) -> DbResult<Option<String>> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(None)
            }
            Some(entry) => match &entry.value {
                Value::Hash(hash) => Ok(hash.get(field).cloned()),
                _ => Err(DbError::WrongType),
            },
            None => Ok(None),
        }
    }

    pub async fn hash_del(&self, key: &str, fields: &[String]) -> DbResult<usize> {
        let mut write = self.inner.write().await;
        if write.get(key).is_some_and(Entry::is_expired) {
            write.remove(key);
            return Ok(0);
        }

        match write.get_mut(key) {
            Some(entry) => match &mut entry.value {
                Value::Hash(hash) => Ok(fields
                    .iter()
                    .filter(|field| hash.remove(*field).is_some())
                    .count()),
                _ => Err(DbError::WrongType),
            },
            None => Ok(0),
        }
    }

    pub async fn hash_exists(&self, key: &str, field: &str) -> DbResult<bool> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(false)
            }
            Some(entry) => match &entry.value {
                Value::Hash(hash) => Ok(hash.contains_key(field)),
                _ => Err(DbError::WrongType),
            },
            None => Ok(false),
        }
    }

    pub async fn hash_get_all(&self, key: &str) -> DbResult<Vec<String>> {
        let mut write = self.inner.write().await;
        match write.get(key) {
            Some(entry) if entry.is_expired() => {
                write.remove(key);
                Ok(Vec::new())
            }
            Some(entry) => match &entry.value {
                Value::Hash(hash) => {
                    let mut pairs: Vec<_> = hash.iter().collect();
                    pairs.sort_by(|(left, _), (right, _)| left.cmp(right));
                    Ok(pairs
                        .into_iter()
                        .flat_map(|(field, value)| [field.clone(), value.clone()])
                        .collect())
                }
                _ => Err(DbError::WrongType),
            },
            None => Ok(Vec::new()),
        }
    }

    async fn clean_up_keys(&self) {
        let mut db = self.inner.write().await;
        db.retain(|_key, entry| !entry.is_expired());
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

fn push_values(list: &mut VecDeque<String>, values: &[String], left: bool) {
    if left {
        for value in values {
            list.push_front(value.clone());
        }
    } else {
        for value in values {
            list.push_back(value.clone());
        }
    }
}

fn list_range(list: &VecDeque<String>, start: i64, stop: i64) -> Vec<String> {
    let len = list.len() as i64;
    if len == 0 {
        return Vec::new();
    }

    let start = if start < 0 { len + start } else { start };
    let stop = if stop < 0 { len + stop } else { stop };
    let start = start.max(0);
    let stop = stop.min(len - 1);

    if start > stop || start >= len {
        return Vec::new();
    }

    list.iter()
        .skip(start as usize)
        .take((stop - start + 1) as usize)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Db, DbError};
    use std::time::{Duration, SystemTime};
    use tokio::time::sleep;

    #[tokio::test]
    async fn get_key_returns_value_before_expiration() {
        let db = Db::new();
        db.set_key(
            "name",
            "redis".to_string(),
            Some(SystemTime::now() + Duration::from_secs(10)),
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
            Some(SystemTime::now() + Duration::from_millis(10)),
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

    #[tokio::test]
    async fn list_operations_preserve_redis_push_order() {
        let db = Db::new();

        assert_eq!(
            db.push_list("letters", &["a".into(), "b".into(), "c".into()], true)
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            db.list_range("letters", 0, -1).await.unwrap(),
            vec!["c", "b", "a"]
        );
        assert_eq!(
            db.pop_list("letters", true).await.unwrap(),
            Some("c".to_string())
        );
    }

    #[tokio::test]
    async fn set_operations_count_new_members_only() {
        let db = Db::new();

        assert_eq!(
            db.set_add("tags", &["rust".into(), "db".into(), "rust".into()])
                .await
                .unwrap(),
            2
        );
        assert!(db.set_is_member("tags", "rust").await.unwrap());
        assert_eq!(
            db.set_remove("tags", &["db".into(), "missing".into()])
                .await
                .unwrap(),
            1
        );
        assert_eq!(db.set_card("tags").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn set_union_and_diff_return_sorted_members() {
        let db = Db::new();

        db.set_add("a", &["one".into(), "two".into(), "three".into()])
            .await
            .unwrap();
        db.set_add("b", &["two".into(), "four".into()])
            .await
            .unwrap();
        db.set_add("c", &["three".into()]).await.unwrap();

        assert_eq!(
            db.set_get_union(&["a".into(), "b".into(), "missing".into()])
                .await
                .unwrap(),
            vec!["four", "one", "three", "two"]
        );
        assert_eq!(
            db.set_get_diff(&["a".into(), "b".into(), "c".into()])
                .await
                .unwrap(),
            vec!["one"]
        );
    }

    #[tokio::test]
    async fn hash_operations_count_new_fields_only() {
        let db = Db::new();

        assert_eq!(
            db.hash_set(
                "user",
                &[
                    ("name".into(), "chen".into()),
                    ("age".into(), "18".into()),
                    ("name".into(), "hao".into()),
                ],
            )
            .await
            .unwrap(),
            2
        );
        assert_eq!(
            db.hash_get("user", "name").await.unwrap(),
            Some("hao".to_string())
        );
        assert_eq!(db.hash_del("user", &["age".into()]).await.unwrap(), 1);
        assert!(!db.hash_exists("user", "age").await.unwrap());
    }

    #[tokio::test]
    async fn get_string_returns_wrong_type_for_non_string_key() {
        let db = Db::new();
        db.push_list("items", &["one".into()], false).await.unwrap();

        assert_eq!(db.get_string("items").await, Err(DbError::WrongType));
    }
}
