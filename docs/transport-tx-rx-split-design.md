# Transport 收发分离架构（v2：创建即分离）

> 状态：**已实施**（2026-11，验证结果见 §14）
> 取代：`docs/transport-receiver-split-design.md`（v1，`take_receiver()` 方案）
> 前置提交：`fd1ddec`、`7458571`、`071bc53`
>
> 已确定的关键决策：
> 1. 命名 `TransportSender` / `TransportReceiver`
> 2. `ReceivedPacket` 用命名结构体（保护 `device_timestamp_us`）
> 3. DAQ 一步到位：DTO 通道按值交给 `DaqManager`，热循环零锁（§3.7）
> 4. router 在 `ConnectionClosed` 时退出；其它 IO 错误退避重试（§3.4）
> 5. `TransportError` 一并重构（§3.9）

## 0. 与 v1 的差别（一句话）

| | v1（`PacketReceiver` + `take_receiver`） | v2（本稿） |
|---|---|---|
| 收半边的产生方式 | `Arc<dyn Transport>` 先创建，再用 `take_receiver()` 从 `rx_slot: Mutex<Option<..>>` 里**取出** | `TransportFactory::open()` **直接返回两个独立对象** `TransportLink { sender, receiver }` |
| 类型上是否存在「没有收半边」的状态 | 存在（`Option`，只能成功一次，第二次返回 `None`，调用方要处理错误） | 不存在 |
| 运行时约定 | 「`take_receiver` 只能调一次」 | 无。所有权在编译期确定 |
| 发送侧类型 | `Arc<dyn Transport>`（同时装着收半边） | `Box<dyn TransportSender>`（只装发送） |
| 启动竞态 | `connect` 里 `sleep(10ms)` 等 router 起来 | 消失：receiver 在 `open()` 返回时就已存在 |
| 每 transport 的内部可变性 | 少一层锁，但仍有 `rx_slot` 的锁 | 接收路径零锁、零 `Option`、零「一次性」语义 |

v1 的进步是把「`&mut`-from-`&self`」的锁从 transport 内部搬到了槽位；v2 的目标是**让接收半边从第一行代码起就没有被共享过**，因此不需要任何转换装置。

---

## 1. 现状问题（保留 v1 的诊断，补充新发现的）

### 1.1 根因仍然是「接收端所有权放错位置」

`Transport` 是 `Arc<dyn Transport>` + `Sync`，而 `recv` 天生只能有一个消费者、天生需要 `&mut`。于是每个 transport 各自造一套 `&mut`-from-`&self` 装置：

| Transport | 装置 | 位置 |
|---|---|---|
| CAN | `async_channel::Receiver`（`recv(&self)`） | `src/transport/can/mod.rs:343` |
| UDP | `Mutex<VecDeque>` + `Mutex<ReceivedCounterTracker>` | `src/transport/udp.rs:137` |
| TCP | `Mutex<TcpReadState>`（读半边 + 缓冲 + pending + 计数器） | `src/transport/tcp.rs:71` |

这些锁不是并发保护：全仓库只有一个 `recv` 调用点（`message_router.rs:135`）。

### 1.2 v1 未解决的「第二处所有权错位」：router 的输出通道

`MessageRouter` 自己也是个共享对象，于是它的两个输出也被迫加锁：

```rust
// src/transport/message_router.rs:88-100
cto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,   // 唯一消费者是 XcpSlave
dto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,   // 唯一消费者是 DaqManager 的后台任务
router_task: Arc<Mutex<Option<JoinHandle<()>>>>,
running: Arc<AtomicBool>,
```

`slave.get_dto_receiver()` 把这个 `Arc<Mutex<Receiver>>` 交给 DAQ，DAQ 任务在热循环里 `dto_rx.lock().await`（`src/daq.rs:1712`）。这也是「用锁把 `&mut` 造出来」，只是发生在 router 的输出侧而不是 transport 侧。

**结论：真正的彻底拆分必须一路拆到消费者。** 拆完 transport 却留着 router 的 `Arc<Mutex<Receiver>>`，只是把问题往后挪了一层。

### 1.3 顺带暴露出来的三处结构问题

1. **router 的 PID 分类与「跳过 EV/SERV 等响应」逻辑分居两地**：分类在 `message_router.rs`，跳过逻辑在 `slave.rs:378-...`（混在 `send_command_and_wait` 里，掺着日志与 `last_cmd_code` 记录），无法单测。
2. **UDP / TCP 重复实现**：`build_xcp_frame`、`CounterWarning`、`ReceivedCounterTracker`、4 字节头解析在 `udp.rs` 与 `tcp.rs` 里各一份（逐字近似），改协议要改两处。
3. **router 错误路径是无限 1ms 空转**（`message_router.rs:190`）：`ConnectionClosed` 之后 router 不会退出，只是每 1ms 打一条 trace，直到 `stop()` 被调用；期间 CTO 通道永远等不到数据，命令只能等到 T1 超时。

---

## 2. 目标

1. **收发在创建时就是两个独立对象**，发送半边只发、接收半边只收，类型上没有任何「可能缺失」的状态。
2. 接收路径（transport → router → CTO/DTO 通道 → 消费者）**零锁**。
3. 每个通道/句柄都**按值交给它唯一的消费者**：receiver → router；CTO → slave；DTO → DaqManager。
4. 语义零变化：超时、断连、PID 分类归属、DAQ 设备时间戳透传、错误映射全部保持。
5. 代码更短、更直白：删掉 4 个 recv 方法 × 3 个实现、3 个 `Mutex`、1 个 `Arc<Mutex<..>>`、1 个 `AtomicBool` 运行标志、1 个 `sleep(10ms)`、1 个 1ms 空转循环、1 份重复的以太网帧编解码。
6. 依赖做减法：撤 `async-channel`。
7. **错误类型一并收敛**：`TransportError` 从 9 个 `String` 变体收到 5 个带来源的错误（§3.9），并且「坏帧」不再作为错误冒泡。

### 非目标

- 不改协议层（UDP/TCP 的 LEN/COUNTER 头、TCP 流式拆帧、CAN 的 DLC 语义）。
- 不改 PID 分类规则（**永远**属于 router）。
- 不改 DAQ 数据解析/MF4/CSV 逻辑。
- 不改 `XcpSlave` 的命令流程语义（发命令 + 等 CTO）。

