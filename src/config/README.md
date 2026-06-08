# config

这里实现服务端配置解析。

- `config.rs`：定义 `ServerConfig`，负责从 `redis.conf` 读取监听地址、端口、RDB/AOF 文件路径和定时任务间隔。
- `mod.rs`：开放配置模块，并重新导出 `ServerConfig`。

使用方式：

```rust
let config = ServerConfig::load_or_default("redis.conf").await?;
```

命令行参数可以覆盖配置文件中的地址和端口。
