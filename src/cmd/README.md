# cmd

这里实现 Redis 命令层。

- `cmd.rs`：负责命令参数校验、命令分发和具体命令处理，例如 `PING`、`SET`、`GET`、List、Set、Hash、`BGSAVE` 等。
- `mod.rs`：开放命令模块，并重新导出常用命令层 API。

服务端收到 RESP 请求后，会将参数数组传给命令分发层：

```rust
dispatch(db, args).await
```

事务模块也会复用 `validate_command`，在命令入队阶段提前发现语法错误。
