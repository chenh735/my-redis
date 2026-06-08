---
title: 用 Rust 实现一个简易 Redis：从 RESP 协议到多数据结构支持
date: 2026-05-16
tags:
  - Rust
  - Redis
  - Tokio
  - 数据库
categories:
  - 项目实战
---

## 项目信息

- GitHub 地址：[my-redis](https://github.com/chenh735/my-redis)
- 技术栈：Rust、Tokio、RESP、Serde、RDB、AOF
- 项目定位：简化版 Redis，实现协议解析、命令分发、多数据结构、事务、配置文件和混合持久化

## 项目背景

在学习 Rust 后端开发和数据库基础原理的过程中，我希望做一个比普通 CRUD 更贴近真实系统的练手项目。Redis 是一个很适合拆解的目标：它有网络通信、协议解析、命令分发、内存数据结构、过期策略和持久化机制，但核心模型又足够清晰。

因此我实现了一个简化版 Redis，项目名为 `my-redis`。这个项目不是为了完整复刻 Redis，而是围绕 Redis 的核心机制做一次从 0 到 1 的实现，用来理解一个内存数据库服务端的基本工作方式。

项目当前支持：

- RESP 协议解析和编码
- TCP 服务端和命令行客户端
- String、List、Set、Hash 多种数据结构
- key 过期时间和后台清理
- RDB + AOF 混合持久化和启动恢复
- `MULTI` / `EXEC` / `DISCARD` 简化事务
- `redis.conf` 配置文件
- 命令行可选日志功能
- 空闲 TCP 连接超时主动断开
- 基础 Redis 命令和 Set 集合运算命令
- 压力测试工具，支持多连接并发、pipeline 和多种 workload

## 我的职责

这个项目由我独立完成，主要负责：

- 设计项目模块结构，将协议、命令、存储和持久化拆分到不同模块
- 使用 Tokio 实现异步 TCP 服务端
- 实现 RESP 请求解析和响应编码
- 设计内存数据库结构，支持多种 Redis 数据类型
- 实现命令分发层，将客户端命令映射到具体数据操作
- 实现 key 过期判断、惰性删除和后台定时清理
- 实现 RDB 快照、AOF 增量日志和混合持久化恢复
- 实现简化事务队列，支持命令入队、统一执行和事务预校验
- 实现配置文件解析，将监听地址、端口和持久化参数从代码中解耦
- 实现可选日志功能，用于定位服务端连接、持久化和网络读写问题
- 实现空闲 TCP 连接超时机制，避免无请求连接长期占用服务端资源
- 编写单元测试，覆盖 DB 层、命令层、RESP 协议、事务和持久化逻辑
- 实现压力测试工具，覆盖基础命令、String、List、Set、Hash 和混合高级场景
- 持续扩展命令能力，例如 `STRLEN`、`APPEND`、`HLEN`、`HKEYS`、`HVALS`、`SINTER`、`SUNION`、`SDIFF`

## 技术选型

### Rust

选择 Rust 的主要原因是它适合实现系统类项目。Redis 这类服务端涉及内存数据结构、并发共享状态和错误处理，Rust 的所有权系统、枚举和模式匹配能很好地表达这些逻辑。

例如数据库 value 使用 enum 建模：

```rust
pub enum Value {
    String(String),
    List(VecDeque<String>),
    Set(HashSet<String>),
    Hash(HashMap<String, String>),
}
```

相比使用字符串或动态类型，enum 可以让不同数据结构的类型边界更清晰。当某个命令操作了错误类型的 key 时，可以直接通过模式匹配返回 `WRONGTYPE` 错误。

### Tokio

服务端使用 Tokio 实现异步 TCP 通信。每个客户端连接都会被放到独立异步任务里处理，多个连接共享同一份数据库状态。

数据库结构使用：

```rust
pub struct Db {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}
```

其中：

- `Arc` 用于在多个连接任务之间共享数据库
- `RwLock` 用于保证并发读写安全
- `HashMap<String, Entry>` 用于保存 key-value 数据

### RESP 协议

Redis 客户端和服务端之间使用 RESP 协议通信。本项目实现了 RESP Array、Bulk String、Integer、Simple String、Error 等基础编码和解码能力。

例如命令：

```text
SET name redis
```

会被编码成：

```text
*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n
```

服务端解析 RESP 后得到参数数组，再交给命令分发层处理。

### RDB + AOF 混合持久化

持久化采用 RDB + AOF 混合方案。RDB 保存某一时刻的完整数据库快照，AOF 保存快照之后的增量写命令。服务启动时先加载 `dump.rdb`，再重放 `appendonly.aof`，既能减少 AOF 文件体积，也能保留增量恢复能力。

## 核心功能

### 1. TCP 服务端和客户端

服务端默认监听 `127.0.0.1:6379`，接收客户端连接后循环读取 RESP 请求，解析命令并返回响应。

客户端是一个简单命令行工具，可以通过 `--cmd` 参数发送命令：

```powershell
cargo run --bin client -- --cmd "PING"
cargo run --bin client -- --cmd "SADD tags rust db"
cargo run --bin client -- --cmd "SMEMBERS tags"
```

### 2. 命令分发

命令分发层接收 RESP 解码后的参数数组，根据第一个参数决定调用哪个处理函数：

```rust
match args[0].to_ascii_lowercase().as_str() {
    "ping" => ping(args),
    "echo" => echo(args),
    "set" | "strset" => strset(db, args).await,
    "get" | "strget" => strget(db, args).await,
    "lpush" => list_push(db, args, true).await,
    "sadd" => set_add(db, args).await,
    "sinter" => set_inter(db, args).await,
    "sunion" => set_union(db, args).await,
    "sdiff" => set_diff(db, args).await,
    "hset" => hash_set(db, args).await,
    _ => syntax_error(),
}
```

这一层主要负责参数校验、调用 DB 层和编码响应，不直接处理复杂的数据结构逻辑。

### 3. 多数据结构支持

项目当前支持四类 Redis 数据结构。

String 支持：

- `SET` / `STRSET`
- `GET` / `STRGET`
- `STRLEN`
- `APPEND`

List 支持：

- `LPUSH`
- `RPUSH`
- `LPOP`
- `RPOP`
- `LLEN`
- `LRANGE`

Set 支持：

- `SADD`
- `SREM`
- `SISMEMBER`
- `SCARD`
- `SMEMBERS`
- `SINTER`
- `SUNION`
- `SDIFF`

Hash 支持：

- `HSET`
- `HGET`
- `HDEL`
- `HEXISTS`
- `HGETALL`
- `HLEN`
- `HKEYS`
- `HVALS`

### 4. key 过期机制

每个 key 对应一个 `Entry`，其中保存 value 和过期时间：

```rust
struct Entry {
    value: Value,
    expires_at: Option<SystemTime>,
}
```

判断是否过期：

```rust
fn is_expired(&self) -> bool {
    self.expires_at
        .is_some_and(|expires_at| SystemTime::now() >= expires_at)
}
```

过期清理采用两种方式：

- 惰性删除：访问 key 时发现过期，立即删除
- 后台清理：服务启动后定时扫描并清理过期 key

### 5. RDB + AOF 混合持久化

写命令执行成功后，会被追加到 AOF 文件。当前支持持久化的写命令包括：

- `SET` / `STRSET`
- `DEL`
- `APPEND`
- `LPUSH`
- `RPUSH`
- `LPOP`
- `RPOP`
- `SADD`
- `SREM`
- `HSET`
- `HDEL`

服务启动时会读取 `appendonly.aof`，解析其中的 RESP 命令并重放：

```rust
for args in parse_array(&content)? {
    if !is_write_command(&args) {
        bail!("AOF only supports write commands: {}", args[0]);
    }

    let response = dispatch(db.clone(), args).await;
    if response.starts_with('-') {
        bail!("replay AOF command failed: {}", response.trim_end());
    }
}
```

在混合持久化中，AOF 不再承担全部恢复压力。`BGSAVE` 会保存一份 `dump.rdb` 快照，并将快照后的增量命令继续写入新的 AOF 文件。这样恢复时的流程变成：

```text
load_rdb("dump.rdb")
load_aof("appendonly.aof")
```

也就是先恢复完整快照，再补上快照之后发生的写命令。

### 6. 配置文件

为了避免服务启动参数写死在 `server.rs` 中，我增加了简化版 `redis.conf`：

```conf
bind 127.0.0.1
port 6379
dbfilename dump.rdb
appendfilename appendonly.aof
appendincrfilename appendonly.aof.incr
appendfsync-seconds 2
save-seconds 60
idle-timeout-seconds 300
```

服务启动时默认读取 `redis.conf`。如果配置文件不存在，则使用 `ServerConfig::default()` 中的默认配置。命令行参数仍然可以覆盖地址、端口和空闲连接超时时间：

```powershell
cargo run --bin server -- --config redis.conf
cargo run --bin server -- --addr 0.0.0.0 --port 6380 --idle-timeout-seconds 60
```

配置解析的思路比较简单：逐行读取，去掉注释和空行，然后按 `key value` 的形式写入配置结构体。

### 7. 日志功能

随着服务端功能变多，仅靠 `println!` 和 `eprintln!` 很难定位高并发下的连接问题和持久化错误。因此我增加了一个轻量日志模块 `logger`，复用项目已有的 `log` crate，不额外引入新的运行时依赖。

服务端默认不启用日志，避免普通运行时输出过多信息。需要排查问题时，可以通过命令行参数开启：

```powershell
cargo run --bin server -- --log
cargo run --bin server -- --log --log-level debug
```

当前支持的日志级别包括：

```text
error / warn / info / debug / trace
```

日志开启后，会记录服务端关键生命周期和错误信息，例如：

- 服务启动地址
- 持久化加载完成
- 客户端连接和断开
- RESP 请求解析错误
- AOF append / flush 错误
- RDB + AOF 混合快照错误
- 写响应失败

示例输出：

```text
[INFO server] server started at 127.0.0.1:6379
[INFO server] persistence loaded, active AOF path: appendonly.aof
[DEBUG server] client connected: 127.0.0.1:52130
```

这个功能在高并发压测时尤其有用。比如客户端异常断开、Windows socket 资源耗尽、写响应失败等问题，都可以通过日志快速定位到服务端连接生命周期中的具体阶段。

### 8. 空闲连接超时

TCP 服务端还有一个容易被忽略的问题：客户端连接建立后，如果长时间不发送任何请求，服务端会一直保留这个连接任务和 socket 资源。在普通测试里这不明显，但在压测或异常客户端场景下，空闲连接过多会影响服务端可用性。

为了解决这个问题，我为服务端增加了空闲连接超时机制。配置项如下：

```conf
idle-timeout-seconds 300
```

也可以通过命令行覆盖：

```powershell
cargo run --bin server -- --idle-timeout-seconds 60
```

其中 `0` 表示禁用空闲连接超时：

```powershell
cargo run --bin server -- --idle-timeout-seconds 0
```

实现上没有引入新依赖，而是使用 Tokio 自带的 `timeout` 包装每次请求读取：

```rust
match timeout(duration, decode_request(reader)).await {
    Ok(request) => request.map(RequestRead::Request),
    Err(_) => Ok(RequestRead::IdleTimeout),
}
```

也就是说，服务端不是限制一个连接最多存在多久，而是限制“距离上一条请求之后，最多可以空闲多久”。如果客户端在超时时间内发送了请求，连接会继续保留；如果一直不发请求，服务端会主动断开当前连接，并在开启日志时输出：

```text
[INFO server] idle client disconnected: 127.0.0.1:52130
```

这个功能可以减少空闲连接长期占用资源，也让服务端在异常客户端或连接泄漏场景下更稳。

### 9. 事务功能

事务功能实现了简化版：

- `MULTI`：开启事务
- 普通命令：不立即执行，只进入队列，返回 `QUEUED`
- `EXEC`：按顺序执行队列中的命令
- `DISCARD`：清空队列并退出事务

示例：

```text
MULTI
SET name redis
GET name
EXEC
```

执行结果中，`SET` 和 `GET` 会在 `EXEC` 阶段统一执行，并以 RESP Array 的形式返回每条命令的结果。事务中的写命令也只会在 `EXEC` 真正执行成功后写入 AOF。如果事务中包含 `BGSAVE`，也会等到 `EXEC` 阶段再触发 RDB + AOF 混合快照。

### 10. 压力测试工具

在功能基本完成后，我又补充了一个独立的压力测试工具 `stress`。它不是直接调用内部函数，而是像真实客户端一样通过 TCP 连接服务端，并使用 RESP 协议发送请求。这样可以覆盖完整链路：

```text
stress 客户端
-> TCP 连接
-> RESP 编码
-> 服务端 RESP 解码
-> 命令分发
-> DB 读写
-> AOF 持久化
-> RESP 响应
-> TCP 返回
```

这个工具支持几个核心参数：

```powershell
cargo run --bin stress -- --addr 127.0.0.1:6379 --clients 50 --requests 20000 --pipeline 10 --workload advanced
```

- `--clients`：并发 TCP 连接数
- `--requests`：总请求数
- `--pipeline`：每批发送多少请求后再读取响应
- `--workload`：压测场景

目前支持的 workload 包括：

| workload | 覆盖内容 |
| --- | --- |
| `ping` | `PING`，用于测试低副作用基础链路 |
| `set` / `get` / `mixed` | String 读写路径 |
| `list` | `LPUSH`、`RPUSH`、`LPOP`、`RPOP`、`LLEN`、`LRANGE` |
| `set-structure` | `SADD`、`SREM`、`SISMEMBER`、`SCARD`、`SMEMBERS`、`SINTER`、`SUNION`、`SDIFF` |
| `hash` | `HSET`、`HGET`、`HDEL`、`HEXISTS`、`HLEN`、`HKEYS`、`HVALUES`、`HGETALL` |
| `advanced` | 基础命令、String、List、Set、Hash 混合场景 |

压测结果会输出总请求数、成功数、失败数、耗时、QPS 和延迟分位数：

```text
requests: 200000
success: 200000
failed: 0
qps: 54228.16
latency_p50_ms: 78.418
latency_p95_ms: 175.648
latency_p99_ms: 177.896
```

下面是几组实际压测样例。测试环境为 Windows 本机，服务端和压测客户端运行在同一台机器上，因此结果会同时受到服务端实现、客户端连接数、操作系统 socket 资源和本机调度的影响。

#### 50 并发基础场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `ping` | 50 | 10000 | 10 | 284654.56 | 0.332ms | 0 |
| `set` | 50 | 20000 | 10 | 33121.50 | 1.841ms | 0 |
| `mixed` | 50 | 20000 | 10 | 61474.55 | 0.990ms | 0 |

#### 50 并发高级数据结构场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list` | 50 | 20000 | 10 | 44752.03 | 2.053ms | 0 |
| `set-structure` | 50 | 20000 | 10 | 75968.82 | 0.881ms | 0 |
| `hash` | 50 | 20000 | 10 | 85652.55 | 0.845ms | 0 |
| `advanced` | 50 | 30000 | 10 | 68206.79 | 1.280ms | 0 |

#### 高并发场景

| workload | clients | requests | pipeline | QPS | P99 延迟 | failed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `list` | 1000 | 200000 | 10 | 46779.31 | 26.903ms | 0 |
| `set-structure` | 1000 | 200000 | 10 | 70728.00 | 15.189ms | 0 |
| `hash` | 1000 | 200000 | 10 | 74710.54 | 14.007ms | 0 |
| `advanced` | 2000 | 100000 | 10 | 51636.65 | 37.855ms | 0 |
| `advanced` | 5000 | 100000 | 10 | 50627.44 | 89.727ms | 0 |
| `advanced` | 8000 | 200000 | 20 | 54228.16 | 177.896ms | 0 |

从结果可以看到，低并发下 `PING` 这类低副作用命令吞吐最高；`SET` 由于会写内存并追加 AOF，吞吐明显低一些；List、Set、Hash 等高级结构在 50 并发下都能保持 0 失败。连接数提升到 8000 后，`advanced` 混合 workload 仍然能完成 20 万请求且失败数为 0，但 P99 延迟明显上升。这说明服务端在高连接压力下保持了稳定性，不过大量连接调度、pipeline 排队、共享 DB 锁竞争和本机 socket 资源都会带来更高尾延迟。

这部分的意义不只是看 QPS。更重要的是，它能验证服务端在大量连接、pipeline 请求、读写混合、客户端异常断开等情况下是否稳定。后面一次高并发压测中，`stress` 就帮助我发现并修复了连接被中止时服务端 worker panic 的问题。

## 难点与解决方案

### 难点一：如何支持多种 value 类型

如果数据库只使用 `HashMap<String, String>`，实现会很简单，但无法扩展 List、Set、Hash。

解决方案是引入 `Value` 枚举：

```rust
pub enum Value {
    String(String),
    List(VecDeque<String>),
    Set(HashSet<String>),
    Hash(HashMap<String, String>),
}
```

这样所有 key 仍然存在同一个 key 空间中，但每个 key 的 value 可以是不同数据结构。命令执行时通过模式匹配判断类型，如果类型不匹配就返回 `WrongType`。

例如列表插入逻辑：

```rust
match write.get_mut(key) {
    Some(entry) => match &mut entry.value {
        Value::List(list) => {
            push_values(list, values, left);
            Ok(list.len())
        }
        _ => Err(DbError::WrongType),
    },
    None => {
        let mut list = VecDeque::new();
        push_values(&mut list, values, left);
        write.insert(key.to_string(), Entry {
            value: Value::List(list),
            expires_at: None,
        });
        Ok(values.len())
    }
}
```

### 难点二：过期 key 如何处理

过期 key 不能只在 `GET` 时处理，因为其他命令比如 `EXISTS`、`DEL`、`SADD` 也可能访问到过期 key。

解决方案是组合使用惰性删除和后台清理：

- 命令访问 key 时先判断是否过期
- 后台任务周期性清理所有过期 key

后台清理代码：

```rust
async fn clean_up_keys(&self) {
    let mut db = self.inner.write().await;
    db.retain(|_key, entry| !entry.is_expired());
}
```

这样即使某些 key 长时间不被访问，也能被定时任务清理掉。

### 难点三：集合运算的语义处理

Set 命令中，`SINTER`、`SUNION`、`SDIFF` 都涉及多个 key。这里需要同时处理几种情况：

- key 不存在
- key 存在但不是 Set
- key 已经过期
- 返回结果顺序不稳定

我的处理方式是：

- 不存在的 key 按空集合处理
- 类型不匹配返回 `WRONGTYPE`
- 操作前先清理过期 key
- 返回前排序，保证测试和输出稳定

`SUNION` 的实现思路是把多个集合的成员放入同一个 `HashSet`：

```rust
let mut values = HashSet::new();
for key in keys {
    match write.get(key) {
        Some(entry) => match &entry.value {
            Value::Set(set) => values.extend(set.iter().cloned()),
            _ => return Err(DbError::WrongType),
        },
        None => {}
    }
}
```

`SDIFF` 的实现思路是先复制第一个集合，再依次删除后续集合中出现过的成员：

```rust
let mut values = match write.get(&keys[0]) {
    Some(entry) => match &entry.value {
        Value::Set(set) => set.clone(),
        _ => return Err(DbError::WrongType),
    },
    None => return Ok(Vec::new()),
};

for key in &keys[1..] {
    match write.get(key) {
        Some(entry) => match &entry.value {
            Value::Set(set) => values.retain(|value| !set.contains(value)),
            _ => return Err(DbError::WrongType),
        },
        None => {}
    }
}
```

### 难点四：AOF 如何恢复内存状态

AOF 的关键不是保存最终数据，而是保存写命令。服务启动时只需要按顺序重放写命令，就可以恢复数据库状态。

这个过程有两个细节：

- AOF 中只能出现写命令，读命令不应该写入
- 重放时如果某条命令执行失败，要及时暴露错误

因此我实现了 `is_write_command` 来判断命令是否应该进入 AOF，同时在加载 AOF 时检查重放结果。

### 难点五：RDB 和 AOF 如何配合

只使用 AOF 时，文件会随着写命令不断变大；只使用 RDB 时，又可能丢失最近一次快照之后的写入。因此我采用了混合持久化：

```text
RDB 保存完整快照
AOF 保存快照后的增量命令
```

`BGSAVE` 的核心流程是：

```text
1. 获取当前数据库快照
2. 切换到增量 AOF 文件
3. 保存 dump.rdb
4. 删除旧 AOF
5. 将增量 AOF 规范化回 appendonly.aof
```

这里最容易出问题的是 AOF 文件切换。如果快照保存过程中仍然继续写旧 AOF，RDB 和 AOF 的时间点就可能对不上。因此我在保存快照时会通过 AOF 锁串行化写命令和文件切换，保证快照和增量日志之间的边界清晰。

### 难点六：事务如何保存客户端状态

事务不是全局数据库状态，而是每个客户端连接自己的状态。一个客户端执行 `MULTI` 后，后续命令需要先进入当前连接的事务队列，不能影响其他客户端。

因此我在每个连接循环中创建一个 `Transaction`：

```rust
let mut transaction = Transaction::default();
```

事务内部保存：

```rust
pub struct Transaction {
    queued: Option<Vec<Vec<String>>>,
    dirty: bool,
    dirty_cmd: Vec<String>,
}
```

- `queued` 表示当前是否处于事务中，以及已经入队的命令
- `dirty` 表示入队阶段是否出现过语法错误
- `dirty_cmd` 用来记录导致事务失败的命令

事务执行时，普通命令不会直接调用 `dispatch`，而是先保存到队列中。等 `EXEC` 到来后，再按顺序执行队列。对于 `BGSAVE` 这类除了返回结果之外还有副作用的命令，则在 `EXEC` 中额外触发快照保存。这样可以比较清楚地体现 Redis 事务的核心思想：先排队，后统一执行。

### 难点七：事务预校验和 EXECABORT

事务中有两类错误：

- 入队阶段错误：比如命令不存在、参数数量不对
- 执行阶段错误：比如对 String 类型执行 `LPUSH`，触发 `WRONGTYPE`

入队阶段错误可以提前判断，所以我实现了 `validate_command`。如果事务中出现语法错误，就将事务标记为 dirty：

```rust
if let Err(err) = validate_command(&args) {
    self.dirty = true;
    self.dirty_cmd = args;
    return error(err);
}
```

之后执行 `EXEC` 时，不会再执行队列里的任何命令，而是返回 `EXECABORT` 并清空事务状态：

```rust
if self.dirty {
    let dirty_cmd = std::mem::take(&mut self.dirty_cmd);
    self.clear();
    return error(format!(
        "EXECABORT Transaction discarded because of previous errors: {:?}",
        dirty_cmd
    ));
}
```

这样可以避免下面这种情况：

```text
MULTI
SET name redis
GET
EXEC
```

由于 `GET` 参数错误，最终 `EXEC` 不会执行前面的 `SET name redis`。

## 一次高并发压测问题复盘

在给项目补充压力测试工具后，我用 `stress` 对服务端做了更高并发的连接压测。压测工具通过真实 TCP 连接发送 RESP 请求，支持 `PING`、`SET`、`GET`、`MIXED`、`LIST`、`SET-STRUCTURE`、`HASH` 和 `ADVANCED` 多种 workload。

例如：

```powershell
cargo run --bin stress -- --addr 127.0.0.1:6379 --clients 5000 --requests 100000 --pipeline 10 --workload advanced
```

在 1000、2000、5000 并发连接下，业务请求都能保持 `failed = 0`。继续冲击到更高连接数时，Windows 本机作为压测客户端出现过 socket 资源限制：

```text
通常每个套接字地址(协议/网络地址/端口)只允许使用一次。 (os error 10048)
```

这个错误主要来自客户端本机临时端口或 `TIME_WAIT` 连接资源耗尽，不是 Redis 命令逻辑错误。不过在这个过程中，服务端也暴露出一个更重要的健壮性问题：当客户端或操作系统中止连接时，服务端 worker 会 panic。

典型日志如下：

```text
thread 'tokio-rt-worker' panicked at src\bin\server.rs:146:65:
main: Os { code: 10053, kind: ConnectionAborted, message: "你的主机中的软件中止了一个已建立的连接。" }

thread 'tokio-rt-worker' panicked at src\resp\resp.rs:10:45:
read_command1: Os { code: 10054, kind: ConnectionReset, message: "远程主机强迫关闭了一个现有的连接。" }
```

问题根因是代码把普通网络错误当成了不可恢复错误处理。RESP 解码阶段使用了 `expect("read_command1")`、`expect("read_command2")`、`expect("read_command3")` 等写法；服务端写响应时也使用了：

```rust
writer.write_all(response.as_bytes()).await.expect("main");
```

在真实网络环境和高并发压测下，客户端断开、连接 reset、半包后关闭都很常见。这些情况不应该让服务端 panic，而应该只关闭当前连接。

修复方式是把网络读写错误显式返回或处理：

```rust
let n = read.read_line(&mut line).await?;
read.read_exact(&mut text).await?;
```

服务端写响应也改成：

```rust
if let Err(e) = writer.write_all(response.as_bytes()).await {
    eprintln!("write response error:{addr},{e}");
    break;
}
```

这样单个连接的异常只会结束当前连接任务，不会造成 Tokio worker panic。

同时我补充了截断输入测试，确保 RESP 层在遇到意外 EOF 时返回 `Err`，而不是 panic：

```rust
#[tokio::test]
async fn decode_request_returns_error_on_unexpected_eof() {
    let err = decode_request_from(b"*1\r\n$3\r\nab").await;
    assert!(err.is_err());
}
```

修复后重新验证：

```text
rustfmt src\resp\resp.rs src\bin\server.rs --edition 2024 --check 通过
cargo test --lib resp::resp 通过，13 passed
cargo test 通过，47 个 lib 测试 + 9 个 stress 测试
断连/重置端到端检查通过：abort-check: no server panic found
```

这次问题让我更直观地意识到：单元测试能验证协议和命令的正确性，但压力测试更容易暴露网络服务端在真实连接生命周期中的问题。对于 TCP 服务端来说，连接被对端关闭、reset 或写响应失败都应该是普通分支，而不是 panic 分支。

## 成果

目前项目已经实现了一个可运行的简化 Redis 服务端，支持通过命令行客户端进行交互。

示例：

```powershell
cargo run
```

另开终端执行：

```powershell
cargo run --bin client -- --cmd "SADD a one two three"
cargo run --bin client -- --cmd "SADD b two four"
cargo run --bin client -- --cmd "SUNION a b"
cargo run --bin client -- --cmd "SDIFF a b"
```

命令会返回 RESP 解码后的结果。

项目测试覆盖了：

- DB 层数据操作
- 命令分发和响应格式
- RESP 编码和解码
- RDB、AOF 和混合持久化恢复
- 事务入队、执行、取消和预校验
- 配置文件解析
- 日志级别解析和服务端日志参数解析
- 空闲连接超时配置、参数解析和读取超时逻辑
- key 过期逻辑
- Set 集合运算逻辑
- 压力测试工具的命令生成、pipeline 执行和统计逻辑

当前运行测试：

```powershell
cargo test
```

结果为：

```text
49 个 lib 测试通过，5 个 server 参数/空闲超时测试通过，9 个 stress 工具测试通过
```

## 总结

这个项目让我把 Redis 的几个核心机制串了起来：协议解析、网络通信、命令执行、内存数据结构、过期策略、事务和持久化。相比只看 Redis 文档，自己实现一遍会更容易理解很多命令背后的细节。

Rust 在这个项目中也很适合发挥优势。用 enum 建模多种 value 类型，用模式匹配处理类型分支，用 `Arc<RwLock<_>>` 管理共享状态，这些都让代码结构比较清晰。

后续我计划继续完善这些能力：

- 支持 `EXPIRE`、`TTL`、`PERSIST`
- 支持 `INCR`、`DECR`、`INCRBY`
- 支持 `MGET`、`MSET`
- 支持 `LINDEX`、`LSET`、`LTRIM`
- 支持 `HMGET`、`HINCRBY`
- 增加 manifest 文件，进一步提高 RDB + AOF 混合持久化的崩溃恢复能力
- 实现 AOF rewrite，减少 AOF 文件体积
- 进一步完善错误类型和命令兼容性

整体来看，`my-redis` 是一个非常适合作为 Rust 后端练手的项目。它规模不大，但包含了后端系统中很多关键问题：协议、并发、状态管理、数据结构和持久化。继续扩展下去，也可以逐步接近一个更完整的 Redis 学习版实现。
