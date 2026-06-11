use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use my_redis::resp::resp::{decode_response, encode_request};
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::task::JoinSet;

#[derive(Clone, Debug, Parser)]
#[command(about = "Run a TCP/RESP stress test against my-redis")]
struct Args {
    // 服务端地址，例如 127.0.0.1:6379。
    #[arg(long, default_value = "127.0.0.1:6379")]
    addr: String,

    // 并发客户端连接数量。
    #[arg(short, long, default_value_t = 50)]
    clients: usize,

    // 所有客户端合计发送的请求总数。
    #[arg(short, long, default_value_t = 10000)]
    requests: usize,

    // 每个连接读取响应前批量发送的请求数量。
    #[arg(short, long, default_value_t = 1)]
    pipeline: usize,

    // 有状态压测场景使用的不同 key 数量。
    #[arg(long, default_value_t = 1000)]
    key_space: usize,

    // 写请求中 value 的字节大小。
    #[arg(long, default_value_t = 16)]
    value_size: usize,

    // 有状态压测场景使用的 key 前缀。
    #[arg(long, default_value = "myredis:stress")]
    key_prefix: String,

    // 要执行的压测场景。
    #[arg(long, value_enum, default_value_t = Workload::Ping)]
    workload: Workload,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Workload {
    Ping,
    Set,
    Get,
    Mixed,
    List,
    SetStructure,
    Hash,
    Advanced,
}

#[derive(Debug)]
struct WorkerResult {
    success: usize,
    failed: usize,
    latencies: Vec<Duration>,
}

#[derive(Debug)]
struct Summary {
    requests: usize,
    success: usize,
    failed: usize,
    elapsed: Duration,
    latencies: Vec<Duration>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    println!(
        "stress start addr={} clients={} requests={} pipeline={} workload={:?}",
        args.addr, args.clients, args.requests, args.pipeline, args.workload
    );

    let summary = run(args).await?;
    print_summary(&summary);

    if summary.failed > 0 {
        bail!("stress finished with {} failed requests", summary.failed);
    }

    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.clients == 0 {
        bail!("clients must be greater than 0");
    }
    if args.requests == 0 {
        bail!("requests must be greater than 0");
    }
    if args.pipeline == 0 {
        bail!("pipeline must be greater than 0");
    }
    if args.key_space == 0 {
        bail!("key-space must be greater than 0");
    }
    Ok(())
}

async fn run(args: Args) -> Result<Summary> {
    let started = Instant::now();
    let request_counts = split_requests(args.requests, args.clients);
    let mut workers = JoinSet::new();

    for (worker_id, requests) in request_counts.into_iter().enumerate() {
        if requests == 0 {
            continue;
        }

        let worker_args = args.clone();
        workers.spawn(async move { run_worker(worker_id, requests, worker_args).await });
    }

    let mut success = 0usize;
    let mut failed = 0usize;
    let mut latencies = Vec::with_capacity(args.requests);

    while let Some(result) = workers.join_next().await {
        let result = result.context("stress worker task failed")??;
        success += result.success;
        failed += result.failed;
        latencies.extend(result.latencies);
    }

    Ok(Summary {
        requests: args.requests,
        success,
        failed,
        elapsed: started.elapsed(),
        latencies,
    })
}

async fn run_worker(worker_id: usize, requests: usize, args: Args) -> Result<WorkerResult> {
    let stream = TcpStream::connect(&args.addr)
        .await
        .with_context(|| format!("connect {}", args.addr))?;
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut success = 0usize;
    let mut failed = 0usize;
    let mut latencies = Vec::with_capacity(requests);
    let mut sent = 0usize;

    while sent < requests {
        let batch_size = args.pipeline.min(requests - sent);
        let batch_started = Instant::now();
        let mut payload = String::new();

        for batch_index in 0..batch_size {
            let request_index = sent + batch_index;
            let command = build_command(worker_id, request_index, &args);
            let encoded = encode_request(command)
                .context("encode stress request")?
                .expect("encode_request always returns Some");
            payload.push_str(&encoded);
        }

        writer.write_all(payload.as_bytes()).await?;
        writer.flush().await?;

        for _ in 0..batch_size {
            match decode_response(&mut reader).await {
                Ok(Some(response)) if response.starts_with("ERR ") => failed += 1,
                Ok(Some(_)) => success += 1,
                Ok(None) => failed += 1,
                Err(_) => failed += 1,
            }
        }

        let per_request = duration_per_request(batch_started.elapsed(), batch_size);
        latencies.extend(std::iter::repeat(per_request).take(batch_size));
        sent += batch_size;
    }

    Ok(WorkerResult {
        success,
        failed,
        latencies,
    })
}

fn split_requests(total: usize, clients: usize) -> Vec<usize> {
    let base = total / clients;
    let remainder = total % clients;

    (0..clients)
        .map(|index| base + usize::from(index < remainder))
        .collect()
}

fn build_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    match args.workload {
        Workload::Ping => vec!["PING".to_string()],
        Workload::Set => build_set(worker_id, request_index, args),
        Workload::Get => build_get(worker_id, request_index, args),
        Workload::Mixed => {
            if request_index % 2 == 0 {
                build_set(worker_id, request_index, args)
            } else {
                build_get(worker_id, request_index - 1, args)
            }
        }
        Workload::List => build_list_command(worker_id, request_index, args),
        Workload::SetStructure => build_set_structure_command(worker_id, request_index, args),
        Workload::Hash => build_hash_command(worker_id, request_index, args),
        Workload::Advanced => build_advanced_command(worker_id, request_index, args),
    }
}

