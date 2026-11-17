# Transport 接收半边拆分设计（PacketReceiver）

> 状态：设计稿，未实施
> 前置提交：`fd1ddec`（删除 `Transport::send_command`、CAN 接收队列改为 FIFO channel）、`7458571`（去掉 CAN 接收队列上的锁）

## 1. 问题

### 1.1 现象

`Transport` 把「发」和「收」放在同一个 `&self` 对象里：

```rust
// src/transport/transport.rs:131
pub trait Transport: Send + Sync {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError>;

    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError>;          // :136
    async fn try_recv_packet(&self, timeout_ms: u64) -> Result<Option<Vec<u8>>, TransportError>;
    async fn recv_packet_ts(&self) -> Result<(Vec<u8>, Option<u64>), TransportError>;
    async fn try_recv_packet_ts(&self, timeout_ms: u64) -> Result<Option<(Vec<u8>, Option<u64>)>, TransportError>;

    async fn close(&self) -> Result<(), TransportError>;                     // :161
    fn transport_type(&self) -> TransportType;
    fn target_addr(&self) -> String;
}
```

接收操作本质上是**有状态的、只能有一个消费者的**：`recv` 要么需要 `&mut self`，要么需要内部可变性。而 trait 要求 `&self` + `Sync`，于是每个 transport 都得自己想办法把 `&self` 变出 `&mut`：

| Transport | 现状的「内部可变性」装置 | 位置 |
|---|---|---|
| CAN | `async_channel::Receiver`（`recv(&self)`，天然无锁） | `src/transport/can/mod.rs:343` |
| UDP | `pending_packets: Mutex<VecDeque<Vec<u8>>>` + `tokio::sync::Mutex` 包 socket 缓冲 | `src/transport/udp.rs:137` |
| TCP | `reader_state: Mutex<TcpReadState>`（内含 `OwnedReadHalf` / 缓冲 / 计数器跟踪） | `src/transport/tcp.rs:71` |

这些锁**都不是并发保护**。全仓库只有一个消费者：

```
src/transport/message_router.rs:135   transport.recv_packet_ts().await   ← 唯一的 recv 调用方
src/slave.rs:252-259                  router_clone.start(transport_for_router)   ← 后台任务持有
src/slave.rs:343                      self.transport.send_command_only(...)      ← 只发不收
```

它们只是「`&mut`-from-`&self`」的转换装置，代价是每个 transport 各自实现一套、类型上多一层包裹、以及 `RxQueue` 这类新类型不得不为锁而存在。

### 1.2 为什么不能简单改成 `&mut self`

`transport` 是 `Arc<dyn Transport>`（`src/transport/transport.rs:165` 工厂返回、`src/slave.rs:174` 持有），并被 clone 进后台 router 任务。`&mut self` 拿不到独占引用，唯一出路是 `Arc<Mutex<dyn Transport>>`，于是：

```
router 任务: loop { let t = arc.lock().await; t.recv_packet_ts().await; }   ← 跨无限期 await 持锁
slave 任务:  arc.lock().await → 永远等不到 → send_command_only 死锁
```

而且必然在连接阶段就炸：`src/slave.rs:290` 启动 router 后立刻 `send_connect()`。所以 `&mut self` 不是「多改几行」，是**否掉了「后台持续收 + 前台随时发」这个模型**。

### 1.3 根因

接收端的**所有权放错了位置**：receiver 天生只该有一个 owner，却被塞进一个必须 `Sync` 的共享对象，只好用锁把 `&mut` 造出来。正确做法不是「选一个更便宜的锁」，而是**把接收半边搬出去，交给它唯一的消费者按值持有**。

## 2. 目标与非目标

### 2.1 目标

