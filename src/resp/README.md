# resp

这里实现 RESP 协议编解码。

- `resp.rs`：负责解析客户端请求、解析服务端响应，以及生成 Simple String、Error、Integer、Bulk String、Array 等 RESP 响应。
- `mod.rs`：开放 RESP 模块。

服务端使用 `decode_request` 从 TCP 流中读取命令数组，客户端和压测工具使用 `encode_request` 生成 RESP 请求。

网络读取错误会返回给调用方处理，避免客户端断开连接时触发 panic。
