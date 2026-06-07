# my-redis

`my-redis` 是一个使用 Rust 和 Tokio 实现的简易 Redis。项目支持 RESP 协议解析、TCP 服务端/客户端、多种 Redis 数据结构、key 过期清理、简化事务、配置文件，以及 RDB + AOF 混合持久化。

## 功能特性

- 基于 Tokio 的异步 TCP 服务端，默认监听 `127.0.0.1:6379`
- 支持 RESP 请求解析和响应编码
- 使用 `Arc<RwLock<HashMap<String, Entry>>>` 保存共享数据库状态
- 支持 String、List、Set、Hash 四类数据结构
- 支持 key 过期时间：`SET key value EX seconds` / `SET key value PX milliseconds`
- 支持惰性删除和后台定时清理过期 key
- 支持 RDB 快照保存和加载
- 支持 AOF 增量日志和启动恢复
- 支持 RDB + AOF 混合持久化
- 支持 `BGSAVE` 手动保存快照
- 支持 `MULTI` / `EXEC` / `DISCARD` 简化事务
- 支持 `redis.conf` 配置文件
- 支持压力测试工具，可覆盖基础命令、String、List、Set、Hash 和混合场景

## 快速开始

启动服务端：

```powershell
cargo run
```

使用客户端发送命令：

```powershell
cargo run --bin client -- --cmd "PING"
cargo run --bin client -- --cmd "SET name redis"
cargo run --bin client -- --cmd "GET name"
```

指定地址和端口：

```powershell
cargo run --bin server -- --addr 127.0.0.1 --port 6380
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "PING"
```

## 配置文件

服务端默认读取 `redis.conf`。如果文件不存在，会使用代码中的默认配置。

示例配置：

```conf
# Network
bind 127.0.0.1
port 6379

# Persistence
dbfilename dump.rdb
appendfilename appendonly.aof
appendincrfilename appendonly.aof.incr

# Seconds. Set to 0 to disable the background task.
appendfsync-seconds 2
save-seconds 60
```

指定配置文件：

```powershell
cargo run --bin server -- --config redis.conf
```

命令行参数可以覆盖配置文件里的地址和端口：

```powershell
cargo run --bin server -- --config redis.conf --addr 0.0.0.0 --port 6380
```

## 支持的命令

### 通用命令

| 命令 | 说明 |
| --- | --- |
| `PING [message]` | 测试连接，带参数时返回参数内容 |
| `ECHO message` | 返回输入内容 |
| `DEL key [key ...]` | 删除一个或多个 key |
| `EXISTS key [key ...]` | 返回存在的 key 数量 |
| `BGSAVE` | 手动触发 RDB + AOF 混合快照 |

### String

| 命令 | 说明 |
| --- | --- |
| `SET key value` / `STRSET key value` | 设置字符串值 |
| `SET key value EX seconds` | 设置字符串值，并以秒为单位设置过期时间 |
| `SET key value PX milliseconds` | 设置字符串值，并以毫秒为单位设置过期时间 |
| `GET key` / `STRGET key` | 获取字符串值 |
| `STRLEN key` | 获取字符串长度 |
| `APPEND key value` | 追加字符串，返回新长度 |

示例：

```powershell
cargo run --bin client -- --cmd "SET name redis"
cargo run --bin client -- --cmd "GET name"
cargo run --bin client -- --cmd "SET temp redis EX 10"
```

### List

List 使用 `VecDeque<String>` 保存，支持从两端插入和弹出。

| 命令 | 说明 |
| --- | --- |
| `LPUSH key value [value ...]` | 从列表左侧插入一个或多个元素 |
| `RPUSH key value [value ...]` | 从列表右侧插入一个或多个元素 |
| `LPOP key` | 从列表左侧弹出一个元素 |
| `RPOP key` | 从列表右侧弹出一个元素 |
| `LLEN key` | 返回列表长度 |
| `LRANGE key start stop` | 返回指定范围内的列表元素，支持负数下标 |

示例：

```powershell
cargo run --bin client -- --cmd "LPUSH letters a b c"
cargo run --bin client -- --cmd "LRANGE letters 0 -1"
cargo run --bin client -- --cmd "RPOP letters"
```

`LPUSH letters a b c` 后，列表内容为 `c b a`，这和 Redis 的插入顺序一致。

### Set

Set 使用 `HashSet<String>` 保存，成员不重复。为了测试结果稳定，集合返回值会按字典序排序。

