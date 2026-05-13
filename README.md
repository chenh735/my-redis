# my-redis

`my-redis` 是一个用 Rust 和 Tokio 实现的简化版 Redis 服务端，用来练习 RESP 协议解析、TCP 客户端/服务端通信、并发共享状态和基础 Redis 命令语义。

当前实现是内存型 key-value 存储，数据不会持久化到磁盘。

## 功能

- Tokio TCP 服务端，默认监听 `127.0.0.1:6379`
- 简单命令行客户端 `client`
- RESP 请求编码、请求解析和响应解析
- 并发共享数据库：`Arc<RwLock<HashMap<String, Entry>>>`
- key 过期时间：支持 `SET key value EX seconds` 和 `SET key value PX milliseconds`
- 惰性过期删除：`GET` / `EXISTS` / `DEL` 访问 key 时发现过期会立即清理
- 后台定时清理：server 启动后每 1 秒扫描并删除过期 key

## 支持的命令

### PING

```powershell
cargo run --bin client -- --cmd "PING"
cargo run --bin client -- --cmd "PING hello"
```

返回：

```text
Success response: PONG
Success response: hello
```

### ECHO

```powershell
cargo run --bin client -- --cmd "ECHO hello"
```

返回：

```text
Success response: hello
```

### SET

```powershell
cargo run --bin client -- --cmd "SET name redis"
```

返回：

```text
Success response: OK
```

设置秒级过期时间：

```powershell
cargo run --bin client -- --cmd "SET temp redis EX 10"
```

设置毫秒级过期时间：

```powershell
cargo run --bin client -- --cmd "SET temp redis PX 500"
```

### GET

```powershell
cargo run --bin client -- --cmd "GET name"
```

存在时返回 bulk string：

```text
Success response: redis
```

key 不存在或已经过期时，服务端返回 RESP Null Bulk String：

```text
$-1\r\n
```

客户端显示为：

```text
Success response: nil
```

### EXISTS

```powershell
cargo run --bin client -- --cmd "EXISTS name missing"
```

返回存在且未过期的 key 数量：

```text
Success response: 1
```

### DEL

```powershell
cargo run --bin client -- --cmd "DEL name"
```

返回实际删除的 key 数量：

```text
Success response: 1
```

过期 key 不计入删除数量。

## 运行

启动服务端：

```powershell
cargo run -- --addr 127.0.0.1 --port 6379
```

另开一个终端运行客户端：

```powershell
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "SET name redis"
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "GET name"
```

如果 6379 端口被占用，可以换端口：

```powershell
cargo run --bin server -- --port 6380
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "PING"
```

## 测试

运行编译检查：

```powershell
cargo check
```

运行单元测试：

```powershell
cargo test
```

只运行命令分发模块测试：

```powershell
cargo test --lib cmd::cmd
```

只运行 RESP 编解码模块测试：

```powershell
cargo test --lib resp::resp
```

当前单元测试覆盖：

- 未过期 key 可以读取
- 过期 key 会返回 `None` 并被清理
- `EXISTS` 可以统计多个 key
- `DEL` 只统计实际存在且未过期的 key
- `SET key value EX seconds` 支持大写 `EX` 并能正确过期
- `PING message` 返回传入的 message
- `DEL` 命令会真正删除 key
- RESP 请求编码覆盖空数组和空 bulk string
- RESP bulk string 编码按字节长度处理非 ASCII 文本
- RESP 请求解析覆盖空数组、空 bulk string、非法协议和 bulk string 缺少 `\r\n` 的边界
- RESP 响应解析覆盖空 bulk string、Null Bulk String、非法协议和 bulk string 缺少 `\r\n` 的边界

手动验证过期时间：

```powershell
cargo run --bin server -- --port 6380
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "SET temp redis PX 500"
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "GET temp"
Start-Sleep -Milliseconds 600
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "GET temp"
```

最后一次 `GET temp` 应该显示：

```text
Success response: nil
```

手动验证后台定时清理：

```powershell
cargo run --bin server -- --port 6380
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "SET temp redis PX 500"
Start-Sleep -Seconds 2
cargo run --bin client -- --addr 127.0.0.1:6380 --cmd "GET temp"
```

后台任务每 1 秒清理一次过期 key，最后一次 `GET temp` 应该显示：

```text
Success response: nil
```

## 项目结构

```text
my-redis
├── Cargo.toml
├── README.md
└── src
    ├── lib.rs
    ├── bin
    │   ├── client.rs    # 简单命令行客户端
    │   └── server.rs    # 服务端入口和命令分发
    ├── db
    │   ├── mod.rs
    │   └── db.rs        # 内存数据库、过期时间和 key 操作
    ├── cmd
    │   ├── mod.rs
    │   └── cmd.rs       # 命令分发和命令语义
    └── resp
        ├── mod.rs
        └── resp.rs      # RESP 编码和解码
```

## 实现说明

数据库不再直接存储 `HashMap<String, String>`，而是存储带元信息的 `Entry`：

```rust
struct Entry {
    value: String,
    expires_at: Option<Instant>,
}
```

`expires_at = None` 表示永不过期，`Some(Instant)` 表示到达该时间后过期。

过期的组合策略：

- `GET` 发现 key 过期时删除并返回 nil
- `EXISTS` 发现 key 过期时删除且不计数
- `DEL` 删除过期 key 时不计入删除数量
- `src/bin/server.rs` 在创建 `Db` 后调用 `db.start_clean_up_keys()` 启动后台清理任务
- `Db::start_clean_up_keys` 内部使用 `tokio::spawn` 运行后台循环，不会阻塞 TCP accept 循环
- 后台清理任务每 1 秒获取 `RwLock` 写锁，并用 `HashMap::retain` 删除已经过期的 entry

命令处理流程：

- `src/bin/server.rs` 负责监听 TCP 连接、解析 RESP 请求并写回响应
- `src/cmd/cmd.rs` 负责命令分发和 `PING`、`ECHO`、`SET`、`GET`、`EXISTS`、`DEL` 的语义
- `src/bin/client.rs` 负责把命令行输入编码成 RESP 请求并解析服务端响应

## 后续可以优化

- 支持更多 Redis `SET` 参数，例如 `NX`、`XX`、`KEEPTTL`
- 支持 `EXPIRE`、`TTL`、`PERSIST` 等过期时间相关命令
- 客户端目前用空白字符拆分命令，暂不适合发送包含空格的单个参数
