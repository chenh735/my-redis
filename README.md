# my-redis

一个用 Rust 和 Tokio 实现的迷你 Redis 学习项目。

这个项目的目标不是完整复刻 Redis，而是通过实现一个可运行的简化版 Redis，练习 TCP 网络编程、RESP 协议解析、异步并发处理和内存键值数据库的基本设计。

## 项目简介

`my-redis` 包含一个 Redis-like 服务端和一个简单的命令行客户端：

- 服务端二进制：`my-redis`
- 客户端二进制：`client`
- 网络层基于 `tokio::net::TcpListener` 和 `tokio::net::TcpStream`
- 数据存储基于 `Arc<RwLock<HashMap<String, String>>>`
- 客户端与服务端之间使用 RESP 协议格式传输命令和响应

通过这个项目，可以比较直观地理解 Redis 的一些基础机制：客户端如何把命令编码成协议数据，服务端如何解析请求、执行命令，并把结果按协议返回给客户端。

## 已实现功能

当前项目已经实现了以下能力：

- 启动一个 TCP 服务端，默认监听 `127.0.0.1:6379`
- 支持多个客户端连接，每个连接由独立的 Tokio task 处理
- 使用内存 `HashMap` 保存字符串键值对
- 支持 RESP 数组请求解析
- 支持 RESP 响应解析：
  - Simple String
  - Error
  - Integer
  - Bulk String
- 支持以下 Redis-like 命令：
  - `PING`
  - `SET`
  - `GET`
  - `EXIST`
  - `EXISTS`
  - `DEL`
  - `ECHO`

## 技术栈

- Rust 2024 Edition
- Tokio：异步运行时与 TCP 网络编程
- Clap：命令行参数解析
- Anyhow：错误处理
- HashMap + RwLock：内存键值存储与并发读写控制

## 快速开始

### 1. 克隆项目

```powershell
git clone <your-repo-url>
cd my-redis
```

如果项目已经在本地，可以直接进入项目目录：

```powershell
cd C:\Users\chenhao\Desktop\rust\my-redis
```

### 2. 检查项目是否可以编译

```powershell
cargo check
```

### 3. 启动服务端

```powershell
cargo run --bin my-redis -- --addr 127.0.0.1 --port 6379
```

服务端启动后会监听本地 `6379` 端口。保持这个终端窗口运行，再打开一个新的终端窗口执行客户端命令。

### 4. 使用客户端发送命令

```powershell
cargo run --bin client -- --cmd "PING"
```

也可以指定服务端地址：

```powershell
cargo run --bin client -- --addr 127.0.0.1:6379 --cmd "PING"
```

## 命令示例

### PING

```powershell
cargo run --bin client -- --cmd "PING"
```

预期返回：

```text
Success response: PONG
```

### SET

```powershell
cargo run --bin client -- --cmd "SET name redis"
```

预期返回：

```text
Success response: OK
```

### GET

```powershell
cargo run --bin client -- --cmd "GET name"
```

预期返回：

```text
Success response: redis
```

如果 key 不存在，返回：

```text
Success response: nil
```

### EXIST

```powershell
cargo run --bin client -- --cmd "EXIST name"
```

预期返回：

```text
Success response: 1
```

不存在时返回：

```text
Success response: 0
```

### EXISTS

```powershell
cargo run --bin client -- --cmd "EXISTS test1 test2 test3"
```

预期返回已缓存的key的数量

```text
Success response: 3
```

### DEL

```powershell
cargo run --bin client -- --cmd "DEL name"
```

预期返回被删除的 key 数量：

```text
Success response: 1
```

### ECHO

```powshell
cargo run --bin client -- --cmd "ECHO hello my-redis"
```

预期返回ECHO后的消息

```text
Success response: hello my-redis
```

## 项目结构

```text
my-redis
├── Cargo.toml
├── Cargo.lock
└── src
    ├── main.rs          # 服务端入口，负责监听连接、解析命令和执行命令
    ├── lib.rs           # 库模块入口
    ├── bin
    │   └── client.rs    # 简单命令行客户端
    ├── db
    │   ├── mod.rs
    │   └── db.rs        # 内存数据库初始化
    └── resp
        ├── mod.rs
        └── resp.rs      # RESP 请求编码、请求解析和响应解析
```

## 核心学习点

### 1. RESP 协议

Redis 客户端和服务端之间通常通过 RESP 协议通信。本项目实现了简化版 RESP 编解码流程：

- 客户端将命令参数编码为数组格式
- 服务端解析数组请求
- 服务端根据命令返回不同类型的 RESP 响应
- 客户端再把响应解析成人类可读的字符串

例如：

```text
SET name redis
```

会被编码为类似下面的 RESP 数据：

```text
*3
$3
SET
$4
name
$5
redis
```

实际网络传输中每一行会使用 `\r\n` 结尾。

### 2. Tokio 异步网络编程

服务端使用 Tokio 监听 TCP 连接。每当有新的客户端连接进来，就通过 `tokio::spawn` 创建一个异步任务单独处理该连接。

这种方式可以让服务端同时处理多个客户端请求，是理解 Rust 异步编程模型的一个很好的练习。

### 3. 共享内存数据库

项目使用：

```rust
Arc<RwLock<HashMap<String, String>>>
```

作为共享数据库：

- `Arc` 允许多个异步任务共享同一个数据库实例
- `RwLock` 支持多读单写，避免并发访问时出现数据竞争
- `HashMap` 负责保存 key-value 数据

### 4. 命令解析与响应构造

服务端会把客户端请求解析成字符串数组，然后根据第一个参数判断命令类型：

- `SET key value`：写入数据
- `GET key`：读取数据
- `PING`：测试连接
- `ECHO hello`：测试连接
- `EXIST key`：判断 key 是否存在
- `EXISTS key1 key2 key3`：判断多个key是否存在
- `DEL key...`：删除一个或多个 key

这部分逻辑可以帮助理解 Redis 命令处理的大致流程。

## 当前限制

这是一个学习型项目，目前还没有实现完整 Redis 的很多能力：

- 数据只保存在内存中，服务端重启后会丢失
- 暂未实现 key 过期时间
- 暂未实现 RDB / AOF 持久化
- 暂未实现事务
- 暂未实现发布订阅
- 暂未实现主从复制和集群
- 暂未实现 Redis 的完整命令集和完整错误处理


## 后续计划

后面可以继续尝试实现：

- 增加 key 过期时间支持
- 增加更多字符串命令，例如 `INCR`、`DECR`、`APPEND`
- 为 RESP 编解码模块补充单元测试
- 为服务端命令处理补充集成测试
- 增加更清晰的错误类型和错误响应
- 尝试实现简单的持久化机制

## 项目定位

这个项目主要用于学习和练习：

- Rust 所有权与并发模型
- Tokio 异步编程
- TCP 客户端 / 服务端通信
- Redis 协议和命令处理流程
- 简单数据库服务的模块拆分

如果你也在学习 Rust 或 Redis 的底层原理，可以把这个项目当作一个小型练手项目，从网络层、协议层、存储层三个方向逐步扩展。