| 命令 | 说明 |
| --- | --- |
| `SADD key member [member ...]` | 添加一个或多个成员，返回新增成员数量 |
| `SREM key member [member ...]` | 删除一个或多个成员，返回成功删除数量 |
| `SISMEMBER key member` | 判断成员是否存在，存在返回 `1`，不存在返回 `0` |
| `SCARD key` | 返回集合成员数量 |
| `SMEMBERS key` | 返回集合全部成员 |
| `SINTER key [key ...]` | 返回多个集合的交集 |
| `SUNION key [key ...]` | 返回多个集合的并集 |
| `SDIFF key [key ...]` | 返回第一个集合和后续集合的差集 |

示例：

```powershell
cargo run --bin client -- --cmd "SADD tags rust db rust"
cargo run --bin client -- --cmd "SISMEMBER tags rust"
cargo run --bin client -- --cmd "SMEMBERS tags"
```

### Hash

Hash 使用 `HashMap<String, String>` 保存字段和值。

| 命令 | 说明 |
| --- | --- |
| `HSET key field value [field value ...]` | 设置一个或多个字段，返回新增字段数量 |
| `HGET key field` | 获取字段值 |
| `HDEL key field [field ...]` | 删除一个或多个字段，返回成功删除数量 |
| `HEXISTS key field` | 判断字段是否存在，存在返回 `1`，不存在返回 `0` |
| `HGETALL key` | 返回所有字段和值，按字段名排序后扁平化返回 |
| `HLEN key` | 返回字段数量 |
| `HKEYS key` | 返回所有 field |
| `HVALUES key` | 返回所有 value |

示例：

```powershell
cargo run --bin client -- --cmd "HSET user name chen age 18"
cargo run --bin client -- --cmd "HGET user name"
cargo run --bin client -- --cmd "HGETALL user"
```

## 事务

事务支持简化版 Redis 事务命令：

| 命令 | 说明 |
| --- | --- |
| `MULTI` | 开启事务 |
| `EXEC` | 执行事务队列 |
| `DISCARD` | 丢弃事务队列 |

事务逻辑：

```text
MULTI
SET name redis
GET name
EXEC
```

- `MULTI` 后，普通命令不会立即执行，而是进入当前连接的事务队列
- 入队成功返回 `QUEUED`
- `EXEC` 会按顺序执行队列，并返回每条命令的结果数组
- `DISCARD` 会清空队列并退出事务
- 事务中的写命令只会在 `EXEC` 真正执行成功后写入 AOF
- 事务中可以入队 `BGSAVE`，并在 `EXEC` 阶段触发快照

如果入队阶段发现命令不存在或参数错误，事务会被标记为 dirty。之后执行 `EXEC` 时会返回 `EXECABORT`，并且不会执行队列中已经入队的命令。

注意：当前命令行客户端每次只发送一条命令并断开连接，而事务状态绑定在单个 TCP 连接上。因此事务更适合通过持久连接客户端或单元测试验证。

## 类型检查

同一个 key 只能保存一种数据结构。比如先执行：

```powershell
cargo run --bin client -- --cmd "SET name redis"
```

再对 `name` 执行列表命令：

```powershell
cargo run --bin client -- --cmd "LPUSH name value"
```

会返回类似 Redis 的错误：

```text
ERR WRONGTYPE Operation against a key holding the wrong kind of value
```

内部通过 `Value` 枚举区分不同数据结构：

```rust
pub enum Value {
    String(String),
    List(VecDeque<String>),
    Set(HashSet<String>),
    Hash(HashMap<String, String>),
}
```

## 持久化

### AOF

服务端会把成功执行的写命令追加到 `appendonly.aof`。AOF 文件使用 RESP Array 格式保存命令。

例如：

```text
SET name redis
```

会保存为：

```text
*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n
```

读命令不会写入 AOF，例如 `GET`、`EXISTS`、`LRANGE`、`SMEMBERS`、`HGETALL`。

### RDB

RDB 使用 `serde_json` 保存完整数据库快照，默认文件名为 `dump.rdb`。保存时会先写入临时文件，再替换正式 RDB 文件，减少写入中断导致的文件损坏风险。

手动保存：

```powershell
cargo run --bin client -- --cmd "BGSAVE"
```

### RDB + AOF 混合持久化

启动恢复顺序：

```text
1. 先加载 dump.rdb
2. 再重放 appendonly.aof
```

`BGSAVE` 的核心流程：

```text
1. 获取当前 Db 快照
2. 切换当前 AOF 到增量文件 appendonly.aof.incr
3. 保存 dump.rdb
4. 删除旧 appendonly.aof
5. 将 appendonly.aof.incr 规范化回 appendonly.aof
```

这样 RDB 保存完整快照，AOF 保存快照之后的增量写命令。

## 压力测试

