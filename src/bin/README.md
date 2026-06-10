# bin

这里放项目的可执行入口。

- `server.rs`：Redis 服务端入口，负责读取配置、监听 TCP 端口、接收连接、分发命令，并协调 AOF/RDB 持久化。
- `client.rs`：简单命令行客户端，支持单条命令模式和交互式多命令模式。
- `stress.rs`：压力测试工具，通过真实 TCP + RESP 链路压测服务端，支持多连接并发、pipeline 和多种 workload。

常用命令：

```powershell
cargo run --bin server -- --addr 127.0.0.1 --port 6379
cargo run --bin server -- --log --log-level debug
cargo run --bin server -- --idle-timeout-seconds 60
cargo run --bin client -- --cmd "PING"
cargo run --bin client -- --cmd "help"
cargo run --bin client
cargo run --bin stress -- --addr 127.0.0.1:6379 --clients 50 --requests 10000 --pipeline 10 --workload ping
```

交互式客户端启动后会复用同一个 TCP 连接，输入 `help` 查看支持的命令，输入 `exit` 断开连接。
