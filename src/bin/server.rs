use anyhow::Result;
use clap::Parser;
use my_redis::cmd::cmd::dispatch;
use my_redis::config::{ServerConfig, ServerConfigOverrides};
use my_redis::db::Db;
use my_redis::logger::{LogLevelArg, init_logging};
use my_redis::persist::{
    Aof, Rdb, is_bgsave_command, is_write_command, save_hybrid_snapshot, tick_flush,
    tick_hybrid_snapshot,
};
use my_redis::resp::resp::decode_request;
use my_redis::transaction::{Transaction, TransactionPersistence};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "redis.conf")]
    config: String,

    #[arg(long)]
    addr: Option<String>,

    #[arg(short, long)]
    port: Option<u16>,

    #[arg(long)]
    log: bool,

    #[arg(long, value_enum, default_value_t = LogLevelArg::Info)]
    log_level: LogLevelArg,

    #[arg(long)]
    idle_timeout_seconds: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
enum RequestRead {
    Request(Option<Vec<String>>),
    IdleTimeout,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    init_logging(args.log, args.log_level).expect("init logging failed");

    let mut config = ServerConfig::load_or_default(&args.config)
        .await
        .expect("load config failed");
    config.apply_overrides(ServerConfigOverrides {
        addr: args.addr,
        port: args.port,
        idle_timeout_sec: args.idle_timeout_seconds,
    });

    let listener = TcpListener::bind(format!("{}:{}", config.addr, config.port))
        .await
        .expect("TcpListener bind failed");
    println!("{}:{} server started", config.addr, config.port);
    log::info!("server started at {}:{}", config.addr, config.port);
    log::info!("idle timeout seconds: {}", config.idle_timeout_sec);

    let db = Db::new();
    let active_aof_path = Rdb::load_hybrid(
        &config.rdb_path,
        &config.aof_path,
        &config.aof_incr_path,
        db.clone(),
    )
    .await
    .expect("load persistence failed");
    log::info!("persistence loaded, active AOF path: {active_aof_path}");

    let aof = Arc::new(Mutex::new(
        Aof::open(&active_aof_path).await.expect("open AOF failed"),
    ));
    tick_flush(config.aof_flush_sec, aof.clone());
    tick_hybrid_snapshot(
        config.rdb_save_sec,
        config.rdb_path.clone(),
        config.aof_path.clone(),
        config.aof_incr_path.clone(),
        db.clone(),
        aof.clone(),
    );
    db.start_clean_up_keys();

    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        log::debug!("client connected: {addr}");
        let cur_db = db.clone();
        let cur_aof = aof.clone();
        let cur_config = config.clone();
        let idle_timeout = idle_timeout_duration(config.idle_timeout_sec);
        let cur_persistence = TransactionPersistence::new(
            config.rdb_path.clone(),
            config.aof_path.clone(),
            config.aof_incr_path.clone(),
        );
        tokio::spawn({
            async move {
                let (reader, mut writer) = socket.into_split();
                let mut reader = BufReader::new(reader);
                let mut transaction = Transaction::default();

                loop {
                    let request = read_request(&mut reader, idle_timeout).await;
                    let request = match request {
                        Ok(RequestRead::Request(r)) => r,
                        Ok(RequestRead::IdleTimeout) => {
                            log::info!("idle client disconnected: {addr}");
                            break;
                        }
                        Err(e) => {
                            log::error!("decode request error: {addr}, {e}");
                            break;
                        }
                    };
                    let text = match request {
                        Some(text) => text,
                        None => break,
                    };
                    if text.is_empty() {
                        break;
                    }

                    let mut response;
                    if let Some(tx_response) = transaction
                        .handle(
                            cur_db.clone(),
                            cur_aof.clone(),
                            cur_persistence.clone(),
                            text.clone(),
                        )
                        .await
                    {
                        response = tx_response;
                    } else if is_bgsave_command(&text) {
                        response = dispatch(cur_db.clone(), text.clone()).await;
                        if !response.starts_with('-') {
                            let save_db = cur_db.clone();
                            let save_aof = cur_aof.clone();
                            let save_config = cur_config.clone();
                            tokio::spawn(async move {
                                if let Err(e) = save_hybrid_snapshot(
                                    &save_config.rdb_path,
                                    &save_config.aof_path,
                                    &save_config.aof_incr_path,
                                    save_db,
                                    save_aof,
                                )
                                .await
                                {
                                    log::error!("hybrid snapshot error: {e}");
                                }
                            });
                        }
                    } else if is_write_command(&text) {
                        let mut aof = cur_aof.lock().await;
                        response = dispatch(cur_db.clone(), text.clone()).await;
                        if !response.starts_with('-') {
                            if let Err(e) = aof.append(&text).await {
                                log::error!("AOF append error: {e}");
                                response = "-ERR persistence error\r\n".to_string();
                            }
                        }
                    } else {
                        response = dispatch(cur_db.clone(), text.clone()).await;
                    }

                    if let Err(e) = writer.write_all(response.as_bytes()).await {
                        log::error!("write response error: {addr}, {e}");
                        break;
                    }
                }
                log::debug!("client disconnected: {addr}");
            }
        });
    }
}