1. 接收端由唯一消费者**按值持有**，`recv` 是 `&mut self` —— 类型系统保证「单消费者」不变量，不再靠注释。
2. 接收路径上**零锁**：CAN 的 channel、UDP 的 pending 队列、TCP 的读缓冲都不再需要 `Mutex`。
3. 收发分离后 `Transport` 只剩发送语义，`Arc<dyn Transport>` 继续成立，`send_command_only(&self)` 行为不变。
4. 可回退 `async-channel` 依赖（CAN 改回 `tokio::sync::mpsc`）。
5. 语义零变化：超时、断连、PID 分类归属、DAQ 时间戳透传、错误映射全部保持。

### 2.2 非目标

- 不改 `MessageRouter` 的 CTO/DTO 分流逻辑与 PID 分类规则（PID 分类**永远**属于 router，不属于 transport）。
- 不改 `XcpSlave` 的命令流程（`send_command_only` + router 的 CTO channel）。
- 不改协议层（UDP 的 LEN/COUNTER 头、TCP 的流式拆帧、CAN 的 DLC 语义）。
- 不引入新的第三方依赖；目标是**减少**一个。

## 3. 目标设计

### 3.1 Trait 定义

```rust
// src/transport/transport.rs

/// 一个已归一化的 XCP 包：载荷 + 可选的设备接收时间戳（微秒）。
pub type ReceivedPacket = (Vec<u8>, Option<u64>);

/// 传输层的接收半边。
///
/// 按值由**唯一**消费者持有（当前是 `MessageRouter`），因此 `recv` 取
/// `&mut self`：单消费者不变量由所有权保证，不需要任何锁，也不需要 `Sync`。
#[async_trait]
pub trait PacketReceiver: Send + 'static {
    /// 阻塞接收下一个包；连接关闭时返回 `ConnectionClosed`。
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError>;

    /// 带超时接收；超时返回 `Ok(None)`。
    async fn try_recv(&mut self, timeout_ms: u64) -> Result<Option<ReceivedPacket>, TransportError>;
}

/// 传输层的发送半边（+ 生命周期管理）。
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError>;

    /// 取走接收半边。只能成功一次，之后返回 `None`。
    ///
    /// 非 async：实现内部用 `std::sync::Mutex<Option<..>>` 一次性 take，
    /// 不跨 `.await`，因此不会与发送路径竞争。
    fn take_receiver(&self) -> Option<Box<dyn PacketReceiver>>;

    async fn close(&self) -> Result<(), TransportError>;
    fn transport_type(&self) -> TransportType;
    fn target_addr(&self) -> String;
}
```

要点：

- `PacketReceiver: Send + 'static`，**不要 `Sync`** —— 这是整个改造的核心收益，`Sync` 正是 `&self` 接收被迫加锁的原因。
- `recv` 返回 `ReceivedPacket` 而不是 `Vec<u8>`：删掉 `recv_packet` / `recv_packet_ts` / `try_recv_packet` / `try_recv_packet_ts` 四个方法（它们只是「有没有时间戳」的两个维度，合并成一个）。
- `take_receiver` 返回 `Option`：语义是「一次性移交」。第二次调用返回 `None`，由调用方决定怎么报错（当前唯一调用点是 `XcpSlave::connect`，只调一次）。
- `close()` 留在 `Transport` 上：它管理连接生命周期，且发送半边仍可能需要它（TCP 的 `shutdown`）。

### 3.2 所有权与时序

```
                      ┌───────────────────────────────┐
TransportFactory ───► │ Arc<dyn Transport>            │  ← XcpSlave 持有，只用于发送/关闭
                      │  ├─ UdpTransport { socket: Arc<UdpSocket>, counter, ... }
                      │  ├─ TcpTransport { writer: Mutex<OwnedWriteHalf>, ... }
                      │  └─ CanTransport { device, reader_stop, ... }
                      │  └─ rx_slot: Mutex<Option<Box<dyn PacketReceiver>>>   ← 一次性移交
                      └───────────────┬───────────────┘
                                      │ take_receiver()  (仅一次)
                                      ▼
                      Box<dyn PacketReceiver>   ← 按值移交，独占
                                      │
                                      ▼
                      MessageRouter 的后台任务（recv(&mut self)）
```