---

## 3. 目标架构

### 3.1 三层，每层只有一个「方向」

```
┌──────────────────────────────────────────────────────────────────┐
│ 第 3 层  XcpSlave                                                │
│   sender: Box<dyn TransportSender>   ← 只发                      │
│   cto:    CtoChannel                 ← 只收命令响应              │
│   dto:    Option<DtoChannel>         ← 一次性交给 DaqManager     │
├──────────────────────────────────────────────────────────────────┤
│ 第 2 层  router 任务（spawn_router）                              │
│   rx: Box<dyn TransportReceiver>     ← 按值独占，&mut self       │
│   → cto_tx / dto_tx（按 PID 分流）                                │
├──────────────────────────────────────────────────────────────────┤
│ 第 1 层  transport 两个半边                                       │
│   Box<dyn TransportSender>   &self + Sync，可共享                 │
│   Box<dyn TransportReceiver> &mut self + Send，不可共享           │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 第 1 层：`TransportLink`（核心变更）

```rust
// src/transport/link.rs

/// 一个已归一化的 XCP 包：载荷 + 可选的设备接收时间戳（微秒）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedPacket {
    pub payload: Vec<u8>,
    pub device_timestamp_us: Option<u64>,
}

/// 发送半边。
///
/// 只发不收，因此 `&self` + `Sync` 是自然成立的（不存在「唯一消费者」问题）。
/// 生命周期（`close`）挂在发送侧：它管的是连接/设备资源，而不是读流。
#[async_trait]
pub trait TransportSender: Send + Sync + 'static {
    async fn send(&self, payload: &[u8]) -> Result<(), TransportError>;
    async fn close(&self) -> Result<(), TransportError>;
    fn transport_type(&self) -> TransportType;
    fn target_addr(&self) -> String;
}

/// 接收半边。
///
/// 由**唯一**消费者按值持有（当前是 router 任务），因此 `recv` 取 `&mut self`：
/// 单消费者不变量由所有权保证，不需要锁，也**不需要 `Sync`**。
///
/// 契约：
/// - `recv` 必须是 cancel-safe（router 会在 `select!` 里对它取消除）。
/// - 链路结束返回 `Err(ConnectionClosed)`；实现不应在内部重试。
/// - 没有 `close()`：Drop 即关闭（读半边/通道随所有权一起消失）。
#[async_trait]
pub trait TransportReceiver: Send + 'static {
    async fn recv(&mut self) -> Result<ReceivedPacket, TransportError>;
}

/// 一条链路的两个半边 —— 创建即分离，没有「先共享再拆分」的阶段。
pub struct TransportLink {
    pub sender: Box<dyn TransportSender>,
    pub receiver: Box<dyn TransportReceiver>,
}
```

要点与取舍：

- **`TransportReceiver` 只有一个方法。** v1 的 `try_recv(timeout)` 在生产代码里没有调用方（只有 TCP 单测用），超时完全可以由调用方 `tokio::time::timeout(rx.recv())` 表达。删掉它，trait 小到不需要文档。
- **`Box` 而不是 `Arc`**：发送半边也只有 `XcpSlave` 一个持有者，没有理由允许共享。若将来确实需要（例如旁路嗅探），改成 `Arc` 是一行。
- **`ReceivedPacket` 用命名结构体**而不是 `(Vec<u8>, Option<u64>)`：`device_timestamp_us` 是 DAQ 时间戳重建的关键输入，元组第二位容易被当成「可有可无」而误删（v1 §9 已把它列为风险）。
- **`close()` 只在发送侧。** 接收侧不需要生命周期方法：Drop 就够（详见 §5.2）。

### 3.3 工厂：直接返回两半

```rust
// src/transport/factory.rs
impl TransportFactory {
    pub async fn open(config: &TransportConfig) -> Result<TransportLink, TransportError> {
        match config {
            TransportConfig::Udp { addr } => ethernet::udp::open(addr).await,
            TransportConfig::Tcp { addr } => ethernet::tcp::open(addr).await,
            TransportConfig::Can { .. } => can::open(config).await,
        }
    }
}
```

每个 transport 模块自己返回 `TransportLink`，内部先造出两个半边再组装：

```rust
// src/transport/ethernet/udp.rs
pub async fn open(addr: &str) -> Result<TransportLink, TransportError> {
    let socket = Arc::new(connect_socket(addr).await?);   // 收发都只需 &self，Arc 共享即可
    Ok(TransportLink {
        sender: Box::new(UdpSender {
            socket: socket.clone(),
            counter: AtomicU16::new(0),
            remote_addr,
        }),
        receiver: Box::new(UdpReceiver {
            socket,
            frames: EthFrameParser::new(),
        }),
    })
}
```

**v1 的 `take_receiver()`、`rx_slot`、`Option`、「第二次返回 `None`」、`SlaveError` 缺变体（v1 §12.2）全部不存在** —— 因为没有「移交」这个动作。

### 3.4 第 2 层：router 从「服务对象」变成「一个函数 + 两条通道」

```rust
// src/transport/router.rs

/// 两条输出通道共用的等待错误。
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    #[error("no message within {0:?}")]
    Timeout(Duration),
    #[error("link to {addr} is gone")]
    Closed { addr: String },
}

/// CTO 通道：唯一消费者是 `XcpSlave::send_command_and_wait`。
///
/// 这里保留一个 `Mutex`，理由见 §3.6（`&self` API 的硬约束），
/// 它是全仓库接收路径上唯一的锁。
pub struct CtoChannel {
    rx: Mutex<mpsc::Receiver<XcpMessage>>,
    addr: String,          // 只用于构造 ChannelError::Closed
}

impl CtoChannel {
    /// 等待一条命令响应（RES 0xFF / ERR 0xFE），跳过异步 EV(0xFD)/SERV(0xFC)。
    pub async fn wait_response(&self, timeout: Duration) -> Result<XcpMessage, ChannelError>;
}

/// DTO 通道：唯一消费者是 `DaqManager`（按值 move 进 DAQ 任务）。
///
/// 因为由任务独占，`recv` 直接取 `&mut self` —— **不需要 Mutex**。
pub struct DtoChannel {
    rx: mpsc::Receiver<XcpMessage>,
    addr: String,
}

