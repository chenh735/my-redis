# db

这里实现内存数据库和数据结构操作。

- `db.rs`：定义 `Db`、`Value`、`Entry` 和 `DbError`，使用 `Arc<RwLock<HashMap<String, Entry>>>` 保存共享状态。
- `mod.rs`：开放数据库模块，并重新导出常用 DB 类型。

当前支持的数据类型：

- String
- List
- Set
- Hash

DB 层同时负责 key 过期判断、惰性删除和后台清理过期 key。
