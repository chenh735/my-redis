# logger

这里实现项目的轻量日志功能。

- `logger.rs`：定义服务端可选日志初始化逻辑和命令行日志级别枚举。
- `mod.rs`：开放日志模块，并重新导出常用日志 API。

服务端默认不启用日志。启动时可以通过命令行开启：

```powershell
cargo run --bin server -- --log --log-level debug
```