项目提供独立压测工具 `stress`，它会通过真实 TCP 连接发送 RESP 请求，覆盖从客户端编码、服务端解码、命令分发、DB 读写到响应返回的完整链路。

启动服务端：

```powershell
cargo run --bin server -- --addr 127.0.0.1 --port 6379
```

运行压测：

```powershell
cargo run --bin stress -- --addr 127.0.0.1:6379 --clients 50 --requests 20000 --pipeline 10 --workload advanced
```

常用参数：

| 参数 | 说明 |
| --- | --- |
| `--addr` | 服务端地址 |
| `--clients` | 并发 TCP 连接数 |
| `--requests` | 总请求数 |
| `--pipeline` | 每批发送多少请求后再读取响应 |
| `--workload` | 压测场景 |
| `--key-space` | 状态类 workload 使用的 key 数量 |
| `--value-size` | 写入 value 的字节数 |
| `--key-prefix` | 压测 key 前缀 |

支持的 workload：

| workload | 覆盖内容 |
| --- | --- |
| `ping` | `PING` |
| `set` / `get` / `mixed` | String 读写路径 |
| `list` | `LPUSH`、`RPUSH`、`LPOP`、`RPOP`、`LLEN`、`LRANGE` |
| `set-structure` | `SADD`、`SREM`、`SISMEMBER`、`SCARD`、`SMEMBERS`、`SINTER`、`SUNION`、`SDIFF` |
| `hash` | `HSET`、`HGET`、`HDEL`、`HEXISTS`、`HLEN`、`HKEYS`、`HVALUES`、`HGETALL` |
| `advanced` | 基础命令、String、List、Set、Hash 混合场景 |

压测输出示例：

```text
requests: 200000
success: 200000
failed: 0
elapsed_ms: 3688.12
qps: 54228.16
latency_avg_ms: 82.567
latency_p50_ms: 78.418
latency_p95_ms: 175.648
latency_p99_ms: 177.896
```

几组真实压测结果如下。测试环境为 Windows 本机，服务端和压测客户端运行在同一台机器上。

### 50 并发基础场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `ping` | 50 | 10000 | 10 | 284654.56 | 0.332ms | 0 |
| `set` | 50 | 20000 | 10 | 33121.50 | 1.841ms | 0 |
| `mixed` | 50 | 20000 | 10 | 61474.55 | 0.990ms | 0 |

### 50 并发高级数据结构场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list` | 50 | 20000 | 10 | 44752.03 | 2.053ms | 0 |
| `set-structure` | 50 | 20000 | 10 | 75968.82 | 0.881ms | 0 |
| `hash` | 50 | 20000 | 10 | 85652.55 | 0.845ms | 0 |
| `advanced` | 50 | 30000 | 10 | 68206.79 | 1.280ms | 0 |

### 高并发场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list` | 1000 | 200000 | 10 | 46779.31 | 26.903ms | 0 |
| `set-structure` | 1000 | 200000 | 10 | 70728.00 | 15.189ms | 0 |
| `hash` | 1000 | 200000 | 10 | 74710.54 | 14.007ms | 0 |
| `advanced` | 2000 | 100000 | 10 | 51636.65 | 37.855ms | 0 |
| `advanced` | 5000 | 100000 | 10 | 50627.44 | 89.727ms | 0 |
| `advanced` | 8000 | 200000 | 20 | 54228.16 | 177.896ms | 0 |

高并发压测也暴露并修复了一个服务端健壮性问题：客户端异常断开时，原先 RESP 读取和响应写入中的 `expect(...)` 可能导致 worker panic。现在连接读取错误会返回 `Err`，写响应失败会关闭当前连接，不再影响服务端整体运行。

## 测试

运行全部测试：

```powershell
cargo test
```

当前测试结果：

```text
47 个 lib 测试通过，9 个 stress 工具测试通过
```

常用测试命令：

```powershell
cargo test --lib cmd::cmd
cargo test --lib db::db
cargo test --lib resp::resp
cargo test --lib transaction
cargo test --lib persist
```

## 项目结构

```text
my-redis
├── Cargo.toml
├── README.md
├── redis.conf
├── dump.rdb
├── appendonly.aof
└── src
    ├── lib.rs
    ├── bin
    │   ├── client.rs
    │   ├── server.rs
    │   └── stress.rs
    ├── cmd
    │   ├── mod.rs
    │   └── cmd.rs
    ├── config.rs
    ├── db
    │   ├── mod.rs
    │   └── db.rs
    ├── persist
    │   ├── mod.rs
    │   ├── parse.rs
    │   ├── aof.rs
    │   └── rdb.rs
    ├── resp
    │   ├── mod.rs
    │   └── resp.rs
    └── transaction.rs
```
