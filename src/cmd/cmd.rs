use crate::db::{Db, DbError};
use crate::resp::resp::{array, bulk, error, integer, nil, simple, syntax_error};
use anyhow::{Result, bail};
use std::time::{Duration, SystemTime};

pub async fn dispatch(db: Db, args: Vec<String>) -> String {
    if args.is_empty() {
        return syntax_error();
    }

    match args[0].to_ascii_lowercase().as_str() {
        "ping" => ping(args),
        "echo" => echo(args),
        "set" => set(db, args).await,
        "get" => get(db, args).await,
        "del" => del(db, args).await,
        "exists" => exists(db, args).await,
        "lpush" => list_push(db, args, true).await,
        "rpush" => list_push(db, args, false).await,
        "lpop" => list_pop(db, args, true).await,
        "rpop" => list_pop(db, args, false).await,
        "llen" => list_len(db, args).await,
        "lrange" => list_range(db, args).await,
        "sadd" => set_add(db, args).await,
        "srem" => set_remove(db, args).await,
        "sismember" => set_is_member(db, args).await,
        "scard" => set_card(db, args).await,
        "smembers" => set_members(db, args).await,
        "hset" => hash_set(db, args).await,
        "hget" => hash_get(db, args).await,
        "hdel" => hash_del(db, args).await,
        "hexists" => hash_exists(db, args).await,
        "hgetall" => hash_get_all(db, args).await,
        _ => syntax_error(),
    }
}

fn ping(args: Vec<String>) -> String {
    match args.len() {
        1 => simple("PONG".to_string()),
        2 => bulk(args[1].to_string()),
        _ => syntax_error(),
    }
}

fn echo(args: Vec<String>) -> String {
    match args.len() {
        2 => bulk(args[1].to_string()),
        _ => syntax_error(),
    }
}

fn expires_at(unit: &str, number: &str) -> Result<SystemTime> {
    let number = match number.parse::<u64>() {
        Ok(number) => number,
        Err(_) => bail!("invalid expiration number"),
    };

    let duration = match unit.to_ascii_lowercase().as_str() {
        "ex" => Duration::from_secs(number),
        "px" => Duration::from_millis(number),
        _ => bail!("invalid expiration unit"),
    };

    SystemTime::now()
        .checked_add(duration)
        .ok_or_else(|| anyhow::anyhow!("expiration time overflow"))
}

async fn set(db: Db, args: Vec<String>) -> String {
    match args.len() {
        3 => {
            db.set_key(args[1].as_str(), args[2].clone(), None).await;
            simple("OK".to_string())
        }
        5 => match expires_at(args[3].as_str(), args[4].as_str()) {
            Ok(time) => {
                db.set_key(args[1].as_str(), args[2].clone(), Some(time))
                    .await;
                simple("OK".to_string())
            }
            Err(_) => syntax_error(),
        },
        _ => syntax_error(),
    }
}

