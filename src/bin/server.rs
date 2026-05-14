use clap::Parser;
use my_redis::cmd::cmd::dispatch;
use my_redis::db::Db;
use my_redis::persist::{Aof, is_write_command, tick_flush};
use my_redis::resp::resp::decode_request;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
struct Args {
    #[arg(default_value = "127.0.0.1", long)]
    addr: String,

    #[arg(short, long, default_value = "6379")]
    port: u32,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let listener = TcpListener::bind(format!("{}:{}", args.addr, args.port))
        .await
        .expect("TcpListener bind failed");
    println!("{}:{} server started", args.addr, args.port);
    let db = Db::new();
    Aof::load("appendonly.aof", db.clone())
        .await
        .expect("load AOF failed");
    let aof = Arc::new(Mutex::new(
        Aof::open("appendonly.aof").await.expect("open AOF failed"),
    ));
    tick_flush(2, aof.clone());
    db.start_clean_up_keys();

    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        let cur_db = db.clone();
        let cur_aof = aof.clone();
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
                    let mut response = dispatch(cur_db.clone(), text.clone()).await;
                    if is_write_command(&text) && !response.starts_with('-') {
                        if let Err(e) = cur_aof.lock().await.append(&text).await {
                            eprintln!("AOF append error:{e}");
                            response = "-ERR persistence error\r\n".to_string();
                        }
                    }
                    writer.write_all(response.as_bytes()).await.expect("main");
                }
            }
        });
    }
}
