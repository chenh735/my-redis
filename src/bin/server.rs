use clap::Parser;
use my_redis::cmd::cmd::dispatch;
use my_redis::db::Db;
use my_redis::resp::resp::decode_request;
use std::fmt::Debug;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

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
    loop {
        let (socket, addr) = listener.accept().await.unwrap();
        let cur_db = db.clone();
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
                    let response = dispatch(cur_db.clone(), text).await;
                    writer.write_all(response.as_bytes()).await.expect("main");
                }
            }
        });
    }
}