fn build_set(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    vec![
        "SET".to_string(),
        stress_key(worker_id, request_index, args),
        stress_value(request_index, args.value_size),
    ]
}

fn build_get(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    vec![
        "GET".to_string(),
        stress_key(worker_id, request_index, args),
    ]
}

fn stress_key(worker_id: usize, request_index: usize, args: &Args) -> String {
    typed_key("string", worker_id, request_index, args)
}

fn typed_key(kind: &str, worker_id: usize, request_index: usize, args: &Args) -> String {
    let key_id = request_index % args.key_space;
    format!("{}:{}:{}:{}", args.key_prefix, kind, worker_id, key_id)
}

fn build_list_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    let key = typed_key("list", worker_id, request_index, args);
    let value = stress_value(request_index, args.value_size);

    match request_index % 6 {
        0 => vec![
            "LPUSH".to_string(),
            key,
            value,
            stress_value(request_index + 1, args.value_size),
        ],
        1 => vec![
            "RPUSH".to_string(),
            key,
            value,
            stress_value(request_index + 1, args.value_size),
        ],
        2 => vec!["LLEN".to_string(), key],
        3 => vec!["LRANGE".to_string(), key, "0".to_string(), "-1".to_string()],
        4 => vec!["LPOP".to_string(), key],
        _ => vec!["RPOP".to_string(), key],
    }
}

fn build_set_structure_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    let key = typed_key("set", worker_id, request_index, args);
    let peer_key = typed_key("set-peer", worker_id, request_index, args);
    let member = stress_member(request_index);
    let peer_member = stress_member(request_index + 1);

    match request_index % 9 {
        0 => vec!["SADD".to_string(), key, member, peer_member],
        1 => vec!["SREM".to_string(), key, member],
        2 => vec!["SISMEMBER".to_string(), key, peer_member],
        3 => vec!["SCARD".to_string(), key],
        4 => vec!["SMEMBERS".to_string(), key],
        5 => vec![
            "SADD".to_string(),
            peer_key,
            peer_member,
            stress_member(request_index + 2),
        ],
        6 => vec!["SINTER".to_string(), key, peer_key],
        7 => vec!["SUNION".to_string(), key, peer_key],
        _ => vec!["SDIFF".to_string(), key, peer_key],
    }
}

fn build_hash_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    let key = typed_key("hash", worker_id, request_index, args);
    let field = stress_field(request_index);
    let value = stress_value(request_index, args.value_size);

    match request_index % 8 {
        0 => vec![
            "HSET".to_string(),
            key,
            field,
            value,
            stress_field(request_index + 1),
            stress_value(request_index + 1, args.value_size),
        ],
        1 => vec!["HGET".to_string(), key, field],
        2 => vec!["HEXISTS".to_string(), key, field],
        3 => vec!["HLEN".to_string(), key],
        4 => vec!["HKEYS".to_string(), key],
        5 => vec!["HVALUES".to_string(), key],
        6 => vec!["HGETALL".to_string(), key],
        _ => vec!["HDEL".to_string(), key, field],
    }
}

