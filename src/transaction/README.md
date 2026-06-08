# transaction

这里实现简化版 Redis 事务。

- `transaction.rs`：定义 `Transaction` 和 `TransactionPersistence`，支持 `MULTI`、`EXEC`、`DISCARD`，并处理事务队列、dirty 状态和事务中的持久化副作用。
- `mod.rs`：开放事务模块，并重新导出事务类型。

事务状态绑定在单个 TCP 连接上。服务端为每个连接创建一个独立的 `Transaction`，普通命令在 `MULTI` 后进入队列，直到 `EXEC` 时统一执行。
