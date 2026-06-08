use anyhow::{Context, Result, bail};
use std::io::ErrorKind;
use tokio::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub addr: String,
    pub port: u16,
    pub rdb_path: String,
    pub aof_path: String,
    pub aof_incr_path: String,
    pub aof_flush_sec: u64,
    pub rdb_save_sec: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            addr: "127.0.0.1".to_string(),
            port: 6379,
            rdb_path: "dump.rdb".to_string(),
            aof_path: "appendonly.aof".to_string(),
            aof_incr_path: "appendonly.aof.incr".to_string(),
            aof_flush_sec: 2,
            rdb_save_sec: 60,
        }
    }
}

impl ServerConfig {
    pub async fn load_or_default(path: &str) -> Result<Self> {
        let content = match fs::read_to_string(path).await {
            Ok(content) => content,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(err).with_context(|| format!("read config failed: {path}")),
        };

        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let mut config = Self::default();

        for (index, line) in content.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }

            let mut parts = line.split_whitespace();
            let key = parts.next().unwrap();
            let value = parts
                .next()
                .with_context(|| format!("config line {} missing value", index + 1))?;

            if parts.next().is_some() {
                bail!("config line {} has too many values", index + 1);
            }

            match key.to_ascii_lowercase().as_str() {
                "bind" => config.addr = value.to_string(),
                "port" => config.port = parse_port(value, index + 1)?,
                "dbfilename" => config.rdb_path = value.to_string(),
                "appendfilename" => config.aof_path = value.to_string(),
                "appendincrfilename" => config.aof_incr_path = value.to_string(),
                "appendfsync-seconds" => config.aof_flush_sec = parse_seconds(value, index + 1)?,
                "save-seconds" => config.rdb_save_sec = parse_seconds(value, index + 1)?,
                _ => bail!("unknown config key on line {}: {key}", index + 1),
            }
        }

        Ok(config)
    }
}

fn parse_port(value: &str, line: usize) -> Result<u16> {
    let port = value
        .parse::<u16>()
        .with_context(|| format!("invalid port on config line {line}: {value}"))?;

    if port == 0 {
        bail!("port must be greater than 0 on config line {line}");
    }

    Ok(port)
}

fn parse_seconds(value: &str, line: usize) -> Result<u64> {
    value
        .parse::<u64>()
        .with_context(|| format!("invalid seconds on config line {line}: {value}"))
}

#[cfg(test)]
mod tests {
    use super::ServerConfig;

    #[test]
    fn parse_accepts_redis_style_config() {
        let config = ServerConfig::parse(
            r#"
            # network
            bind 0.0.0.0
            port 6380

            dbfilename data.rdb
            appendfilename data.aof
            appendincrfilename data.aof.incr
            appendfsync-seconds 1
            save-seconds 30
            "#,
        )
        .unwrap();

        assert_eq!(config.addr, "0.0.0.0");
        assert_eq!(config.port, 6380);
        assert_eq!(config.rdb_path, "data.rdb");
        assert_eq!(config.aof_path, "data.aof");
        assert_eq!(config.aof_incr_path, "data.aof.incr");
        assert_eq!(config.aof_flush_sec, 1);
        assert_eq!(config.rdb_save_sec, 30);
    }

    #[test]
    fn parse_rejects_unknown_config_key() {
        let err = ServerConfig::parse("unknown yes").unwrap_err();
        assert!(err.to_string().contains("unknown config key"));
    }
}
