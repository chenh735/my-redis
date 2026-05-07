pub mod db;
pub mod resp;

use crate::db::Db;
use clap::Parser;
use resp::resp::decode_request;
use std::fmt::Debug;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::time::{Duration, Instant};

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
        .expect("TcpListener绑定失败");
    println!("{}:{} 开启成功", args.addr, args.port);

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
                    if text[0].eq_ignore_ascii_case("set") {
                        if text.len() == 3 || text.len() == 5 {
                            let mut expires_at = None;
                            if text.len() == 5 {
                                let ttl: u64 = match text[4].parse() {
                                    Ok(ttl) => ttl,
                                    Err(_) => {
                                        writer
                                            .write_all(b"-ERR invalid expire time\r\n")
                                            .await
                                            .expect("set1");
                                        continue;
                                    }
                                };

                                if text[3].eq_ignore_ascii_case("ex") {
                                    expires_at = Some(Instant::now() + Duration::from_secs(ttl));
                                } else if text[3].eq_ignore_ascii_case("px") {
                                    expires_at = Some(Instant::now() + Duration::from_millis(ttl));
                                } else {
                                    writer
                                        .write_all(b"-ERR syntax error\r\n")
                                        .await
                                        .expect("set3");
                                    continue;
                                }
                            }
                            let key = text[1].as_str();
                            let value = text[2].as_str();
                            cur_db.set_key(key, value.to_string(), expires_at).await;
                            writer.write_all(b"+OK\r\n").await.expect("set4");
                        } else {
                            writer
                                .write_all(b"-ERR wrong number of arguments for 'set' command\r\n")
                                .await
                                .expect("set6");
                        }
                    } else if text[0].eq_ignore_ascii_case("get") {
                        if text.len() == 2 {
                            let key = text[1].as_str();
                            match cur_db.get_key(key).await {
                                Some(value) => {
                                    let resp =
                                        format!("${}\r\n{}\r\n", value.as_bytes().len(), value);
                                    writer.write_all(resp.as_bytes()).await.expect("get1");
                                }
                                None => {
                                    writer.write_all(b"$-1\r\n").await.expect("get2");
                                }
                            }
                        } else {
                            writer
                                .write_all(b"-ERR wrong number of arguments for 'get' command\r\n")
                                .await
                                .expect("get3");
                        }
                    } else if text.len() >= 1 && text[0].eq_ignore_ascii_case("ping") {
                        if text.len() == 1 {
                            writer
                                .write_all(b"+PONG\r\n")
                                .await
                                .expect("PING won't fail");
                        } else if text.len() == 2 {
                            let msg = text[1].as_str();
                            let resp = format!("${}\r\n{}\r\n", msg.as_bytes().len(), msg);
                            writer.write_all(resp.as_bytes()).await.unwrap();
                        } else {
                            writer
                                .write_all(b"-ERR wrong number of arguments for 'ping' command\r\n")
                                .await
                                .unwrap();
                        }
                    } else if text[0].eq_ignore_ascii_case("echo") {
                        if text.len() == 2 {
                            let msg = text[1].as_str();
                            let resp = format!("${}\r\n{}\r\n", msg.as_bytes().len(), msg);
                            writer.write_all(resp.as_bytes()).await.unwrap();
                        } else {
                            writer
                                .write_all(b"-ERR wrong number of arguments for 'echo' command\r\n")
                                .await
                                .unwrap();
                        }
                    } else if text[0].eq_ignore_ascii_case("exists") {
                        if text.len() > 1 {
                            let keys: Vec<&str> = text[1..].iter().map(|x1| x1.as_str()).collect();
                            let count = cur_db.exists(keys).await;
                            let resp = format!(":{count}\r\n");
                            writer.write_all(resp.as_bytes()).await.expect("exists1");
                        } else {
                            writer
                                .write_all(
                                    b"-ERR wrong number of arguments for 'exists' command\r\n",
                                )
                                .await
                                .expect("exists2");
                        }
                    } else if text[0].eq_ignore_ascii_case("del") {
                        if text.len() > 1 {
                            let keys: Vec<&str> = text[1..].iter().map(|s| s.as_str()).collect();
                            let count = cur_db.del_key(keys).await;
                            let resp = format!(":{count}\r\n");
                            writer.write_all(resp.as_bytes()).await.expect("del1");
                        } else {
                            writer
                                .write_all(b"-ERR wrong number of arguments for 'del' command\r\n")
                                .await
                                .expect("del2");
                        }
                    } else {
                        println!("text: {:?}", text);
                        writer.write_all(b"-ERR invalid command\r\n").await.unwrap();
                    }
                }
            }
        });
    }
}