fn build_advanced_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    let inner_index = request_index / 4;

    match request_index % 4 {
        0 => build_basic_command(worker_id, inner_index, args),
        1 => build_list_command(worker_id, inner_index, args),
        2 => build_set_structure_command(worker_id, inner_index, args),
        _ => build_hash_command(worker_id, inner_index, args),
    }
}

fn build_basic_command(worker_id: usize, request_index: usize, args: &Args) -> Vec<String> {
    let key = stress_key(worker_id, request_index, args);
    let value = stress_value(request_index, args.value_size);

    match request_index % 8 {
        0 => vec!["PING".to_string()],
        1 => vec!["ECHO".to_string(), value],
        2 => vec!["SET".to_string(), key, value],
        3 => vec!["GET".to_string(), key],
        4 => vec!["STRLEN".to_string(), key],
        5 => vec!["APPEND".to_string(), key, value],
        6 => vec!["EXISTS".to_string(), key],
        _ => vec!["DEL".to_string(), key],
    }
}

fn stress_value(request_index: usize, value_size: usize) -> String {
    if value_size == 0 {
        return String::new();
    }

    let seed = format!("value-{request_index}-");
    seed.chars().cycle().take(value_size).collect()
}

fn stress_member(request_index: usize) -> String {
    format!("member-{request_index}")
}

fn stress_field(request_index: usize) -> String {
    format!("field-{request_index}")
}

fn duration_per_request(duration: Duration, requests: usize) -> Duration {
    if requests == 0 {
        return Duration::ZERO;
    }

    Duration::from_secs_f64(duration.as_secs_f64() / requests as f64)
}

