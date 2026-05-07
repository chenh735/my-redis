pub mod db;
pub mod resp;
// use db::*;

use std::ascii::AsciiExt;
use std::fmt::Debug;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use clap::Parser;
use tokio::net::TcpListener;
use resp::resp::decode_request;



#[derive(Parser, Debug)]
struct Args{

    #[arg(default_value = "127.0.0.1", long)]
    addr: String,

    #[arg(short, long, default_value = "6379")]
    port: u32,
}

#[tokio::main]
async  fn main() {
    let args = Args::parse();
    let listener = TcpListener::bind(format!("{}:{}",args.addr,args.port)).await.expect("TcpListener绑定失败");
    println!("{}:{} 开启成功",args.addr,args.port);

    let db = db::new_db();
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
                        Ok(r)=> r,
                        Err(e)=>{
                            eprintln!("error:{addr},{e}");
                            break;
                        }
                    };
                    let text = match request {
                        Some(text) =>{
                            text
                        },
                        None=>{
                            break
                        }
                    };
                    if text.len() == 3 && text[0].eq_ignore_ascii_case("set") {
                        let key = &text[1];
                        let value = text[2..].join(" ");
                        cur_db.write().await.insert(key.to_string(), value.to_string());
                        writer.write_all(b"+OK\r\n").await.unwrap();
                    }else if text.len() == 2 && text[0].eq_ignore_ascii_case("get"){
                        let key = text[1].as_str();
                        if let Some(value) = cur_db.read().await.get(key).cloned() {
                            writer.write_all(format!("${}\r\n{value}\r\n",value.as_bytes().len()).as_bytes()).await.unwrap();
                        }else {
                            writer.write_all("$-1\r\n".as_bytes()).await.unwrap();
                        }
                    }else if text.len() >= 1 && text[0].eq_ignore_ascii_case("ping"){
                        if text.len() == 1 {
                            writer.write_all(b"+PONG\r\n").await.expect("PING won't fail");
                        }else if text.len() == 2 {
                            let msg = text[1].as_str();
                            let resp = format!("${}\r\n{}\r\n",msg.len(),msg);
                            writer.write_all(resp.as_bytes()).await.unwrap();
                        }else {
                            writer.write_all(b"-ERR wrong number of arguments for 'ping' command\r\n")
                                .await.unwrap();
                        }
                    }else if text.len() >= 1 && text[0].eq_ignore_ascii_case("echo"){
                        if text.len() == 2{
                            let msg = text[1].as_str();
                            let resp = format!("${}\r\n{}\r\n",msg.as_bytes().len(),msg);
                            writer.write_all(resp.as_bytes()).await.unwrap();
                        }else{
                            writer.write_all(b"-ERR wrong number of arguments for 'echo' command\r\n").await.unwrap();
                        }
                    }else if text.len() == 2 && text[0].eq_ignore_ascii_case("exist") {
                        let key = text[1].as_str();
                        if cur_db.read().await.contains_key(key){
                            writer.write_all(b":1\r\n").await.expect("exist won't fail");
                        }else{
                            writer.write_all(b":0\r\n").await.expect("exist won't fail");
                        }
                    }else if text.len() >= 2 && text[0].eq_ignore_ascii_case("exists"){
                        let mut exi_cnt = 0;
                        for key in &text[1..]{
                            if cur_db.read().await.contains_key(key){
                                exi_cnt += 1;
                            }
                        }
                        let resp = format!(":{exi_cnt}\r\n");
                        writer.write_all(resp.as_bytes()).await.expect("exists");
                    } else if text.len() >= 2 && text[0].eq_ignore_ascii_case("del") {
                        let mut write = cur_db.write().await;
                        let count = text[1..].iter().filter(|x| {
                            write.remove(*x).is_some()
                        }).count();
                        let resp = format!(":{count}\r\n");
                        writer.write_all(resp.as_bytes()).await.unwrap();
                    } else {
                        println!("text: {:?}", text);
                        writer.write_all(b"-Error: Invalid Command\r\n ").await.unwrap();
                    }
                }
            }
        });

    }
}
