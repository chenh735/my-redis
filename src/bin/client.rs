use anyhow::{Result, bail};
use clap::Parser;
use my_redis::resp::resp::{decode_response, encode_request};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, stdin,
};
use tokio::net::TcpStream;

#[derive(Debug, Parser)]
struct Args {
    // 服务端地址。
    #[arg(long, default_value = "127.0.0.1:6379")]
    addr: String,

    #[arg(long)]
    cmd: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum ClientCommandResult {
    Response(String),
    Exit,
    Empty,
}

const HELP_TEXT: &str = r#"Supported commands:
  Connection:
    HELP
    EXIT

  Generic:
    PING [message]
    ECHO message
    DEL key [key ...]
    EXISTS key [key ...]
    BGSAVE

  String:
    SET key value
    SET key value EX seconds
    SET key value PX milliseconds
    STRSET key value
    GET key
    STRGET key
    STRLEN key
    APPEND key value

  List:
    LPUSH key value [value ...]
    RPUSH key value [value ...]
    LPOP key
    RPOP key
    LLEN key
    LRANGE key start stop

  Set:
    SADD key member [member ...]
    SREM key member [member ...]
    SISMEMBER key member
    SCARD key
    SMEMBERS key
    SINTER key [key ...]
    SUNION key [key ...]
    SDIFF key [key ...]

  Hash:
    HSET key field value [field value ...]
    HGET key field
    HDEL key field [field ...]
    HEXISTS key field
    HGETALL key
    HLEN key
    HKEYS key
    HVALUES key

  Transaction:
    MULTI
    EXEC
    DISCARD"#;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if matches!(args.cmd.as_deref(), Some(cmd) if is_help_command(cmd)) {
        print_help();
        return Ok(());
    }

    let stream = TcpStream::connect(args.addr)
        .await
        .expect("TCP connect error");
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    if let Some(cmd) = args.cmd {
        match execute_command(&mut reader, &mut writer, &cmd).await? {
            ClientCommandResult::Response(resp) => println!("Success response: {resp}"),
            ClientCommandResult::Exit | ClientCommandResult::Empty => {}
        }
        return Ok(());
    }

    let input = stdin();
    let mut input = BufReader::new(input);
    run_interactive(&mut reader, &mut writer, &mut input).await
}

fn is_exit_command(command: &str) -> bool {
    command.trim().eq_ignore_ascii_case("exit")
}

fn is_help_command(command: &str) -> bool {
    command.trim().eq_ignore_ascii_case("help")
}

fn print_help() {
    println!("{HELP_TEXT}");
}

/// 将文本命令或原始 RESP 命令转换为发送给服务端的字节内容。
fn build_request(command: &str) -> Result<Option<String>> {
    if command.trim_start().starts_with('*') && command.ends_with("\r\n") {
        return Ok(Some(command.to_string()));
    }

    let command = command.trim();
    if command.is_empty() {
        return Ok(None);
    }

    encode_request(command.split_whitespace().map(|s| s.to_string()).collect())
}

/// 在当前 TCP 连接上发送一条命令，并等待一条响应。
async fn execute_command<R, W>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    command: &str,
) -> Result<ClientCommandResult>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if is_exit_command(command) {
        return Ok(ClientCommandResult::Exit);
    }

    let Some(request) = build_request(command)? else {
        return Ok(ClientCommandResult::Empty);
    };

    writer.write_all(request.as_bytes()).await.expect("client1");
    let resp = match decode_response(reader).await? {
        Some(resp) => resp,
        None => {
            eprintln!("resp is empty");
            bail!("resp is empty")
        }
    };

    Ok(ClientCommandResult::Response(resp))
}