async fn get(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.get_string(args[1].as_str()).await {
            Ok(Some(value)) => bulk(value),
            Ok(None) => nil(),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn exists(db: Db, args: Vec<String>) -> String {
    match args.len() {
        1 => syntax_error(),
        _ => {
            let count = db
                .exists(args[1..].iter().map(|x| x.as_str()).collect())
                .await;
            integer(count as i32)
        }
    }
}

async fn del(db: Db, args: Vec<String>) -> String {
    match args.len() {
        1 => syntax_error(),
        _ => {
            let count = db
                .del_key(args[1..].iter().map(|x| x.as_str()).collect())
                .await;
            integer(count as i32)
        }
    }
}

async fn list_push(db: Db, args: Vec<String>, left: bool) -> String {
    match args.len() {
        0..=2 => syntax_error(),
        _ => match db.push_list(&args[1], &args[2..], left).await {
            Ok(len) => integer(len as i32),
            Err(err) => db_error(err),
        },
    }
}

async fn list_pop(db: Db, args: Vec<String>, left: bool) -> String {
    match args.len() {
        2 => match db.pop_list(&args[1], left).await {
            Ok(Some(value)) => bulk(value),
            Ok(None) => nil(),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn list_len(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.list_len(&args[1]).await {
            Ok(len) => integer(len as i32),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn list_range(db: Db, args: Vec<String>) -> String {
    if args.len() != 4 {
        return syntax_error();
    }

    let start = match args[2].parse::<i64>() {
        Ok(start) => start,
        Err(_) => return syntax_error(),
    };
    let stop = match args[3].parse::<i64>() {
        Ok(stop) => stop,
        Err(_) => return syntax_error(),
    };

    match db.list_range(&args[1], start, stop).await {
        Ok(values) => array(values),
        Err(err) => db_error(err),
    }
}

async fn set_add(db: Db, args: Vec<String>) -> String {
    match args.len() {
        0..=2 => syntax_error(),
        _ => match db.set_add(&args[1], &args[2..]).await {
            Ok(count) => integer(count as i32),
            Err(err) => db_error(err),
        },
    }
}

async fn set_remove(db: Db, args: Vec<String>) -> String {
    match args.len() {
        0..=2 => syntax_error(),
        _ => match db.set_remove(&args[1], &args[2..]).await {
            Ok(count) => integer(count as i32),
            Err(err) => db_error(err),
        },
    }
}

async fn set_is_member(db: Db, args: Vec<String>) -> String {
    match args.len() {
        3 => match db.set_is_member(&args[1], &args[2]).await {
            Ok(true) => integer(1),
            Ok(false) => integer(0),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn set_card(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.set_card(&args[1]).await {
            Ok(count) => integer(count as i32),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn set_members(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.set_members(&args[1]).await {
            Ok(members) => array(members),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn hash_set(db: Db, args: Vec<String>) -> String {
    if args.len() < 4 || (args.len() - 2) % 2 != 0 {
        return syntax_error();
    }

    let pairs: Vec<_> = args[2..]
        .chunks_exact(2)
        .map(|chunk| (chunk[0].clone(), chunk[1].clone()))
        .collect();

    match db.hash_set(&args[1], &pairs).await {
        Ok(count) => integer(count as i32),
        Err(err) => db_error(err),
    }
}

async fn hash_get(db: Db, args: Vec<String>) -> String {
    match args.len() {
        3 => match db.hash_get(&args[1], &args[2]).await {
            Ok(Some(value)) => bulk(value),
            Ok(None) => nil(),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn hash_del(db: Db, args: Vec<String>) -> String {
    match args.len() {
        0..=2 => syntax_error(),
        _ => match db.hash_del(&args[1], &args[2..]).await {
            Ok(count) => integer(count as i32),
            Err(err) => db_error(err),
        },
    }
}

async fn hash_exists(db: Db, args: Vec<String>) -> String {
    match args.len() {
        3 => match db.hash_exists(&args[1], &args[2]).await {
            Ok(true) => integer(1),
            Ok(false) => integer(0),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

async fn hash_get_all(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.hash_get_all(&args[1]).await {
            Ok(values) => array(values),
            Err(err) => db_error(err),
        },
        _ => syntax_error(),
    }
}

fn db_error(err: DbError) -> String {
    match err {
        DbError::WrongType => {
            error("WRONGTYPE Operation against a key holding the wrong kind of value".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use crate::db::Db;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn set_get_and_expire_accept_uppercase_units() {
        let db = Db::new();

        assert_eq!(
            dispatch(
                db.clone(),
                vec![
                    "SET".into(),
                    "temp".into(),
                    "redis".into(),
                    "EX".into(),
                    "1".into()
                ]
            )
            .await,
            "+OK\r\n"
        );
        assert_eq!(
            dispatch(db.clone(), vec!["GET".into(), "temp".into()]).await,
            "$5\r\nredis\r\n"
        );

        sleep(Duration::from_millis(1100)).await;

        assert_eq!(
            dispatch(db, vec!["GET".into(), "temp".into()]).await,
            "$-1\r\n"
        );
    }

    #[tokio::test]
    async fn ping_with_message_returns_bulk_message() {
        let db = Db::new();

        assert_eq!(
            dispatch(db, vec!["PING".into(), "hello".into()]).await,
            "$5\r\nhello\r\n"
        );
    }

    #[tokio::test]
    async fn del_removes_keys() {
        let db = Db::new();

        dispatch(
            db.clone(),
            vec!["SET".into(), "name".into(), "redis".into()],
        )
        .await;

        assert_eq!(
            dispatch(db.clone(), vec!["DEL".into(), "name".into()]).await,
            ":1\r\n"
        );
        assert_eq!(
            dispatch(db, vec!["EXISTS".into(), "name".into()]).await,
            ":0\r\n"
        );
    }

    #[tokio::test]
    async fn list_commands_return_lengths_values_and_ranges() {
        let db = Db::new();

        assert_eq!(
            dispatch(
                db.clone(),
                vec!["LPUSH".into(), "letters".into(), "a".into(), "b".into()]
            )
            .await,
            ":2\r\n"
        );
        assert_eq!(
            dispatch(
                db.clone(),
                vec!["LRANGE".into(), "letters".into(), "0".into(), "-1".into()]
            )
            .await,
            "*2\r\n$1\r\nb\r\n$1\r\na\r\n"
        );
        assert_eq!(
            dispatch(db, vec!["RPOP".into(), "letters".into()]).await,
            "$1\r\na\r\n"
        );
    }

    #[tokio::test]
    async fn set_commands_count_members_and_return_sorted_members() {
        let db = Db::new();

        assert_eq!(
            dispatch(
                db.clone(),
                vec![
                    "SADD".into(),
                    "tags".into(),
                    "rust".into(),
                    "db".into(),
                    "rust".into()
                ]
            )
            .await,
            ":2\r\n"
        );
        assert_eq!(
            dispatch(
                db.clone(),
                vec!["SISMEMBER".into(), "tags".into(), "db".into()]
            )
            .await,
            ":1\r\n"
        );
        assert_eq!(
            dispatch(db, vec!["SMEMBERS".into(), "tags".into()]).await,
            "*2\r\n$2\r\ndb\r\n$4\r\nrust\r\n"
        );
    }

    #[tokio::test]
    async fn hash_commands_store_fields_and_return_flat_arrays() {
        let db = Db::new();

        assert_eq!(
            dispatch(
                db.clone(),
                vec![
                    "HSET".into(),
                    "user".into(),
                    "name".into(),
                    "chen".into(),
                    "age".into(),
                    "18".into(),
                ]
            )
            .await,
            ":2\r\n"
        );
        assert_eq!(
            dispatch(
                db.clone(),
                vec!["HGET".into(), "user".into(), "name".into()]
            )
            .await,
            "$4\r\nchen\r\n"
        );
        assert_eq!(
            dispatch(db, vec!["HGETALL".into(), "user".into()]).await,
            "*4\r\n$3\r\nage\r\n$2\r\n18\r\n$4\r\nname\r\n$4\r\nchen\r\n"
        );
    }

    #[tokio::test]
    async fn wrong_type_commands_return_error() {
        let db = Db::new();

        dispatch(
            db.clone(),
            vec!["SET".into(), "name".into(), "redis".into()],
        )
        .await;

        assert!(
            dispatch(db, vec!["LPUSH".into(), "name".into(), "value".into()])
                .await
                .starts_with("-ERR WRONGTYPE")
        );
    }
}
