# 压力测试实现 Todo

## 实现思路
- 新增独立二进制 `stress`，通过 TCP 连接真实服务端，复用现有 RESP 编码/解码逻辑。
- 支持多 client 并发、总请求数、pipeline 批量发送、key 空间、value 大小和 workload 参数。
- 默认使用 `PING` 做低副作用压测，也支持 `SET`、`GET`、`MIXED` 覆盖读写路径。
- 汇总输出总请求数、成功数、失败数、耗时、QPS、平均/最小/最大耗时和 P50/P95/P99。
- 尽量不改服务端、数据库、命令分发等已有功能代码。

## Todo
- ✅ 新增 `src/bin/stress.rs` 压测入口。
- ✅ 实现命令行参数解析。
- ✅ 实现请求分配、请求生成和 pipeline 执行。
- ✅ 实现压测结果聚合和统计输出。
- ✅ 补充压测工具相关单元测试。
- ✅ 运行格式化、测试和构建验证。
- ✅ 复查本清单，确认功能均已实现。

## 高级功能压测扩展 Todo
- ✅ 新增 List / Set / Hash / Advanced workload。
- ✅ 覆盖 List 命令：LPUSH、RPUSH、LPOP、RPOP、LLEN、LRANGE。
- ✅ 覆盖 Set 命令：SADD、SREM、SISMEMBER、SCARD、SMEMBERS、SINTER、SUNION、SDIFF。
- ✅ 覆盖 Hash 命令：HSET、HGET、HDEL、HEXISTS、HLEN、HKEYS、HVALUES、HGETALL。
- ✅ 覆盖常用基础命令：PING、ECHO、SET、GET、STRLEN、APPEND、DEL、EXISTS。
- ✅ 补充命令生成单元测试。
- ✅ 运行格式化、测试、构建和端到端验证。