impl DtoChannel {
    /// 等待下一个 DTO。`timeout` 让 DAQ 循环能周期性检查停止标志。
    pub async fn recv(&mut self, timeout: Duration) -> Result<XcpMessage, ChannelError>;
}

/// 启动路由任务。`rx` 的所有权移入后台任务；`addr` 只用于错误/日志上下文。
pub fn spawn_router(
    rx: Box<dyn TransportReceiver>,
    addr: String,
    capacity: usize,
) -> RouterTask;

pub struct RouterTask {
    cto: CtoChannel,
    dto: DtoChannel,
    cancel: CancellationToken,
    handle: Option<JoinHandle<()>>,   // 只为 shutdown 时能 await，对外不可见
}

impl RouterTask {
    pub fn take_cto(&mut self) -> CtoChannel;
    pub fn take_dto(&mut self) -> DtoChannel;
    /// 取消 + 等待任务结束；`rx` 随任务 drop，读半边随之关闭。
    pub async fn shutdown(&mut self);
}
```

任务体（伪代码，全部逻辑一眼看完）：

```rust
tokio::spawn(async move {
    let mut backoff = Duration::from_millis(1);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            received = rx.recv() => match received {
                Ok(packet) => {
                    backoff = Duration::from_millis(1);      // 成功即复位
                    route(packet, &cto_tx, &dto_tx);
                }
                // 链路正常结束：router 退出，两条输出通道随之关闭，
                // 等待中的命令立刻拿到 ChannelError::Closed。
                Err(TransportError::ConnectionClosed) => break,
                // 底层 IO 抖动（典型：UDP 的 ECONNREFUSED、EINTR）：退避重试，
                // 不退出、也不 1ms 空转。
                Err(e) => {
                    tracing::warn!("receive error, retrying in {:?}: {e}", backoff);
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(Duration::from_millis(100));
                }
            }
        }
    }
    tracing::info!("router stopped");
});
```

与现状相比的三处语义改进（**需要明确认可**，见 §8）：

| 项 | 现状 | 新 |
|---|---|---|
| `ConnectionClosed` | 打 trace 后 `sleep(1ms)` 无限循环 | router 退出，CTO/DTO 通道关闭 |
| 其它 IO 错误 | 固定 `sleep(1ms)` 无限重试（可能打满 CPU） | 指数退避 1ms → 100ms，成功即复位 |
| 命令等待时链路断开 | `cto.recv()` 永远 pending，命令只能等 T1 超时 | 通道关闭 → 立刻 `ChannelError::Closed` → `SlaveError::Transport(ConnectionClosed)` |
| 启动 | `connect` 里 `sleep(10ms)` 等任务起来 | 无需等待（receiver 已在 `open()` 里创建） |

### 3.5 第 3 层：`XcpSlave` 只持有「发」和「两个输出」

```rust
pub struct XcpSlave {
    pub info: SlaveInfo,
    /// 发送半边（只发）。
    sender: Box<dyn TransportSender>,
    /// router 任务 + CTO 通道。
    cto: CtoChannel,
    router: RouterTask,
    /// DAQ 数据通道，一次性交给 DaqManager（见 §3.7）。
    dto: std::sync::Mutex<Option<DtoChannel>>,
    // ... cal_manager / daq_manager / packet_log / command_timeout 不变
}
```

`connect`：

```rust
let TransportLink { sender, receiver } = TransportFactory::open(&config).await?;
let mut router = spawn_router(receiver, transport_config.addr_string(), ROUTER_CHANNEL_CAPACITY);
let cto = router.take_cto();
let dto = router.take_dto();

let mut slave = Self { sender, cto, router, dto: Mutex::new(Some(dto)), /* ... */ };
let connect_info = slave.send_connect().await?;   // 立刻可发，无 sleep
```

`disconnect`：

```rust
let _ = self.send_command_and_wait(&commands::disconnect()).await;
self.router.shutdown().await;      // rx drop → 读半边关闭；CTO/DTO 通道关闭
self.sender.close().await?;        // 设备/写半边收尾
```

`send_command_and_wait` 变薄（只保留「发 + 记日志 + 记 last_cmd_code + 等响应」）：

```rust
pub async fn send_command_and_wait(&self, command: &[u8]) -> Result<Vec<u8>, SlaveError> {
    self.sender.send(command).await?;                 // 发（TransportError → SlaveError::Transport）
    self.log_outgoing(command);                       // 日志 + last_cmd_code
    let msg = self.cto.wait_response(self.command_timeout).await?;   // 收（跳过 EV/SERV 的逻辑已下沉）
    Ok(msg.payload)
}
```

错误映射集中在两个 `From` 实现里，`send_command_and_wait` 保持无分支：

```rust
impl From<ChannelError> for SlaveError {
    fn from(e: ChannelError) -> Self {
        match e {
            ChannelError::Timeout(_) => SlaveError::ResponseTimeout,
            ChannelError::Closed { addr } => {
                SlaveError::Transport(TransportError::ConnectionClosed { addr })
            }
        }
    }
}
```

### 3.6 为什么 `CtoChannel` 里还留一个 `Mutex`（而 `DtoChannel` 没有）

这是全仓库接收路径上**唯一**保留的锁，必须解释清楚：

- `send_command_and_wait(&self)` 的 `&self` 不是随手写的：`XcpSlave` 到处是 `Arc<Mutex<XcpSlave>>`（`client.rs:87`），且 `DaqManager::start(&mut self, slave: &XcpSlave, ...)` 在持有 `slave.daq_manager` 的锁的同时还要对 `slave` 发命令（`daq.rs:1596`）。把 32 个调用点全改成 `&mut self` 会直接和「`slave.daq_manager.lock()` 与 `&mut slave` 不能共存」冲突。
- 因此这里需要「`&mut`-from-`&self`」，但它被**限制在一个 10 行的类型里**，且不变量写在该类型的文档上：*CTO 通道只有一个消费者；外层 `Arc<Mutex<XcpSlave>>` 已经串行化了所有命令*。
- 与现状的区别：现状是 `Arc<Mutex<Receiver>>` 泄漏到 `XcpSlave` 字段和 `send_command_and_wait` 的内联循环里（还要和 `last_cmd_code`、日志、`timeout` 混在一起）；新设计里它被封装成 `CtoChannel::wait_response(timeout)`，是**可单测的纯逻辑**（喂一条 EV + 一条 RES，断言跳过）。

如果将来接受「`&mut self` 化」，把 `CtoChannel` 的字段改成 `rx: mpsc::Receiver<..>` 并把 `wait_response(&mut self)` 即可，其它层不动。

**对比 `DtoChannel`：它没有锁。** 因为 DAQ 任务按值独占它（`recv(&mut self)`）。同一份代码里两种形状并存，正好说明了区别：*按值交给唯一消费者* 不需要锁，*共享之后再造 `&mut`* 才需要。

### 3.7 DTO 通道：按值交给 `DaqManager`（已确定，一步到位）

现状：`slave.get_dto_receiver() -> Arc<Mutex<Receiver>>`，DAQ 任务每轮 `lock().await` 一次（`daq.rs:1712`）。

新设计：DTO 通道的所有者是 `DaqManager`；`start` 时 move 进后台任务，任务退出时**归还**。

```rust
pub struct DaqManager {
    /// DAQ 数据通道：所有权在「DaqManager」与「运行中的 DAQ 任务」之间来回传递。
    dto: Option<DtoChannel>,
    /// 后台任务退出时把 dto 还回来（JoinHandle<DtoChannel>）。
    task: Option<JoinHandle<DtoChannel>>,
    ...
}

