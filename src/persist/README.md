# persist

这里实现持久化能力。

- `aof.rs`：AOF 增量日志写入、刷盘、命令重放，以及写命令识别。
- `rdb.rs`：RDB 快照保存、加载，以及 RDB + AOF 混合快照流程。
- `parse.rs`：解析 AOF 中保存的 RESP Array 命令。
- `mod.rs`：开放持久化模块，并重新导出 AOF/RDB 常用 API。

服务端启动时会先加载 RDB 快照，再重放 AOF 增量日志。写命令执行成功后会追加到 AOF。
