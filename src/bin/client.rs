use anyhow::{Result, bail};
use clap::Parser;
use my_redis::resp::resp::{decode_response, encode_request};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Debug, Parser)]
struct Args {
    // 监听地址
    #[arg(long, default_value = "127.0.0.1:6379")]
    addr: String,

    #[arg(long)]
    cmd: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let stream = TcpStream::connect(args.addr)
        .await
        .expect("TCP connect error");
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut request = args.cmd;
    if !request.ends_with("\r\n") {
        request = match encode_request(request.split_whitespace().map(|s| s.to_string()).collect())?
        {
            Some(request) => request,
            None => {
                eprintln!("没有东西");
                bail!("错误")
            }
        }
    }
    writer.write_all(request.as_bytes()).await.expect("client1");
    let resp = match decode_response(&mut reader).await? {
        Some(resp) => resp,
        None => {
            eprintln!("resp is empty");
            bail!("resp is empty")
        }
    };
    println!("Success response: {resp}");
    Ok(())
}