impl DaqManager {
    pub async fn start(&mut self, slave: &XcpSlave, _interval_ms: u64) -> Result<(), String> {
        self.reclaim_dto().await;                       // ① 先回收上一次任务归还的通道
        let dto = match self.dto.take() {
            Some(dto) => dto,
            None => slave.take_dto()                     // ② 否则向 slave 一次性取走
                .ok_or("DAQ data channel already taken")?,
        };
        ...
        self.task = Some(tokio::spawn(async move {
            loop {
                match dto.recv(Duration::from_millis(100)).await {
                    Ok(msg) => { /* 解析、落盘：逻辑不变 */ }
                    Err(ChannelError::Timeout(_)) => {
                        if !running.load(Ordering::SeqCst) { break; }   // 现有停止标志
                    }
                    Err(ChannelError::Closed { .. }) => break,            // 链路断了
                }
            }
            dto                                             // ③ 归还
        }));
        Ok(())
    }

    pub async fn stop(&mut self, _slave: &XcpSlave) -> Result<(), String> {
        self.running.store(false, Ordering::SeqCst);
        self.reclaim_dto().await;                       // await 任务 → 拿回 dto
        Ok(())
    }

    /// 等待后台任务结束并收回 DTO 通道。
    async fn reclaim_dto(&mut self) {
        if let Some(handle) = self.task.take() {
            match handle.await {
                Ok(dto) => self.dto = Some(dto),
                // 任务 panic/abort：通道丢失。显式报错，不再静默收不到数据。
                Err(e) => tracing::error!("DAQ task ended abnormally, DTO channel lost: {e}"),
            }
        }
    }
}
```

`XcpSlave` 侧只多一个一次性取走方法：

```rust
/// 取走 DAQ 数据通道（一次性）。`DaqManager` 是它的唯一消费者。
pub fn take_dto(&self) -> Option<DtoChannel> {
    self.dto.lock().expect("dto slot poisoned").take()
}
```

内部是 `std::sync::Mutex<Option<DtoChannel>>` 的一次性 take，**不跨 await**。

要点：

- **热循环零锁**：`dto` 是任务内的局部独占变量，`recv(&mut self)` 直接 await 底层 `mpsc::Receiver`。
- **任务必须优雅退出**（`break` 后返回 `dto`），不能用 `abort`。现有任务已在每轮检查 `running` / `selection_changed`，把「退出」改成 break 即可。
- **重启路径**（`selection_changed` 让任务自行退出）也走 `reclaim_dto`：`start` 开头先回收，再取新通道，顺序不能反。
- **panic 语义显式化**：任务 panic 会让 `dto` 丢失，后续 `start` 报错，而不是像现在那样静默地「DAQ 在跑但收不到数据」。
- `XcpSlave::get_dto_receiver()`（`slave.rs:1815`）与 `XcpClient::get_dto_receiver()`（`client.rs:324`，无调用方）一并删除。

### 3.8 各 transport 的两个半边

#### 以太网共用部分（新增，删掉 UDP/TCP 的重复）

```rust
// src/transport/ethernet/codec.rs
pub const ETH_HEADER_SIZE: usize = 4;

pub fn build_frame(payload: &[u8], counter: u16) -> Vec<u8>;