连接时序（`XcpSlave::connect`）：

```
TransportFactory::create()          → Arc<dyn Transport>（rx 暂存在 rx_slot）
let rx = transport.take_receiver()  → 取出接收半边（此后 transport 只能发/关）
router.start(rx)                    → 移交后台任务，循环 rx.recv().await
send_connect()                      → 走 send_command_only + router 的 CTO channel
```

断开时序（`XcpSlave::disconnect`）不变：`router.stop()`（内部 `handle.abort()`，任务被丢弃时 `Box<dyn PacketReceiver>` 随之 drop）→ `transport.close()`。

### 3.3 各 Transport 的接收半边

#### CAN

现状：reader 线程 → `async_channel` → `RxQueue`（`src/transport/can/mod.rs:343`）。

改造后：

```rust
/// reader 线程投递过来的帧，按到达顺序。
struct CanRx(mpsc::UnboundedReceiver<CanFrame>);   // 可改回 tokio::sync::mpsc

#[async_trait]
impl PacketReceiver for CanRx {
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError> {
        self.0.recv().await
            .map(|f| (f.data, f.timestamp_us))
            .ok_or(TransportError::ConnectionClosed)
    }

    async fn try_recv(&mut self, timeout_ms: u64) -> Result<Option<ReceivedPacket>, TransportError> {
        match tokio::time::timeout(Duration::from_millis(timeout_ms), self.0.recv()).await {
            Ok(Some(f)) => Ok(Some((f.data, f.timestamp_us))),
            Ok(None) => Err(TransportError::ConnectionClosed),
            Err(_) => Ok(None),
        }
    }
}
```

- `CanTransport` 不再有 `rx` 字段，改为 `rx_slot: std::sync::Mutex<Option<Box<dyn PacketReceiver>>>`。
- reader 线程不变（`Sender<CanFrame>` + `try_send`）。
- **`async-channel` 依赖可以撤掉**，改回 `tokio::sync::mpsc`（`recv` 需要 `&mut self`，此时完全合适）。

#### UDP

现状：`pending_packets: Mutex<VecDeque<Vec<u8>>>`（`udp.rs:137`）+ 计数器跟踪 `Mutex<ReceivedCounterTracker>`，都在接收路径上。

改造后：

```rust
struct UdpRx {
    socket: Arc<UdpSocket>,              // tokio UdpSocket 的 recv/send 都是 &self → Arc 即可，无需锁
    pending: VecDeque<Vec<u8>>,          // 一次 datagram 解出的多余 payload
    counter_tracker: ReceivedCounterTracker,
}
```

- `UdpTransport` 持有 `socket: Arc<UdpSocket>`（供 `send_command_only`）+ `counter: AtomicU16` + `rx_slot`。
- `decode_datagram_payloads` / `receive_payload_from_socket` / `recv_payload_packet` 整体搬进 `UdpRx`，`&self` → `&mut self`，两个 `Mutex` 直接删除。
- 断连语义不变：UDP 无连接，`recv` 只可能返回超时。

#### TCP

现状：`reader_state: Mutex<TcpReadState>`（`tcp.rs:71`），内含 `OwnedReadHalf` + 字节缓冲 + `pending_packets` + 计数器跟踪。

改造后：

```rust
struct TcpRx {
    reader: OwnedReadHalf,
    buffer: Vec<u8>,
    pending: VecDeque<Vec<u8>>,
    counter_tracker: ReceivedCounterTracker,
}
```

- `TcpReadState` 这个名字消失，直接就是 `TcpRx`；`extract_buffered_packets` 变成 `TcpRx::extract_buffered(&mut self)`。
- `TcpTransport` 只留 `writer: Mutex<OwnedWriteHalf>`（这个 `Mutex` 是**真需要**的：`&mut writer` 跨 `.await`，且可能有多个发送者）。
- `read() == 0` → `ConnectionClosed`，语义不变。

