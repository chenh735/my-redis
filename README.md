# my-redis

`my-redis` 是一个使用 Rust 和 Tokio 编写的简易 Redis 实现，支持 RESP 协议、TCP 服务端/客户端、AOF 持久化、key 过期清理，以及多种 Redis 数据结构。

## 功能特性

- 基于 Tokio 的 TCP 服务端，默认监听 `127.0.0.1:6379`
- 支持 RESP 请求解析和响应编码
- 使用 `Arc<RwLock<HashMap<String, Entry>>>` 保存共享数据库状态
- 支持 key 过期时间：`SET key value EX seconds` / `SET key value PX milliseconds`
- 支持后台定时清理过期 key
- 支持 AOF 持久化，写命令会追加到 `appendonly.aof`
- 服务启动时会自动加载 `appendonly.aof` 恢复数据
- 支持 String、List、Set、Hash 四类数据结构

## 支持的数据结构和命令

### 通用命令

| 命令                     | 说明              |
|------------------------|-----------------|
| `PING [message]`       | 测试连接，带参数时返回参数内容 |
| `ECHO message`         | 返回输入内容          |
| `DEL key [key ...]`    | 删除一个或多个 key     |
| `EXISTS key [key ...]` | 返回存在的 key 数量    |

### String

| 命令                                 | 说明                   |
|------------------------------------|----------------------|
| `STRSET key value`                 | 设置字符串值               |
| `STRSET key value EX seconds`      | 设置字符串值，并以秒为单位设置过期时间  |
| `STRSET key value PX milliseconds` | 设置字符串值，并以毫秒为单位设置过期时间 |
| `STRGET key`                       | 获取字符串值               |
| `STRLEN key`                       | 获取字符串长度              |
| `APPEND key value`                 | 追加字符串，返回新长度          |           


示例：

```powershell
cargo run --bin client -- --cmd "SET name redis"
cargo run --bin client -- --cmd "GET name"
cargo run --bin client -- --cmd "SET temp redis EX 10"
```

### List

List 使用 `VecDeque<String>` 保存，支持从两端插入和弹出。

| 命令                            | 说明                  |
|-------------------------------|---------------------|
| `LPUSH key value [value ...]` | 从列表左侧插入一个或多个元素      |
| `RPUSH key value [value ...]` | 从列表右侧插入一个或多个元素      |
| `LPOP key`                    | 从列表左侧弹出一个元素         |
| `RPOP key`                    | 从列表右侧弹出一个元素         |
| `LLEN key`                    | 返回列表长度              |
| `LRANGE key start stop`       | 返回指定范围内的列表元素，支持负数下标 |

示例：

```powershell
cargo run --bin client -- --cmd "LPUSH letters a b c"
cargo run --bin client -- --cmd "LRANGE letters 0 -1"
cargo run --bin client -- --cmd "RPOP letters"
```

`LPUSH letters a b c` 后，列表内容为 `c b a`，这和 Redis 的插入顺序一致。

### Set

Set 使用 `HashSet<String>` 保存，成员不重复。为了测试结果稳定，`SMEMBERS` 的返回值会按字典序排序。

| 命令                              | 说明                          |
|---------------------------------|-----------------------------|
| `SADD key member [member ...]`  | 添加一个或多个成员，返回新增成员数量          |
| `SREM key member [member ...]`  | 删除一个或多个成员，返回成功删除数量          |
| `SISMEMBER key member`          | 判断成员是否存在，存在返回 `1`，不存在返回 `0` |
| `SCARD key`                     | 返回集合成员数量                    |
| `SMEMBERS key`                  | 返回集合全部成员                    |
| `SINTER key [key ...]`          | 返回多个集合的交集                   |
| `SUNION key [key ...]`          | 返回多个集合的并集                   |
| `SDIFF key [key ...]`           | 返回第一个集合和后续集合的差集             |

示例：

```powershell
cargo run --bin client -- --cmd "SADD tags rust db rust"
cargo run --bin client -- --cmd "SISMEMBER tags rust"
cargo run --bin client -- --cmd "SMEMBERS tags"
```

### Hash

Hash 使用 `HashMap<String, String>` 保存字段和值。

| 命令                                       | 说明                          |
|------------------------------------------|-----------------------------|
| `HSET key field value [field value ...]` | 设置一个或多个字段，返回新增字段数量          |
| `HGET key field`                         | 获取字段值                       |
| `HDEL key field [field ...]`             | 删除一个或多个字段，返回成功删除数量          |
| `HEXISTS key field`                      | 判断字段是否存在，存在返回 `1`，不存在返回 `0` |
| `HGETALL key`                            | 返回所有字段和值，按字段名排序后扁平化返回       |
| `HLEN key`                               | 字段数量                        |
| `HKEYS key`                              | 所有 field                    |
| `HVALS key`                              | 所有 value                    |

示例：

```powershell
cargo run --bin client -- --cmd "HSET user name chen age 18"
cargo run --bin client -- --cmd "HGET user name"
cargo run --bin client -- --cmd "HGETALL user"
```

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

## AOF 持久化

服务端会把成功执行的写命令追加到 `appendonly.aof`。当前支持持久化的写命令包括：

- `SET`
- `DEL`
- `LPUSH`
- `RPUSH`
- `LPOP`
- `RPOP`
- `SADD`
- `SREM`
- `HSET`
- `HDEL`

AOF 文件使用 RESP Array 格式保存命令。例如：

```text
SET name redis
```

会保存为：

```text
*3\r\n$3\r\nSET\r\n$4\r\nname\r\n$5\r\nredis\r\n
```

读命令不会写入 AOF，例如 `GET`、`EXISTS`、`LRANGE`、`SMEMBERS`、`HGETALL`。

## 运行服务端

默认运行：

```powershell
cargo run
```

指定端口：

```powershell
cargo run --bin server -- --port 6380
```

指定地址和端口：

```powershell
cargo run --bin server -- --addr 127.0.0.1 --port 6380
```

## 使用客户端

```powershell
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "PING"
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "SET name redis"
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "GET name"
```

如果使用默认地址 `127.0.0.1:6379`，可以省略 `--addr`：

```powershell
cargo run --bin client -- --cmd "PING"
```

## 测试

运行全部测试：

```powershell
cargo test
```

只运行命令层测试：

```powershell
cargo test --lib cmd::cmd
```

只运行数据库层测试：

```powershell
cargo test --lib db::db
```

只运行 RESP 测试：

```powershell
cargo test --lib resp::resp
```

## 项目结构

```text
my-redis
├── Cargo.toml
├── README.md
├── appendonly.aof
└── src
    ├── lib.rs
    ├── bin
    │   ├── client.rs
    │   └── server.rs
    ├── cmd
    │   ├── mod.rs
    │   └── cmd.rs
    ├── db
    │   ├── mod.rs
    │   └── db.rs
    ├── persist
    │   ├── mod.rs
    │   ├── parse.rs
    │   └── persistence.rs
    └── resp
        ├── mod.rs
        └── resp.rs
```