fn print_summary(summary: &Summary) {
    let seconds = summary.elapsed.as_secs_f64();
    let qps = if seconds > 0.0 {
        summary.success as f64 / seconds
    } else {
        0.0
    };
    let stats = LatencyStats::from_latencies(summary.latencies.clone());

    println!("requests: {}", summary.requests);
    println!("success: {}", summary.success);
    println!("failed: {}", summary.failed);
    println!("elapsed_ms: {:.2}", seconds * 1000.0);
    println!("qps: {:.2}", qps);

    if let Some(stats) = stats {
        println!("latency_avg_ms: {:.3}", duration_ms(stats.avg));
        println!("latency_min_ms: {:.3}", duration_ms(stats.min));
        println!("latency_p50_ms: {:.3}", duration_ms(stats.p50));
        println!("latency_p95_ms: {:.3}", duration_ms(stats.p95));
        println!("latency_p99_ms: {:.3}", duration_ms(stats.p99));
        println!("latency_max_ms: {:.3}", duration_ms(stats.max));
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

#[derive(Debug, Eq, PartialEq)]
struct LatencyStats {
    avg: Duration,
    min: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
}

impl LatencyStats {
    fn from_latencies(mut latencies: Vec<Duration>) -> Option<Self> {
        if latencies.is_empty() {
            return None;
        }

        latencies.sort();
        let total: f64 = latencies.iter().map(Duration::as_secs_f64).sum();
        let avg = Duration::from_secs_f64(total / latencies.len() as f64);

        Some(Self {
            avg,
            min: latencies[0],
            p50: percentile(&latencies, 50),
            p95: percentile(&latencies, 95),
            p99: percentile(&latencies, 99),
            max: *latencies.last().expect("latencies is not empty"),
        })
    }
}

fn percentile(sorted_latencies: &[Duration], percentile: usize) -> Duration {
    let max_index = sorted_latencies.len() - 1;
    let index = (max_index * percentile).div_ceil(100);
    sorted_latencies[index]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn test_args(workload: Workload) -> Args {
        Args {
            addr: "127.0.0.1:0".to_string(),
            clients: 2,
            requests: 5,
            pipeline: 2,
            key_space: 3,
            value_size: 8,
            key_prefix: "test".to_string(),
            workload,
        }
    }

    #[test]
    fn split_requests_spreads_remainder_across_first_workers() {
        assert_eq!(split_requests(10, 3), vec![4, 3, 3]);
        assert_eq!(split_requests(2, 4), vec![1, 1, 0, 0]);
    }

    #[test]
    fn build_command_supports_all_workloads() {
        let ping = test_args(Workload::Ping);
        assert_eq!(build_command(0, 0, &ping), vec!["PING"]);

        let set = test_args(Workload::Set);
        assert_eq!(
            build_command(1, 4, &set),
            vec!["SET", "test:string:1:1", "value-4-"]
        );

        let get = test_args(Workload::Get);
        assert_eq!(build_command(1, 4, &get), vec!["GET", "test:string:1:1"]);

        let mixed = test_args(Workload::Mixed);
        assert_eq!(build_command(1, 0, &mixed)[0], "SET");
        assert_eq!(build_command(1, 1, &mixed), vec!["GET", "test:string:1:0"]);
    }

    #[test]
    fn list_workload_covers_list_commands() {
        let args = test_args(Workload::List);
        let commands: Vec<_> = (0..6).map(|index| build_command(0, index, &args)).collect();

        assert_eq!(
            command_names(&commands),
            vec!["LPUSH", "RPUSH", "LLEN", "LRANGE", "LPOP", "RPOP"]
        );
    }

    #[test]
    fn set_structure_workload_covers_set_commands() {
        let args = test_args(Workload::SetStructure);
        let commands: Vec<_> = (0..9).map(|index| build_command(0, index, &args)).collect();

        assert_eq!(
            command_names(&commands),
            vec![
                "SADD",
                "SREM",
                "SISMEMBER",
                "SCARD",
                "SMEMBERS",
                "SADD",
                "SINTER",
                "SUNION",
                "SDIFF",
            ]
        );
    }

    #[test]
    fn hash_workload_covers_hash_commands() {
        let args = test_args(Workload::Hash);
        let commands: Vec<_> = (0..8).map(|index| build_command(0, index, &args)).collect();

        assert_eq!(
            command_names(&commands),
            vec![
                "HSET", "HGET", "HEXISTS", "HLEN", "HKEYS", "HVALUES", "HGETALL", "HDEL",
            ]
        );
    }

    #[test]
    fn advanced_workload_mixes_basic_and_data_structure_commands() {
        let args = test_args(Workload::Advanced);
        let commands: Vec<_> = (0..36)
            .map(|index| build_command(0, index, &args))
            .collect();
        let names = command_names(&commands);

        for expected in [
            "PING",
            "ECHO",
            "SET",
            "GET",
            "STRLEN",
            "APPEND",
            "EXISTS",
            "DEL",
            "LPUSH",
            "RPUSH",
            "LLEN",
            "LRANGE",
            "LPOP",
            "RPOP",
            "SADD",
            "SREM",
            "SISMEMBER",
            "SCARD",
            "SMEMBERS",
            "SINTER",
            "SUNION",
            "SDIFF",
            "HSET",
            "HGET",
            "HEXISTS",
            "HLEN",
            "HKEYS",
            "HVALUES",
            "HGETALL",
            "HDEL",
        ] {
            assert!(names.contains(&expected), "{expected} was not generated");
        }
    }

    fn command_names(commands: &[Vec<String>]) -> Vec<&str> {
        commands.iter().map(|command| command[0].as_str()).collect()
    }

    #[test]
    fn validate_args_rejects_zero_values() {
        let mut args = test_args(Workload::Ping);
        assert!(validate_args(&args).is_ok());

        args.clients = 0;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn latency_stats_calculates_percentiles() {
        let latencies = vec![1, 2, 3, 4, 5]
            .into_iter()
            .map(Duration::from_millis)
            .collect();

        assert_eq!(
            LatencyStats::from_latencies(latencies),
            Some(LatencyStats {
                avg: Duration::from_millis(3),
                min: Duration::from_millis(1),
                p50: Duration::from_millis(3),
                p95: Duration::from_millis(5),
                p99: Duration::from_millis(5),
                max: Duration::from_millis(5),
            })
        );
    }

    #[tokio::test]
    async fn run_worker_sends_pipelined_requests_and_reads_responses() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let mut handled = 0usize;

            loop {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }

                let current = String::from_utf8_lossy(&buf[..n]).matches("*1\r\n").count();
                handled += current;
                for _ in 0..current {
                    socket.write_all(b"+PONG\r\n").await.unwrap();
                }

                if handled >= 4 {
                    break;
                }
            }
        });

        let args = Args {
            addr: addr.to_string(),
            clients: 1,
            requests: 4,
            pipeline: 2,
            key_space: 1,
            value_size: 0,
            key_prefix: "test".to_string(),
            workload: Workload::Ping,
        };

        let result = run_worker(0, 4, args).await.unwrap();
        assert_eq!(result.success, 4);
        assert_eq!(result.failed, 0);
        assert_eq!(result.latencies.len(), 4);

        server.await.unwrap();
    }
}