/// 增量拆帧器：同时服务 datagram（UDP）与字节流（TCP）。
pub struct EthFrameParser {
    buf: Vec<u8>,
    pending: VecDeque<Vec<u8>>,
    counter: ReceivedCounterTracker,   // 计数器跳变告警
}
impl EthFrameParser {
    pub fn push_datagram(&mut self, datagram: &[u8]);  // 一次 datagram 可能含多帧；残帧丢弃并 warn
    pub fn push_stream(&mut self, chunk: &[u8]);       // 追加到 buf，能解多少解多少
    pub fn pop(&mut self) -> Option<Vec<u8>>;
}
```

#### UDP

```rust
pub struct UdpSender { socket: Arc<UdpSocket>, counter: AtomicU16, remote_addr: SocketAddr }
pub struct UdpReceiver { socket: Arc<UdpSocket>, frames: EthFrameParser }
```

- 发送/接收共用一个 `Arc<UdpSocket>`：`tokio::net::UdpSocket` 的 `send`/`recv` 都是 `&self`，这是**真共享**，不是可变性转换。
- `pending_packets: Mutex<VecDeque>` 与 `received_counter_tracker: Mutex<..>` 直接消失（搬进 `frames`）。
- UDP 无连接：`recv` 只可能返回 IO 错误，不会 `ConnectionClosed`（语义不变）。

#### TCP

```rust
pub struct TcpSender { writer: Mutex<OwnedWriteHalf>, counter: AtomicU16, remote_addr: SocketAddr }
pub struct TcpReceiver { reader: OwnedReadHalf, frames: EthFrameParser }
```

- `into_split()` 天然给出两个半边，这是「创建即分离」在 TCP 上最自然的落点。
- `TcpSender.writer` 的 `Mutex` 保留：`&mut OwnedWriteHalf` 要跨 `.await`，且属于发送路径。
- `TcpReadState` 这个名字和它的 `Mutex` 一起消失。
- `read() == 0` → `ConnectionClosed`（不变）。

#### CAN

```rust
/// 两个半边共享的连接资源（设备句柄 + 读线程）。
struct CanShared {
    device: Mutex<Box<dyn CanDevice>>,   // PCAN/ES582 句柄需要串行访问（真锁）
    stop: CancellationToken,
    reader: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for CanShared { /* cancel + join + device.close() */ }

pub struct CanSender {
    shared: Arc<CanShared>,
    can_id: u32, extended: bool, fd: bool, max_dlc_required: bool,
}
pub struct CanReceiver {
    shared: Arc<CanShared>,                          // 保证读线程活到最后一个半边消失
    frames: mpsc::UnboundedReceiver<CanFrame>,       // tokio::sync::mpsc（撤掉 async-channel）
}
```

- reader 线程不变（`Sender<CanFrame>` + `try_send`；接收端 drop → `try_send` 失败 → 线程退出）。
- `AtomicBool` 停止标志换成 `CancellationToken`（`tokio-util` 已是依赖），与 router 的取消用同一个原语。
- `close()` 在发送侧：`stop.cancel()` + `join` + `device.close()` → channel 关闭 → 接收半边返回 `ConnectionClosed`。
- **`async-channel` 依赖可以整体撤掉**（`7458571` 引入的，`recv` 变成 `&mut self` 后 `tokio::sync::mpsc` 足够）。

---

### 3.9 `TransportError` 重构（已确定）

#### 现状的问题

| 变体 | 问题 |
|---|---|
| `SendFailed(String)` / `RecvFailed(String)` / `SocketError(String)` / `ConnectFailed(String)` / `BindFailed(String)` | 全是 `String`：原始 `io::Error` 被 `to_string()` 掉，丢失 `ErrorKind`（将来想区分「可重试的 WouldBlock」就只能去解析字符串） |
| `RecvFailed` 同时表示两种相反的失败 | ① 底层 IO 失败（链路坏了，该停）② 收到的字节不是合法帧（对端发垃圾，该跳过继续）。处理策略相反，却共用一个变体 |
| `Timeout` | 改造后接收半边没有超时方法，**无人产生**（仅 `slave.rs:2290` 的单测在用） |
| `NotImplemented` | 仅 `transport.rs` 单测在用，生产代码从不产生 |
| `ConnectionClosed` | 唯一「非故障」变体，却和故障混在同一个 enum 里，调用方必须 `matches!` 才能区分 |

#### 新定义

```rust
// src/transport/error.rs

/// 传输层错误。分类对应半边的生命周期：
/// - `InvalidAddress` / `OpenFailed`：只从 `open()` 返回（配置/建链失败）
/// - `SendFailed` / `RecvFailed`：运行期故障，携带原始错误
/// - `ConnectionClosed`：链路正常结束，**不是故障**
///
/// 刻意不提供：
/// - `Timeout`：接收半边没有超时方法，超时由调用方 `tokio::time::timeout` 表达
/// - `MalformedFrame`：坏帧是传输层内部事务，接收半边自行丢弃并 warn，
///   不冒泡给消费者（否则 router 必须为它写一条恢复分支）
/// - `NotImplemented`：没有任何实现会返回它
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid address `{0}`")]
    InvalidAddress(String),

    #[error("cannot open link to {addr}: {source}")]
    OpenFailed {
        addr: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("send failed on {addr}: {source}")]
    SendFailed {
        addr: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("receive failed on {addr}: {source}")]
    RecvFailed {
        addr: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("link to {addr} closed")]
    ConnectionClosed { addr: String },
}