#### 工厂

`TransportFactory::create` 返回值不变（`Arc<dyn Transport>`），因为接收半边暂存在 `rx_slot` 里随 transport 一起返回。**调用方只需多一步 `take_receiver()`**，工厂签名不用改 —— 这是本方案对现有代码侵入最小的关键点。

## 4. 调用方改动

| 文件 | 现状 | 改动 |
|---|---|---|
| `src/transport/transport.rs` | trait 含 4 个 recv 方法 | 删除 4 个 recv 方法，新增 `PacketReceiver` trait + `take_receiver` |
| `src/transport/message_router.rs:118` | `start(&self, transport: Arc<dyn Transport>)` | `start(&self, rx: Box<dyn PacketReceiver>)`；循环内 `rx.recv().await`（`&mut`，因为 rx 由任务按值持有） |
| `src/slave.rs:252-259` | `transport.clone()` 给 router | 先 `transport.take_receiver().ok_or(...)?`，再把 `Box<dyn PacketReceiver>` move 进 `start` |
| `src/slave.rs` 其余 | `send_command_only` / `close` / `transport_type` | 不变 |
| `src/transport/{udp,tcp,can}` | 各自实现 4 个 recv 方法 | 删除；新增各自的 `*Rx` 类型实现 `PacketReceiver` |
| `src/transport/tcp.rs` 测试 | 直接调 `transport.recv_packet()`（`:373-391`） | 改为 `rx.recv()` / `rx.try_recv(10)`，并显式 `take_receiver()` |
| `docs/ARCHITECTURE_DIAGRAM.md` | 列了 `recv_packet()` | 更新为 `PacketReceiver` / `recv()` |

`XcpSlave` 结构体**不需要**新增字段：接收半边在 `connect` 里一次性移交后就不再需要。

## 5. 语义与边界

### 5.1 超时/断连映射（必须与现状一致）

| 情况 | 现状 | 改造后 |
|---|---|---|
| 超时（有 timeout 参数） | `Ok(None)` | `try_recv` → `Ok(None)` |
| 无超时等待中断连 | `Err(ConnectionClosed)` | `recv` → `Err(ConnectionClosed)` |
| CAN reader 线程退出 | channel 关闭 → `ConnectionClosed` | 同（channel 随 `CanRx` 一起 drop） |
| TCP 对端关闭（`read()==0`） | `ConnectionClosed` | 同 |
| 底层 IO 错误 | `RecvFailed` | 同 |
| 路由任务被 abort | `Box<dyn PacketReceiver>` drop | 同（CAN 侧 reader 的 `try_send` 失败 → 线程退出） |

### 5.2 边界条件

- **`take_receiver` 只能成功一次**：第二次返回 `None`。`XcpSlave::connect` 用 `ok_or(SlaveError::...)` 直接失败，避免静默地「router 永远收不到包」。
- **`close()` 与接收半边的关系**：CAN 的 `close()` 会 `join` reader 线程（`can/mod.rs` 的 `close`/`Drop`），此时 `CanRx` 若仍被 router 持有，channel 关闭后 `recv` 返回 `ConnectionClosed`，任务自然退出 —— 与现状一致。TCP 的 `close()` 只 shutdown 写半，读半由对端 FIN 或 abort 结束。
- **多 slave**：每个 `XcpSlave` 一个 transport、一个 router、一个 `PacketReceiver`，互不共享，天然无锁。
- **不做 `try_lock` / `Option<Mutex>` 之类的临时方案**：`rx_slot` 里的 `std::sync::Mutex` 只做一次性 take，**绝不跨 `.await`**，这是它可以安全用 std 版本的原因（`std::sync::MutexGuard` 非 `Send`）。

## 6. 依赖与回退

