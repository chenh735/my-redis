use anyhow::{Result, bail};
use std::str;

pub fn parse_array(all: &[u8]) -> Result<Vec<Vec<String>>> {
    let mut pos = 0usize;
    let mut requests = Vec::new();

    while pos < all.len() {
        if all[pos] != b'*' {
            bail!("request array must start with *");
        }

        let end = read_line(pos + 1, all)?;
        let count: usize = str::from_utf8(&all[pos + 1..end])?.parse()?;
        pos = end + 2;

        let mut request = Vec::with_capacity(count);
        for _ in 0..count {
            if pos >= all.len() || all[pos] != b'$' {
                bail!("bulk string must start with $");
            }

            let end = read_line(pos + 1, all)?;
            let len: usize = str::from_utf8(&all[pos + 1..end])?.parse()?;
            pos = end + 2;

            if pos + len + 2 > all.len() {
                bail!("bulk string is shorter than declared length");
            }
            if &all[pos + len..pos + len + 2] != b"\r\n" {
                bail!("bulk string must end with CRLF");
            }

            request.push(String::from_utf8(all[pos..pos + len].to_vec())?);
            pos += len + 2;
        }

        requests.push(request);
    }

    Ok(requests)
}

pub fn read_line(mut pos: usize, all: &[u8]) -> Result<usize> {
    while pos + 1 < all.len() {
        if all[pos] == b'\r' && all[pos + 1] == b'\n' {
            return Ok(pos);
        }
        pos += 1;
    }
    bail!("missing CRLF")
}

#[cfg(test)]
mod tests {
    use super::parse_array;

    #[test]
    fn parse_array_accepts_multiple_resp_requests() {
        let input =
            b"*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n*2\r\n$3\r\nDEL\r\n$4\r\nname\r\n";

        assert_eq!(
            parse_array(input).unwrap(),
            vec![
                vec!["SET".to_string(), "name".to_string(), "redis".to_string()],
                vec!["DEL".to_string(), "name".to_string()],
            ]
        );
    }

    #[test]
    fn parse_array_accepts_empty_bulk_string() {
        let input = b"*3\r\n$3\r\nSET\r\n$5\r\nempty\r\n$0\r\n\r\n";

        assert_eq!(
            parse_array(input).unwrap(),
            vec![vec!["SET".to_string(), "empty".to_string(), "".to_string(),]]
        );
    }

    #[test]
    fn parse_array_rejects_invalid_resp() {
        assert!(parse_array(b"+OK\r\n").is_err());
        assert!(parse_array(b"*1\r\n+PING\r\n").is_err());
        assert!(parse_array(b"*1\r\n$3\r\nabcxx").is_err());
    }
}