impl TransportError {
    pub(crate) fn open(addr: &str, source: impl Into<Box<dyn Error + Send + Sync>>) -> Self;
    pub(crate) fn send(addr: &str, source: impl Into<Box<dyn Error + Send + Sync>>) -> Self;
    pub(crate) fn recv(addr: &str, source: impl Into<Box<dyn Error + Send + Sync>>) -> Self;
    pub(crate) fn closed(addr: &str) -> Self;
}
```

#### 四个要点

1. **`Box<dyn Error + Send + Sync>` 而不是 `io::Error`**：UDP/TCP 的底层错误是 `io::Error`，CAN 的是 `CanDeviceError`（不是 io）。用 trait object 统一，构造处写 `TransportError::send(self.addr(), e)` 即可（`From<io::Error> for Box<dyn Error + Send + Sync>` 是 std 现成的）。需要 `ErrorKind` 时用 `source.downcast_ref::<io::Error>()`。
2. **`addr` 进错误体**：现在日志里只有 `RecvFailed("Connection reset by peer")`，多 slave 场景下不知道是哪条链路。发送/接收半边本来就存着地址，顺手带上。
3. **`ConnectionClosed { addr }` 带地址**：router 的日志和 `SlaveError` 都能自解释。
4. **构造器而不是结构体字面量**：调用点从 `TransportError::SendFailed(e.to_string())` 变成 `TransportError::send(self.addr(), e)`，原始错误完整保留。

#### 连带清理

- `Timeout`、`NotImplemented`、`SocketError`、`BindFailed`、`ConnectFailed` 全部删除（后两个折叠进 `OpenFailed`）。
- `SlaveError::Transport(#[from] TransportError)` 不变；`slave.rs:2290` 的单测改用 `TransportError::InvalidAddress`。
- 具体 transport 上那些**只为 trait 而存在的访问器**一并删掉（grep 已确认生产代码零调用）：`UdpTransport::set_timeout/local_addr/remote_addr/current_counter`、`TcpTransport::set_timeout/remote_addr/current_counter`、`CanTransport::driver/channel/can_id/is_fd`。`target_addr()` 保留（发送半边的错误信息与日志需要它）。

---

## 4. 所有权与生命周期全景

```
TransportFactory::open(config)
        │
        ├─► Box<dyn TransportSender> ──────────────► XcpSlave.sender   （只发）
        │                                              │
        └─► Box<dyn TransportReceiver> ─► router 任务 ──┤
                                            │          │
                              ┌─────────────┴───┐      │
                              ▼                 ▼      │
                        CtoChannel        DtoChannel   │
                              │                 │      │
                     XcpSlave.cto      DaqManager.dto  │
                    （wait_response）  （move 进任务）  │
                                                      │
                    shutdown: cancel → await → rx drop ┘ → 读半边/通道关闭
```

生命周期规则（三条，全部由类型保证）：

1. `TransportReceiver` 只被 router 任务持有；任务结束 = 接收半边关闭。
2. `CtoChannel` 只被 `XcpSlave` 持有；`DtoChannel` 只被 `DaqManager` 持有。
3. `close()` 只在发送侧；接收侧没有生命周期方法，靠 Drop。

---

## 5. 语义与边界（必须与现状逐项对齐）

### 5.1 错误/超时映射

| 情况 | 现状 | 新 |
|---|---|---|
| 命令等待超时 | `SlaveError::ResponseTimeout` | `ChannelError::Timeout` → `ResponseTimeout`（不变） |
| 等待期间链路断开 | 等满 T1 后超时 | `ChannelError::Closed` → `SlaveError::Transport(ConnectionClosed)`（**语义改进**，见 §8.1） |
| 底层 IO 错误 | `RecvFailed`，router 无限 1ms 重试 | `RecvFailed { addr, source }`，router 指数退避重试（1ms→100ms） |
| 收到的字节不是合法帧 | `RecvFailed("...without a complete XCP frame")` 冒泡给 router | 接收半边内部丢弃 + warn，**不冒泡**（§3.9） |
| CAN reader 线程退出 | channel 关闭 → `ConnectionClosed` | `ConnectionClosed { addr }` |
| TCP 对端关闭（`read()==0`） | `ConnectionClosed` | `ConnectionClosed { addr }` |
| DAQ 通道满 | `try_send` 丢弃并 trace | 同（容量策略不变） |
| router 任务停止 | `abort` | 先 cancel，再 `shutdown` 里 await |

### 5.2 `close()` 与接收半边

- **TCP**：`sender.close()` 只 shutdown 写半；读半由 router 的 `shutdown()`（drop `OwnedReadHalf`）或对端 FIN 结束。
- **CAN**：`sender.close()` 取消 + join 读线程 + 关设备 → 接收通道关闭 → `ConnectionClosed`（与现状一致）。
- **UDP**：无连接，`close()` 只记日志。
- `disconnect()` 的顺序固定为 **router 先停，sender 后关**：先停接收避免「发了 DISCONNECT 之后还在收」，再释放设备。

### 5.3 边界条件

- **`recv` 必须 cancel-safe**：router 在 `select!` 里对它取消。已核对：`tokio::sync::mpsc::Receiver::recv`、`UdpSocket::recv`、`AsyncReadExt::read` 都是 cancel-safe；CAN 侧同理。
- **多 slave**：每个 `XcpSlave` 一条链路、一个 router、一个 receiver，互不共享，天然无锁。
- **DAQ 重启**：任务必须优雅退出并归还 `DtoChannel`（不能用 `abort`），否则重启拿不到通道。
- **`take_dto` 只能成功一次**：第二次返回 `None`，`DaqManager::start` 报错而不是静默收不到数据。

---

## 6. 文件布局（建议）

```
src/transport/
├── mod.rs              // 对外导出：TransportLink / TransportSender / TransportReceiver
│                       //           TransportConfig / TransportError / TransportType / Factory
├── link.rs             // trait TransportSender / TransportReceiver / ReceivedPacket / TransportLink
├── error.rs            // TransportError / TransportType
├── config.rs           // TransportConfig
├── factory.rs          // TransportFactory::open
├── router.rs           // spawn_router / RouterTask / CtoChannel / DtoChannel / XcpMessage
├── ethernet/
│   ├── mod.rs
│   ├── codec.rs        // 4 字节头、build_frame、EthFrameParser、ReceivedCounterTracker
│   ├── udp.rs          // UdpSender / UdpReceiver / open()
│   └── tcp.rs          // TcpSender / TcpReceiver / open()
└── can/
    ├── mod.rs          // open() / CanShared / FrameFilter
    ├── sender.rs
    ├── receiver.rs
    ├── reader.rs       // 读线程
    ├── device.rs       // CanDevice trait
    ├── pcan.rs
    └── es582.rs
```

`transport.rs`（当前 22 行的 re-export 文件）与 `transport/transport.rs`（326 行，trait + config + error + factory 混在一起）合并/拆分到上面的 `mod.rs` / `link.rs` / `error.rs` / `config.rs` / `factory.rs`。

---

## 7. 删除清单（可量化的收益）

| 删除项 | 位置 |
|---|---|
| `Transport::recv_packet` / `try_recv_packet` / `recv_packet_ts` / `try_recv_packet_ts`（4 × 3 实现） | `transport.rs:136-158`、udp/tcp/can |
| `rx_slot` / `take_receiver` 一次性移交（v1 引入的概念） | 不进入 v2 |
| `Mutex<VecDeque>` + `Mutex<ReceivedCounterTracker>` | `udp.rs:137-141` |
| `Mutex<TcpReadState>` | `tcp.rs:71,157` |
| `RxQueue` | `can/mod.rs:343` |
| `async-channel` 依赖 | `Cargo.toml` |
| `MessageRouter` 结构体（`Arc<Mutex<Receiver>>` × 2 + `AtomicBool` + `Mutex<Option<JoinHandle>>`） | `message_router.rs:88-100` |
| `XcpSlave.router` / `XcpSlave.transport` 双字段 | `slave.rs:174-180` |
| `connect` 里的 `sleep(10ms)` | `slave.rs:257` |
| router 的 1ms 错误空转 | `message_router.rs:190` |
| UDP/TCP 重复的 `build_xcp_frame` / `CounterWarning` / `ReceivedCounterTracker` | `udp.rs:36-84`、`tcp.rs:36-70` |
| `send_command_and_wait` 里内联的「跳过 EV/SERV」循环 | `slave.rs:378-...`（下沉为 `CtoChannel::wait_response`） |
| DAQ 热循环里的 `dto_rx.lock().await` | `daq.rs:1712` |
| `XcpSlave::get_dto_receiver` / `XcpClient::get_dto_receiver` | `slave.rs:1815`、`client.rs:324` |
| `TransportError` 的 `Timeout` / `NotImplemented` / `SocketError` / `BindFailed` / `ConnectFailed` | `transport.rs:106-133` |
| 无人调用的 transport 访问器（`set_timeout` / `local_addr` / `remote_addr` / `current_counter` / `driver` / `channel` / `can_id` / `is_fd`） | udp/tcp/can |

---

## 8. 已定的行为变化（均已经过确认）

### 8.1 router 在 `ConnectionClosed` 时退出

现状是无限 1ms 空转，命令只能等到 T1 超时。新行为是通道关闭 → 命令立刻失败（`ChannelError::Closed` → `SlaveError::Transport(ConnectionClosed)`）。

**注意区分**：只有 `ConnectionClosed` 让 router 退出；其它 IO 错误（典型是 UDP connected socket 上的 `ECONNREFUSED`）走指数退避重试，保留「对端暂时不在线」的可恢复性。

### 8.2 DAQ 通道所有权改为按值（一步到位）

见 §3.7。`daq.rs` 的 `start`/`stop` 改为「通道 move 进任务 / 任务归还通道」，任务必须优雅退出（`break` + 返回 `dto`），不能 `abort`。

### 8.3 `try_recv` 删除

生产代码无调用方；单测用 `tokio::time::timeout` 表达。

### 8.4 `TransportError` 是不兼容变更

变体集合与字段形状都变了。全仓库（含 `src-tauri`）没有对变体做 `matches!`（已 grep 确认），只有 `slave.rs:2290` 的单测用了 `Timeout`，改一行即可。

### 8.5 坏帧不再冒泡

UDP 的「datagram 里没有完整 XCP 帧」与 TCP 的「帧头长度不合法」现在会作为 `RecvFailed` 冒泡给 router（然后 router 等 1ms 重试）。新设计把它们当传输层内部事务：**丢弃并 warn，继续收下一帧**。对消费者而言，`recv` 只会返回合法包或终止错误，router 因此只需要两个分支。

---

## 9. 测试计划

### 9.1 单测（无硬件）

| 用例 | 覆盖点 |
|---|---|
| `CtoChannel::wait_response` 跳过 EV(0xFD)/SERV(0xFC) | 迁移 `mock_xcp_integration.rs` 的场景为纯单测（喂 channel） |
| `CtoChannel::wait_response` 超时 / 通道关闭 | `ChannelError::Timeout` / `Closed` |
| `ChannelError` → `SlaveError` 映射 | 超时 → `ResponseTimeout`；关闭 → `Transport(ConnectionClosed)` |
| `spawn_router` PID 分类（0xFC/0xFB 边界） | 用 mock receiver 喂包，断言 CTO/DTO 分流 |
| `spawn_router` 遇 `ConnectionClosed` 退出并关闭输出 | 新行为（§8.1） |
| `spawn_router` 遇 `RecvFailed` 退避重试而不退出 | 与 `ConnectionClosed` 区别对待（§3.4） |
| mock `TransportReceiver` 的 `recv` 被 cancel | cancel-safe 契约 |
| `EthFrameParser` datagram 多帧 / 流式半包粘包 / 残帧丢弃并继续 | 合并现有 UDP/TCP 解析单测；**残帧不再返回错误**（§8.5） |
| `ReceivedCounterTracker` 跳变与回绕 | 从 udp/tcp 各一份合并为一份 |
| `TcpReceiver` 对端关闭 → `ConnectionClosed { addr }` | 迁移 |
| `CanReceiver` 保序 + 时间戳透传 + sender drop | 迁移 `test_rx_queue_*` |
| `TransportError` 构造器保留原始错误 | `source.to_string()` 含 io 错误文本（不再丢 `ErrorKind`） |
| `TransportFactory::open` 返回两半且互不依赖 | 新不变量的回归测试 |
| `DaqManager::stop` 后重启仍能拿到 `DtoChannel` | `reclaim_dto` 的正确性（§3.7） |

### 9.2 集成

- `tests/mock_xcp_integration.rs`：`send_command_and_wait_skips_unsolicited_ev_and_serv_packets` 必须保持通过（走新的接收路径 + 新的跳过逻辑）。
- 多 slave 场景：每个 router 独立持有自己的 receiver。

### 9.3 手工（硬件）

- CAN（PCAN / ES582）：连接、标定读写、DAQ 录制；确认 DAQ 设备时间戳仍透传（`daq_timestamp_reconstruction` 依赖 `device_timestamp_us`）。
- UDP/TCP：连接、标定读写、DAQ 录制各一遍；断开后确认命令立即报 `ConnectionClosed` 而不是等 1s。

---

## 10. 验收标准

- [ ] `TransportSender` 上没有任何接收方法；`TransportReceiver` 只有一个 `recv`
- [ ] 不存在 `take_receiver` / `rx_slot` / 「一次性移交」概念
- [ ] 接收路径（transport → router → CTO/DTO 通道 → 消费者）无 `Mutex`，`CtoChannel` 除外（§3.6 已论证）
- [ ] `TransportError` 只剩 5 个变体，且运行期错误携带 `addr` + 原始 `source`（§3.9）
- [ ] `Cargo.toml` 不再含 `async-channel`
- [ ] `cargo test` 全绿；`transport::*` 单测数量不少于改造前
- [ ] `cargo clippy --all-targets` 对 `src/transport/**` 无 warning
- [ ] `docs/ARCHITECTURE_DIAGRAM.md` 更新为两个半边 + router 函数

---

## 11. 备选方案与否决理由

| 方案 | 结论 | 理由 |
|---|---|---|
| v1：`Arc<dyn Transport>` + `take_receiver()` + `rx_slot` | ❌ 已否决 | 仍然存在「对象可能没有收半边」的状态和「只能成功一次」的运行时约定；`connect` 多一个可失败步骤 |
| `Arc<Mutex<dyn Transport>>`，`recv` 取 `&mut self` | ❌ 否决 | router 会跨无限期 `.await` 持锁，`send` 死锁（v1 §1.2 的论证仍然成立） |
| 保留 `Mutex<Receiver>`（CTO/DTO 通道） | ❌ 已否决（DTO） | CTO 侧因 `&self` API 不可避，已封装进 `CtoChannel`（§3.6）；DTO 侧无此约束，按值交给 `DaqManager` |
| `async-channel`（`recv(&self)`） | ❌ 本方案后移除 | 零锁，但仍是「接收端塞在共享对象里」；`recv` 变 `&mut self` 后 `tokio::sync::mpsc` 足够 |
| 接收半边做成 `enum TransportReceiver { Udp/Tcp/Can }`（无 trait object） | ⚠️ 可选，不采纳 | 少一层装箱；代价是三个模块被枚举耦合，且 mock receiver（集成测试需要）要额外加变体 |
| `UnsafeCell` + `unsafe impl Sync` | ❌ 否决 | 并发 `recv` 即 UB |
| 保持现状 | ❌ 不采纳 | 功能无缺陷，但接收路径三套锁 + router 双锁 + 错误类型混杂 |

---

## 12. 分步实施（每步可编译 + 提交）

| 步骤 | 内容 | 验证 |
|---|---|---|
| 1 | 新增 `error.rs`（新 `TransportError` + 构造函数）、`link.rs`（两个 trait + `ReceivedPacket` + `TransportLink`）、`ethernet/codec.rs`；不动实现 | `cargo build` |
| 2 | `ethernet/udp.rs`：`UdpSender` / `UdpReceiver` / `open()`，删两个 `Mutex` + 无用访问器 | `cargo test transport` |
| 3 | `ethernet/tcp.rs`：`TcpSender` / `TcpReceiver` / `open()`，删 `TcpReadState` 的 `Mutex` | `cargo test transport` |
| 4 | `can/`：`CanSender` / `CanReceiver` / `CanShared`，撤 `async-channel`，`CancellationToken` 取代 `AtomicBool` | `cargo test transport::can` |
| 5 | `router.rs`：`spawn_router` + `CtoChannel` + `DtoChannel` + `ChannelError`；`factory::open` 返回 `TransportLink` | `cargo test` |
| 6 | `slave.rs`：字段换成 `sender`/`cto`/`router`/`dto`，`send_command_and_wait` 变薄，删 `sleep(10ms)`、`get_dto_receiver` | `cargo test`（全量） |
| 7 | `daq.rs`：`DtoChannel` 按值持有 + 任务归还 + `reclaim_dto`（§3.7）；删 `client.rs::get_dto_receiver` | `cargo test` + 手工 DAQ |
| 8 | 清理：删 `async-channel`、更新文档与 `ARCHITECTURE_DIAGRAM.md` | `cargo clippy --all-targets` |

步骤 5 是唯一的「一刀切」点（trait 与调用方必须同时改）；1-4 只增不删，5 之后旧路径整体消失，不需要 v1 §8 那种「两套接收路径并存」的阶段。

---

## 13. 决策记录

| # | 问题 | 决定 | 落地位置 |
|---|---|---|---|
| 1 | 命名 | `TransportSender` / `TransportReceiver` | §3.2 |
| 2 | `ReceivedPacket` 形状 | 命名结构体（`payload` + `device_timestamp_us`） | §3.2 |
| 3 | DAQ 通道所有权 | 一步到位：按值交给 `DaqManager`，热循环零锁 | §3.7 / §8.2 |
| 4 | router 遇 `ConnectionClosed` | 退出；其它 IO 错误退避重试 | §3.4 / §8.1 |
| 5 | `TransportError` | 一并重构：`InvalidAddress` / `OpenFailed` / `SendFailed` / `RecvFailed` / `ConnectionClosed` | §3.9 |

### 仅剩的实现细节（不阻塞开工）

1. `spawn_router` 的两条通道容量：现在 CTO/DTO 共用一个 `100`。拆分后可给 DTO 更大容量（DAQ 是突发流，`try_send` 满了就丢包）。建议 CTO=64、DTO=1024，实测后定。
2. `ReceivedPacket` 是否顺手带上 CAN ID / 帧元信息（现在 `CanFrame.id` 在 filter 之后被丢弃）。不属于本重构，但接收半边重构后加字段成本最低。

---

## 14. 实施记录

### 14.1 与设计稿的偏差（均为简化，已落地）

| 设计稿 | 实际实现 | 原因 |
|---|---|---|
| `RouterTask { take_cto(), take_dto() }` | `spawn_router(...) -> (CtoChannel, DtoChannel, RouterHandle)` | 少一层 `Option`/`take_*`，调用点一行解构 |
| `CanTransportConfig` | 改名 `CanConfig` | `CanTransport` 类型已不存在（只剩两个半边） |
| `ChannelError::Closed` | `ChannelError::Closed { addr }` | `From<ChannelError> for SlaveError` 不需要额外上下文 |
| `DaqManager { dto: Option<DtoChannel> }` | `dto: Arc<RwLock<Option<DtoChannel>>>` | `DaqManager` 是 `Clone`（`task_handle` 同样是 `Arc<RwLock<..>>`）；热循环仍零锁 |
| DTO 在 DAQ 配置前取走 | 在**所有可失败步骤之后、spawn 之前**取走 | `start()` 中途 `?` 返回时不会丢通道；期间到达的 DTO 仍缓存在 router 通道里 |
| — | `TransportError::addr()` 辅助方法 | 便于日志与映射 |

通道容量按 §13 落地：`CTO_CHANNEL_CAPACITY = 64`、`DTO_CHANNEL_CAPACITY = 1024`。

### 14.2 验证结果

- `cargo test`：**549 项全绿**（lib 443 + bin 86 + 集成 20），`cargo clippy --all-targets` 对新代码无 warning。
- 硬件（ETAS ES582 `USB/ES582.1;114913` ctrl 1，`pms-a.a2l`，CAN 0x550/0x551 500 kbit/s）：
  - `daq-print`（两个 event channel 并发）：持续 20 行/秒，无丢帧、无告警，DAQ 设备时间戳透传正常。
  - **并发标定**：另起一个进程 `calibrate --var AIN_v_VoltageThd_C --value 0.75`，在 DAQ 持续流的同时完成 SET_CAL_PAGE + SET_MTA + DOWNLOAD（Success: 1），DAQ 侧输出不间断（校准前后行数 128 → 170，无 gap）。
  - 回读校验：临时探针读 0x08120004 得到 `0x3F400000` = 0.75，写入确实生效（写的是 working page，复位即回退）。
  - DAQ 生命周期探针：连续两轮 `start → 有数据 → stop → reclaim_dto → start` 均成功，最后 `disconnect` 干净退出（验证 DTO 通道交接与 router/sender 关闭顺序）。