- 新增：无。
- 移除：`async-channel = "=2.5.0"` 及其 4 个传递依赖（`concurrent-queue` / `event-listener` / `event-listener-strategy` / `parking`）—— CAN 的 `recv` 变成 `&mut self` 后，`tokio::sync::mpsc` 就够用。
- 提交 `7458571` 中为 CAN 引入的 `async_channel` 可整体回退，属于本方案的一个子步骤。

## 7. 测试计划

### 7.1 单测（无硬件）

| 用例 | 覆盖点 |
|---|---|
| `CanRx::recv` 保序 + 时间戳透传 | FIFO 语义（迁移现有 `test_rx_queue_preserves_arrival_order`） |
| `CanRx` 超时 / sender drop | `Ok(None)` vs `ConnectionClosed`（迁移 `test_rx_queue_timeout_and_close`） |
| `take_receiver` 第二次返回 `None` | 一次性移交不变量 |
| `UdpRx` 一个 datagram 多个 payload 的排队顺序 | `pending` 队列搬进 `&mut self` 后仍正确 |
| `TcpRx` 半包/粘包拆帧 | 迁移现有 `test_recv_packet_buffers_multiple_frames_from_stream`、`test_try_recv_packet_times_out` |
| `TcpRx` 对端关闭 → `ConnectionClosed` | 迁移/新增 |

### 7.2 集成（现有 mock slave）

- `tests/mock_xcp_integration.rs`：`send_command_and_wait_skips_unsolicited_ev_and_serv_packets` 必须保持通过（它验证的是 router 的 PID 分类，不涉及本改造，但会经过新的接收路径）。
- 多 slave 场景（若已有用例）：确认每个 router 独立持有自己的 `PacketReceiver`。

### 7.3 手工验证（硬件）

- CAN（PCAN / ES582）：DAQ 录制 + 标定上传，确认 DAQ 时间戳仍然透传（`daq_timestamp_reconstruction` 相关逻辑依赖 `device_timestamp_us`）。
- UDP/TCP：连接、标定读写、DAQ 录制各跑一遍。

## 8. 分步实施计划（每步可独立编译 + 提交）

| 步骤 | 内容 | 验证 |
|---|---|---|
| 1 | 新增 `PacketReceiver` trait + `ReceivedPacket` 类型别名，**不改任何实现** | `cargo build`（会有 dead_code 警告，可接受或加 `#[allow]`） |
| 2 | CAN 迁移：`RxQueue` → `CanRx` 实现 `PacketReceiver`，`CanTransport` 加 `rx_slot` + `take_receiver`，回退 `async-channel` | `cargo test transport::can` |
| 3 | TCP 迁移：`TcpReadState` → `TcpRx`，删 `reader_state` 的 `Mutex` | `cargo test transport::tcp` |
| 4 | UDP 迁移：`UdpRx`，删两个 `Mutex` | `cargo test transport::udp` |
| 5 | `Transport` trait 删除 4 个 recv 方法；`MessageRouter::start` 改为吃 `Box<dyn PacketReceiver>`；`XcpSlave::connect` 加 `take_receiver()` | `cargo test`（全量） |
| 6 | 清理：`Cargo.toml` 移除 `async-channel`，更新 `docs/ARCHITECTURE_DIAGRAM.md` 与 `Transport` trait 文档 | `cargo test` + `cargo clippy --all-targets` |

步骤 1-4 期间两套接收路径并存（旧的 trait 方法 + 新的 `PacketReceiver`），第 5 步一次性切换调用方。若想避免并存，可把 2-5 合成一个 commit —— 代价是 diff 较大、不好 bisect。**建议按上表分步，每步都保持全绿。**

## 9. 风险与对策

