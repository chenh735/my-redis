use clap::Parser;
use my_redis::cmd::cmd::dispatch;
use my_redis::config::ServerConfig;
use my_redis::db::Db;
use my_redis::persist::{
    Aof, Rdb, is_bgsave_command, is_write_command, save_hybrid_snapshot, tick_flush,
    tick_hybrid_snapshot,
};
use my_redis::resp::resp::decode_request;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
struct Args {
    #[arg(long, default_value = "redis.conf")]
    config: String,

    #[arg(long)]
    addr: Option<String>,

    #[arg(short, long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let mut config = ServerConfig::load_or_default(&args.config)
        .await
        .expect("load config failed");
    if let Some(addr) = args.addr {
        config.addr = addr;
    }
    if let Some(port) = args.port {
        config.port = port;
    }

    let listener = TcpListener::bind(format!("{}:{}", config.addr, config.port))
        .await
        .expect("TcpListener bind failed");
    println!("{}:{} server started", config.addr, config.port);
    let db = Db::new();
    let active_aof_path = Rdb::load_hybrid(
        &config.rdb_path,
        &config.aof_path,
        &config.aof_incr_path,
        db.clone(),
    )
    .await
    .expect("load persistence failed");
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
        let cur_db = db.clone();
        let cur_aof = aof.clone();
        let cur_config = config.clone();
        tokio::spawn({
            async move {
                let (reader, mut writer) = socket.into_split();
                let mut reader = BufReader::new(reader);

                loop {
                    let request = decode_request(&mut reader).await;
                    let request = match request {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("error:{addr},{e}");
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
                    if is_bgsave_command(&text) {
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
                                    eprintln!("hybrid snapshot error:{e}");
                                }
                            });
                        }
                    } else if is_write_command(&text) {
                        let mut aof = cur_aof.lock().await;
                        response = dispatch(cur_db.clone(), text.clone()).await;
                        if !response.starts_with('-') {
                            if let Err(e) = aof.append(&text).await {
                                eprintln!("AOF append error:{e}");
                                response = "-ERR persistence error\r\n".to_string();
                            }
                        }
                    } else {
                        response = dispatch(cur_db.clone(), text.clone()).await;
                    }

                    writer.write_all(response.as_bytes()).await.expect("main");
                }
            }
        });
    }
}