/// 运行交互式客户端循环，直到标准输入关闭或用户输入 `exit`。
async fn run_interactive<R, W, I>(
    reader: &mut BufReader<R>,
    writer: &mut W,
    input: &mut I,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    I: AsyncBufRead + Unpin,
{
    loop {
        print!("my-redis> ");
        std::io::Write::flush(&mut std::io::stdout())?;

        let mut command = String::new();
        let n = input.read_line(&mut command).await?;
        if n == 0 {
            break;
        }

        if is_help_command(&command) {
            print_help();
            continue;
        }

        match execute_command(reader, writer, &command).await? {
            ClientCommandResult::Response(resp) => println!("Success response: {resp}"),
            ClientCommandResult::Exit => break,
            ClientCommandResult::Empty => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use my_redis::resp::resp::decode_request;
    use tokio::io::duplex;

    #[test]
    fn build_request_encodes_text_command() {
        let request = build_request("SET name redis").unwrap().unwrap();

        assert_eq!(request, "*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n");
    }

    #[test]
    fn build_request_encodes_windows_line_endings_as_text_command() {
        let request = build_request("PING\r\n").unwrap().unwrap();

        assert_eq!(request, "*1\r\n$4\r\nPING\r\n");
    }

    #[test]
    fn build_request_keeps_raw_resp_command() {
        let raw = "*1\r\n$4\r\nPING\r\n";

        assert_eq!(build_request(raw).unwrap(), Some(raw.to_string()));
    }

    #[test]
    fn build_request_skips_blank_command() {
        assert_eq!(build_request("   ").unwrap(), None);
    }

    #[test]
    fn exit_command_is_case_insensitive() {
        assert!(is_exit_command("exit"));
        assert!(is_exit_command(" EXIT \n"));
        assert!(!is_exit_command("PING"));
    }

    #[test]
    fn help_command_is_case_insensitive() {
        assert!(is_help_command("help"));
        assert!(is_help_command(" HELP \n"));
        assert!(!is_help_command("PING"));
    }

    #[test]
    fn help_text_lists_server_commands() {
        for command in [
            "PING", "ECHO", "SET", "GET", "LPUSH", "SADD", "HSET", "BGSAVE", "MULTI", "EXEC",
            "DISCARD",
        ] {
            assert!(HELP_TEXT.contains(command), "missing {command}");
        }
    }

    #[tokio::test]
    async fn execute_command_sends_request_and_reads_response() {
        let (client, server) = duplex(256);
        let (client_read, mut client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let mut client_reader = BufReader::new(client_read);

        let server = tokio::spawn(async move {
            let mut server_reader = BufReader::new(server_read);
            let request = decode_request(&mut server_reader).await.unwrap();
            assert_eq!(request, Some(vec!["PING".to_string()]));
            server_write.write_all(b"+PONG\r\n").await.unwrap();
        });

        let result = execute_command(&mut client_reader, &mut client_write, "PING")
            .await
            .unwrap();

        assert_eq!(result, ClientCommandResult::Response("PONG".to_string()));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn interactive_loop_reuses_connection_until_exit() {
        let (client, server) = duplex(512);
        let (client_read, mut client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let mut client_reader = BufReader::new(client_read);
        let mut input = BufReader::new("PING\nECHO hello\nexit\n".as_bytes());

        let server = tokio::spawn(async move {
            let mut server_reader = BufReader::new(server_read);

            let first = decode_request(&mut server_reader).await.unwrap();
            assert_eq!(first, Some(vec!["PING".to_string()]));
            server_write.write_all(b"+PONG\r\n").await.unwrap();

            let second = decode_request(&mut server_reader).await.unwrap();
            assert_eq!(second, Some(vec!["ECHO".to_string(), "hello".to_string()]));
            server_write.write_all(b"$5\r\nhello\r\n").await.unwrap();
        });

        run_interactive(&mut client_reader, &mut client_write, &mut input)
            .await
            .unwrap();

        server.await.unwrap();
    }

    #[tokio::test]
    async fn interactive_help_does_not_send_request_to_server() {
        let (client, server) = duplex(512);
        let (client_read, mut client_write) = tokio::io::split(client);
        let (server_read, mut server_write) = tokio::io::split(server);
        let mut client_reader = BufReader::new(client_read);
        let mut input = BufReader::new("help\nPING\nexit\n".as_bytes());

        let server = tokio::spawn(async move {
            let mut server_reader = BufReader::new(server_read);
            let request = decode_request(&mut server_reader).await.unwrap();
            assert_eq!(request, Some(vec!["PING".to_string()]));
            server_write.write_all(b"+PONG\r\n").await.unwrap();
        });

        run_interactive(&mut client_reader, &mut client_write, &mut input)
            .await
            .unwrap();

        server.await.unwrap();
    }
}
