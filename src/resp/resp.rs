use anyhow::{Result, bail};
use std::string::ToString;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};

pub async fn decode_request<R>(read: &mut BufReader<R>) -> Result<Option<Vec<String>>>
where
    R: AsyncRead + Unpin,
{
    let mut line = String::new();
    let n = read.read_line(&mut line).await.expect("read_command1");
    if n == 0 {
        return Ok(None);
    }
    let cur_line = line.trim_end_matches("\r\n");
    if !cur_line.starts_with("*") {
        bail!("第一行没有一*开头");
    }
    let count: usize = cur_line[1..].parse()?;
    let mut ans = Vec::<String>::with_capacity(count);
    for _ in 0..count {
        line.clear();
        read.read_line(&mut line).await.expect("read_command2");
        let cur_line = line.trim_end_matches("\r\n");
        if !cur_line.starts_with("$") {
            bail!("没有以$开头");
        }
        let len: usize = cur_line[1..].parse()?;
        let mut text = vec![0; len + 2];
        read.read_exact(&mut text).await.expect("read_command3");
        if !text.ends_with(b"\r\n") {
            bail!(r#"内容缺少\r\n"#);
        }
        ans.push(String::from_utf8(text[..len].to_vec())?);
    }
    Ok(Some(ans))
}

pub fn encode_request(request: Vec<String>) -> Result<Option<String>> {
    let mut ans = format!("*{}\r\n", request.len());
    for x in &request {
        let next = format!("${}\r\n{}\r\n", x.len(), x);
        ans.push_str(next.as_str());
    }
    Ok(Some(ans))
}

pub async fn decode_response<R>(read: &mut BufReader<R>) -> Result<Option<String>>
where
    R: AsyncRead + Unpin,
{
    let mut line = String::new();
    read.read_line(&mut line).await.expect("decode_response1");
    let cur_line = line.trim_end_matches("\r\n");
    let ans = match cur_line.as_bytes().first() {
        Some(b'+') => Ok(Some(cur_line[1..].to_string())),
        Some(b'-') => Ok(Some(cur_line[1..].to_string())),
        Some(b':') => Ok(Some(cur_line[1..].to_string())),
        Some(b'$') => {
            let num: i32 = cur_line[1..].parse()?;
            if num == -1 {
                return Ok(Some("nil".to_string()));
            }
            let num = num as usize;
            let mut buf = vec![0u8; num + 2];
            read.read_exact(&mut buf).await.expect("decode_response2");
            if &buf[num..] != b"\r\n" {
                bail!(format!("decode_response3:{:?}", buf))
            }
            Ok(Some(String::from_utf8(buf[..num].to_vec())?))
        }
        Some(b'*') => {
            let count: usize = cur_line[1..].parse()?;
            let mut values = Vec::with_capacity(count);

            for _ in 0..count {
                line.clear();
                read.read_line(&mut line).await.expect("decode_response4");
                let cur_line = line.trim_end_matches("\r\n");

                match cur_line.as_bytes().first() {
                    Some(b'+') | Some(b'-') | Some(b':') => values.push(cur_line[1..].to_string()),
                    Some(b'$') => {
                        let num: i32 = cur_line[1..].parse()?;
                        if num == -1 {
                            values.push("nil".to_string());
                            continue;
                        }
                        let num = num as usize;
                        let mut buf = vec![0u8; num + 2];
                        read.read_exact(&mut buf).await.expect("decode_response5");
                        if &buf[num..] != b"\r\n" {
                            bail!(format!("decode_response6:{:?}", buf))
                        }
                        values.push(String::from_utf8(buf[..num].to_vec())?);
                    }
                    _ => bail!("invalid array response item:{line:?}"),
                }
            }

            Ok(Some(values.join(" ")))
        }
        _ => {
            bail!("invalid response:{line:?}");
        }
    };
    ans
}

pub fn bulk(msg: String) -> String {
    format!("${}\r\n{msg}\r\n", msg.as_bytes().len())
}

pub fn array(items: Vec<String>) -> String {
    let mut response = format!("*{}\r\n", items.len());
    for item in items {
        response.push_str(&bulk(item));
    }
    response
}

pub fn raw_array(items: Vec<String>) -> String {
    let mut response = format!("*{}\r\n", items.len());
    for item in items {
        response.push_str(&item);
    }
    response
}

pub fn integer(num: i32) -> String {
    format!(":{num}\r\n")
}

pub fn nil() -> String {
    "$-1\r\n".to_string()
}

pub fn simple(msg: String) -> String {
    format!("+{msg}\r\n")
}

pub fn error(msg: String) -> String {
    format!("-ERR {msg}\r\n")
}

pub fn syntax_error() -> String {
    error("syntax error".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn decode_request_from(input: &'static [u8]) -> Result<Option<Vec<String>>> {
        let mut reader = BufReader::new(input);
        decode_request(&mut reader).await
    }

    async fn decode_response_from(input: &'static [u8]) -> Result<Option<String>> {
        let mut reader = BufReader::new(input);
        decode_response(&mut reader).await
    }

    #[test]
    fn encode_request_handles_empty_array_and_empty_bulk_string() {
        assert_eq!(encode_request(vec![]).unwrap(), Some("*0\r\n".to_string()));

        let encoded =
            encode_request(vec!["SET".to_string(), "empty".to_string(), "".to_string()]).unwrap();

        assert_eq!(
            encoded,
            Some("*3\r\n$3\r\nSET\r\n$5\r\nempty\r\n$0\r\n\r\n".to_string())
        );
    }

    #[test]
    fn bulk_uses_byte_length_for_non_ascii_text() {
        assert_eq!(
            bulk("\u{4f60}\u{597d}".to_string()),
            "$6\r\n\u{4f60}\u{597d}\r\n"
        );
    }

    #[tokio::test]
    async fn decode_request_accepts_empty_array_and_empty_bulk_string() {
        let empty_array = decode_request_from(b"*0\r\n").await.unwrap();
        assert_eq!(empty_array, Some(vec![]));

        let request = decode_request_from(b"*2\r\n$4\r\nECHO\r\n$0\r\n\r\n")
            .await
            .unwrap();
        assert_eq!(request, Some(vec!["ECHO".to_string(), "".to_string()]));
    }

    #[tokio::test]
    async fn decode_request_rejects_invalid_protocol() {
        assert!(decode_request_from(b"+PING\r\n").await.is_err());
        assert!(decode_request_from(b"*1\r\n+PING\r\n").await.is_err());
        assert!(decode_request_from(b"*x\r\n").await.is_err());
        assert!(decode_request_from(b"*1\r\n$x\r\n").await.is_err());
    }

    #[tokio::test]
    async fn decode_request_rejects_bulk_string_without_crlf_suffix() {
        let err = decode_request_from(b"*1\r\n$3\r\nabcxx").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn decode_response_accepts_bulk_string_boundaries() {
        let empty_bulk = decode_response_from(b"$0\r\n\r\n").await.unwrap();
        assert_eq!(empty_bulk, Some("".to_string()));

        let nil_bulk = decode_response_from(b"$-1\r\n").await.unwrap();
        assert_eq!(nil_bulk, Some("nil".to_string()));
    }

    #[tokio::test]
    async fn decode_response_rejects_invalid_protocol() {
        assert!(decode_response_from(b"*1\r\n").await.is_err());
        assert!(decode_response_from(b"$x\r\n").await.is_err());
    }

    #[tokio::test]
    async fn decode_response_rejects_bulk_string_without_crlf_suffix() {
        let err = decode_response_from(b"$3\r\nabcxx").await;
        assert!(err.is_err());
    }

    #[test]
    fn array_encodes_bulk_string_items() {
        assert_eq!(
            array(vec!["age".to_string(), "18".to_string()]),
            "*2\r\n$3\r\nage\r\n$2\r\n18\r\n"
        );
    }

    #[test]
    fn raw_array_keeps_item_response_types() {
        assert_eq!(
            raw_array(vec![simple("OK".to_string()), integer(1), nil()]),
            "*3\r\n+OK\r\n:1\r\n$-1\r\n"
        );
    }

    #[tokio::test]
    async fn decode_response_accepts_array_of_bulk_strings() {
        let response = decode_response_from(b"*2\r\n$3\r\nage\r\n$2\r\n18\r\n")
            .await
            .unwrap();

        assert_eq!(response, Some("age 18".to_string()));
    }
}