/// Converts zero to disabled timeout and positive seconds to a Tokio timeout duration.
fn idle_timeout_duration(seconds: u64) -> Option<Duration> {
    if seconds == 0 {
        None
    } else {
        Some(Duration::from_secs(seconds))
    }
}

/// Reads one RESP request and returns IdleTimeout when the connection stays silent too long.
async fn read_request<R>(
    reader: &mut BufReader<R>,
    idle_timeout: Option<Duration>,
) -> Result<RequestRead>
where
    R: AsyncRead + Unpin,
{
    match idle_timeout {
        Some(duration) => match timeout(duration, decode_request(reader)).await {
            Ok(request) => request.map(RequestRead::Request),
            Err(_) => Ok(RequestRead::IdleTimeout),
        },
        None => decode_request(reader).await.map(RequestRead::Request),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[test]
    fn server_args_enable_logging_from_command_line() {
        let args = Args::try_parse_from([
            "server",
            "--log",
            "--log-level",
            "debug",
            "--addr",
            "127.0.0.1",
            "--port",
            "6380",
            "--idle-timeout-seconds",
            "10",
        ])
        .unwrap();

        assert!(args.log);
        assert_eq!(args.log_level, LogLevelArg::Debug);
        assert_eq!(args.addr, Some("127.0.0.1".to_string()));
        assert_eq!(args.port, Some(6380));
        assert_eq!(args.idle_timeout_seconds, Some(10));
    }

    #[test]
    fn server_args_disable_logging_by_default() {
        let args = Args::try_parse_from(["server"]).unwrap();

        assert!(!args.log);
        assert_eq!(args.log_level, LogLevelArg::Info);
        assert_eq!(args.idle_timeout_seconds, None);
    }

    #[test]
    fn idle_timeout_zero_disables_timeout() {
        assert_eq!(idle_timeout_duration(0), None);
        assert_eq!(idle_timeout_duration(3), Some(Duration::from_secs(3)));
    }

    #[tokio::test]
    async fn read_request_returns_idle_timeout_without_input() {
        let (client, server) = tokio::io::duplex(64);
        let mut reader = BufReader::new(server);

        let request = read_request(&mut reader, Some(Duration::from_millis(10)))
            .await
            .unwrap();

        assert_eq!(request, RequestRead::IdleTimeout);
        drop(client);
    }

    #[tokio::test]
    async fn read_request_receives_request_before_timeout() {
        let (mut client, server) = tokio::io::duplex(64);
        let mut reader = BufReader::new(server);

        client.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
        let request = read_request(&mut reader, Some(Duration::from_secs(1)))
            .await
            .unwrap();

        assert_eq!(
            request,
            RequestRead::Request(Some(vec!["PING".to_string()]))
        );
    }
}