| 风险 | 说明 | 对策 |
|---|---|---|
| 步骤 5 是「一刀切」 | trait 与调用方必须同时改，否则编译不过 | 1-4 步先让三套实现都就绪，第 5 步只改签名与两处调用 |
| `recv` 语义漂移 | 4 个方法合并成 2 个，容易出现「超时返回 `Ok(None)` 还是 `Err`」的偏差 | 严格对照 §5.1 表格，逐项写单测 |
| TCP 读缓冲迁移遗漏 | `TcpReadState` 与 `extract_buffered_packets` 是关联函数，搬移时易漏 `drain` 逻辑 | 迁移后保留原有 3 个 TCP 单测，逐字对比 `drain(..consumed)` |
| DAQ 时间戳丢失 | `recv` 返回 `(Vec<u8>, Option<u64>)` 若被简化成 `Vec<u8>`，DAQ 时间戳重建会静默退化 | 类型别名 `ReceivedPacket` 固定二元组；`message_router` 侧保持 `device_timestamp_us` 透传，集成测试覆盖 |
| 硬件回归 | CAN/USB 设备行为无法在 CI 覆盖 | §7.3 手工验证清单，作为合入前置条件 |

## 10. 备选方案与否决理由

| 方案 | 结论 | 理由 |
|---|---|---|
| 接收方法改 `&mut self`（保留单一 trait） | ❌ 否决 | `Arc<dyn Transport>` 拿不到 `&mut`；改成 `Arc<Mutex<dyn Transport>>` 会让 router 跨无限期 `await` 持锁，`send_command_only` 死锁（§1.2） |
| 保留 `tokio::sync::Mutex<Receiver>`（当前 `7458571` 之前的状态） | ⚠️ 可用但非解 | 单消费者下永不竞争，成本可忽略；但它只是把「`&mut`-from-`&self`」藏进一层包装，问题本身没解 |
| `async-channel` / `flume`（`recv(&self)`） | ✅ 当前采用（`7458571`） | 零锁，但仍是「接收端塞在共享对象里」的思路；且 CAN 需要 `try_send` 适配。本方案完成后可移除 |
| `UnsafeCell` + `unsafe impl Sync` | ❌ 否决 | 并发 `recv` 即 UB；用不安全换取「少一层类型」，收益与风险完全不成比例 |
| 保持现状不动 | ⚠️ 可接受 | 功能无缺陷，纯粹是设计洁癖问题；本方案的价值是可维护性（类型保证不变量、删锁、删依赖） |

## 11. 验收标准

- [ ] `Transport` 上不再有任何接收方法；`PacketReceiver` 是接收的唯一入口
- [ ] 接收路径上无 `Mutex`（TCP 的 `writer` 除外，它属于发送路径）
- [ ] `Cargo.toml` 不再含 `async-channel`
- [ ] `cargo test` 全绿，且 `transport::can` / `transport::tcp` / `transport::udp` 的单测数量不少于改造前
- [ ] `cargo clippy --all-targets` 对 `src/transport/**` 无 warning
- [ ] 手工验证 §7.3 通过（CAN + UDP + TCP 各一遍）

## 12. 未决问题

1. `PacketReceiver` 是否需要 `try_recv`？当前生产代码只用 `recv_packet_ts`（阻塞版），`try_recv` 仅被 TCP 单测使用。倾向**保留**（DAQ 侧将来可能要做带超时的轮询），但若确认无用可一并删除，进一步缩小 trait。
2. `take_receiver` 失败时 `XcpSlave::connect` 报什么错？当前无对应 `SlaveError` 变体，可能需要 `SlaveError::Transport(TransportError::NotImplemented(...))` 或新增一个内部错误。
3. 是否顺势把 `Transport::close()` 也拆成 `Transport::shutdown()`（发送侧）+ `PacketReceiver` 的 `Drop`？倾向**不做**：`close()` 涉及设备资源释放（CAN 的 `join` + `device.close()`），与接收半边生命周期不强绑定。
4. `MessageRouter::stop()` 现在靠 `handle.abort()` 打断阻塞中的 `recv`。拆分后 `abort` 会在 await 点丢弃 `Box<dyn PacketReceiver>`（安全），但若将来换成「优雅停止」，需要给 `PacketReceiver` 加一个 `close()`/`CancellationToken`。当前不改。