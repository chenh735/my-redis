use anyhow::{Result, bail};
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

pub async fn encode_request(request: Vec<String>) -> Result<Option<String>> {
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
        _ => {
            bail!("invalid response:{line:?}");
        }
    };
    ans
}
