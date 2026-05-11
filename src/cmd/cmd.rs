use crate::db::Db;
use crate::resp::resp::{bulk, integer, nil, simple, syntax_error};
use anyhow::{Result, bail};
use tokio::time::{Duration, Instant};

pub async fn dispatch(db: Db, args: Vec<String>) -> String {
    match args[0].to_ascii_lowercase().as_str() {
        "ping" => ping(args),
        "echo" => echo(args),
        "set" => set(db, args).await,
        "get" => get(db, args).await,
        "del" => del(db, args).await,
        "exists" => exists(db, args).await,
        _ => {
            eprintln!("dispatch1");
            syntax_error()
        }
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

fn expires_at(unit: &str, number: &str) -> Result<Instant> {
    let number = match number.parse::<u64>() {
        Ok(number) => number,
        Err(_) => {
            bail!("expires_at1")
        }
    };

    match unit.to_ascii_lowercase().as_str() {
        "ex" => Ok(Instant::now() + Duration::from_secs(number)),
        "px" => Ok(Instant::now() + Duration::from_millis(number)),
        _ => bail!("expires_at2"),
    }
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
            Err(e) => {
                eprintln!("{:?}", e);
                syntax_error()
            }
        },
        _ => {
            eprintln!("set1");
            syntax_error()
        }
    }
}

async fn get(db: Db, args: Vec<String>) -> String {
    match args.len() {
        2 => match db.get_key(args[1].as_str()).await {
            Some(value) => bulk(value),
            None => nil(),
        },
        _ => {
            eprintln!("get1");
            syntax_error()
        }
    }
}

async fn exists(db: Db, args: Vec<String>) -> String {
    match args.len() {
        1 => {
            eprintln!("exists1");
            syntax_error()
        }
        _ => {
            let count = db
                .exists(args[1..].iter().map(|x| x.as_str()).collect())
                .await;
            integer(count as i32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use crate::db::Db;
    use tokio::time::{Duration, sleep};

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
}

async fn del(db: Db, args: Vec<String>) -> String {
    match args.len() {
        1 => {
            eprintln!("del1");
            syntax_error()
        }
        _ => {
            let count = db
                .del_key(args[1..].iter().map(|x| x.as_str()).collect())
                .await;
            integer(count as i32)
        }
    }
}
