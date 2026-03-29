# XCP 多 Slave 支持重构计划

## 1. 需求概述

### 1.1 目标
- 支持一个 XCP Client 连接多个 Slave 设备
- 支持不同类型的 Transport (UDP、CAN等)
- 每个 Slave 的 A2L 数据、DAQ 配置、标定变量独立存储
- UI 能够区分显示不同设备的变量

### 1.2 示例场景
```
设备1: UDP 172.100.1.5:5555  - 控制单元 A
设备2: UDP 172.100.1.6:5555  - 控制单元 B
设备3: CAN 通道 (ID: 0x123) - 预留扩展
```

---

## 2. 当前架构分析

### 2.1 核心组件现状

| 组件 | 当前设计 | 问题 |
|------|----------|------|
| `XcpClient` | 单连接，直接绑定 `UdpTransport` | 无法管理多连接 |
| `CalibrationManager` | 全局单例，独立存储 | 无法区分设备来源 |
| `DaqManager` | 全局单例，独立存储 | 无法区分设备来源 |
| `A2lReader` | 每次调用新建实例 | 数据无法与设备关联 |
| TUI `App` | 单一状态管理 | 没有设备选择概念 |

### 2.2 关键代码问题

```rust
// src/client.rs:75-81 - XcpClient 当前设计
pub struct XcpClient {
    transport: Option<Arc<UdpTransport>>,  // 单一Transport
    router: Option<Arc<MessageRouter>>,
    state: Arc<RwLock<ClientState>>,
    cto_rx: Option<Arc<Mutex<mpsc::Receiver<XcpMessage>>>>,
}

// src/calibration.rs:269-271 - CalibrationManager 当前设计
pub struct CalibrationManager {
    params: Vec<CalibrationParam>,  // 无设备标识
}

// src/daq.rs:467-502 - DaqManager 当前设计
pub struct DaqManager {
    all_measurements: Vec<Measurement>,  // 无设备标识
    slave_addr: String,  // 单一地址
    // ...
}
```

---

## 3. 新架构设计

### 3.1 核心设计图

```
┌─────────────────────────────────────────────────────────────────┐
│                    XcpClient (Manager)                          │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  - slaves: HashMap<SlaveId, Arc<Mutex<XcpSlave>>>       │   │
│  │  - transport_factory: TransportFactory                   │   │
│  │  - 全局路由协调                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
└──────────────────────┬──────────────────────────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        │              │              │
        ▼              ▼              ▼
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│  Slave: ECU1 │ │  Slave: ECU2 │ │  Slave: ECU3 │
│  ┌──────────┐│ │  ┌──────────┐│ │  ┌──────────┐│
│  │Transport ││ │  │Transport ││ │  │Transport ││
│  │  (UDP)   ││ │  │  (UDP)   ││ │  │  (CAN)   ││
│  └──────────┘│ │  └──────────┘│ │  └──────────┘│
│  ┌──────────┐│ │  ┌──────────┐│ │  ┌──────────┐│
│  │CalManager││ │  │CalManager││ │  │CalManager││
│  │- A2L数据 ││ │  │- A2L数据 ││ │  │- A2L数据 ││
│  └──────────┘│ │  └──────────┘│ │  └──────────┘│
│  ┌──────────┐│ │  ┌──────────┐│ │  ┌──────────┐│
│  │DaqManager││ │  │DaqManager││ │  │DaqManager││
│  │- DAQ配置 ││ │  │- DAQ配置 ││ │  │- DAQ配置 ││
│  └──────────┘│ │  └──────────┘│ │  └──────────┘│
└──────────────┘ └──────────────┘ └──────────────┘
```

### 3.2 核心数据结构

#### 3.2.1 Transport 抽象层

```rust
// src/transport/mod.rs

/// Transport 类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TransportType {
    Udp,
    Can,  // 预留
}

/// Transport 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportConfig {
    Udp { addr: String },
    Can { channel: String, can_id: u32 },  // 预留
}

impl TransportConfig {
    pub fn transport_type(&self) -> TransportType {
        match self {
            TransportConfig::Udp { .. } => TransportType::Udp,
            TransportConfig::Can { .. } => TransportType::Can,
        }
    }
}

/// Transport Trait - 抽象传输层
#[async_trait]
pub trait Transport: Send + Sync {
    /// 发送命令（仅发送，不等待响应）
    async fn send_command_only(&self, command: &[u8]) -> Result<(), TransportError>;

    /// 接收数据包
    async fn recv_packet(&self) -> Result<Vec<u8>, TransportError>;

    /// 关闭连接
    async fn close(&self) -> Result<(), TransportError>;

    /// 获取传输类型
    fn transport_type(&self) -> TransportType;

    /// 获取目标地址标识（用于日志）
    fn target_addr(&self) -> String;
}

/// Transport 工厂
pub struct TransportFactory;

impl TransportFactory {
    pub async fn create(config: &TransportConfig) -> Result<Arc<dyn Transport>, TransportError> {
        match config {
            TransportConfig::Udp { addr } => {
                let transport = UdpTransport::connect(addr).await?;
                Ok(Arc::new(transport))
            }
            TransportConfig::Can { .. } => {
                Err(TransportError::NotImplemented("CAN transport not yet implemented".to_string()))
            }
        }
    }
}
```

#### 3.2.2 Slave ID 和标识

```rust
// src/slave.rs

/// Slave 唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SlaveId(pub Uuid);

impl SlaveId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SlaveId {
    fn default() -> Self {
        Self::new()
    }
}

/// Slave 信息（显示用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaveInfo {
    pub id: SlaveId,
    pub name: String,              // 用户定义的名称
    pub transport_config: TransportConfig,
    pub slave_id_string: Option<String>,  // XCP GET_ID 返回的标识
    pub connected: bool,
    pub connect_info: Option<ConnectInfo>,
}

impl SlaveInfo {
    /// 获取显示名称
    pub fn display_name(&self) -> String {
        if let Some(slave_id) = &self.slave_id_string {
            format!("{} [{}]", self.name, slave_id)
        } else {
            self.name.clone()
        }
    }

    /// 获取地址字符串
    pub fn addr_string(&self) -> String {
        match &self.transport_config {
            TransportConfig::Udp { addr } => addr.clone(),
            TransportConfig::Can { channel, can_id } => {
                format!("CAN {}:{:03X}", channel, can_id)
            }
        }
    }
}
```

#### 3.2.3 XcpSlave 结构体

```rust
// src/slave.rs

/// 单个 Slave 的完整上下文
pub struct XcpSlave {
    /// Slave 信息
    pub info: SlaveInfo,

    /// Transport 层（动态分发）
    transport: Arc<dyn Transport>,

    /// 消息路由器
    router: Arc<MessageRouter>,

    /// CTO 响应接收通道
    cto_rx: Arc<Mutex<mpsc::Receiver<XcpMessage>>>,

    /// 标定管理器（包含该设备的 A2L 数据）
    pub cal_manager: CalibrationManager,

    /// DAQ 管理器（包含该设备的 DAQ 配置）
    pub daq_manager: Arc<Mutex<DaqManager>>,

    /// A2L 文件路径
    a2l_path: Option<PathBuf>,
}

impl XcpSlave {
    /// 创建新的 Slave 连接
    pub async fn connect(
        name: impl Into<String>,
        transport_config: TransportConfig,
    ) -> Result<(Self, SlaveId), XcpError> {
        let id = SlaveId::new();
        let name = name.into();

        // 创建 Transport
        let transport = TransportFactory::create(&transport_config).await?;

        // 创建消息路由器
        let router = Arc::new(MessageRouter::new(100));
        router.reset_counter().await;

        let cto_rx = router.get_cto_receiver();
        let dto_rx = router.get_dto_receiver();

        // 启动路由任务
        let router_clone = router.clone();
        let transport_for_router = transport.clone();
        tokio::spawn(async move {
            router_clone.start(transport_for_router).await;
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let mut slave = Self {
            info: SlaveInfo {
                id,
                name,
                transport_config,
                slave_id_string: None,
                connected: false,
                connect_info: None,
            },
            transport,
            router,
            cto_rx,
            cal_manager: CalibrationManager::new(),
            daq_manager: Arc::new(Mutex::new(DaqManager::new())),
            a2l_path: None,
        };

        // 发送 CONNECT 命令
        let connect_info = slave.send_connect().await?;
        slave.info.connected = true;
        slave.info.connect_info = Some(connect_info);

        // 尝试获取 Slave ID
        if let Ok(slave_id) = slave.get_id(0).await {
            slave.info.slave_id_string = Some(slave_id);
        }

        Ok((slave, id))
    }

    /// 加载 A2L 文件
    pub fn load_a2l<P: AsRef<Path>>(&mut self, path: P) -> Result<(), String> {
        let path = path.as_ref();

        // 加载标定参数
        self.cal_manager = CalibrationManager::from_a2l(path)?;

        // 加载 DAQ 测量量
        let daq_manager = DaqManager::from_a2l(path)?;
        self.daq_manager = Arc::new(Mutex::new(daq_manager));

        self.a2l_path = Some(path.to_path_buf());

        tracing::info!(
            "Loaded A2L for slave '{}': {} cal params, {} measurements",
            self.info.name,
            self.cal_manager.params().len(),
            self.daq_manager.blocking_lock().available_count()
        );

        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&mut self) -> Result<(), XcpError> {
        // 发送 DISCONNECT 命令
        let _ = self.send_command_and_wait(&commands::disconnect()).await;

        // 停止路由器
        self.router.stop().await;

        // 关闭 Transport
        self.transport.close().await?;

        self.info.connected = false;

        Ok(())
    }

    // ... 其他方法（与原来 XcpClient 类似）
}
```

#### 3.2.4 XcpClient 重构为 Manager

```rust
// src/client.rs

/// 多 Slave XCP Client
pub struct XcpClient {
    /// 所有 Slave 连接
    slaves: HashMap<SlaveId, Arc<Mutex<XcpSlave>>>,

    /// 当前活跃的 Slave ID（用于默认操作）
    current_slave: Option<SlaveId>,

    /// Slave 事件广播通道
    event_tx: broadcast::Sender<SlaveEvent>,
}

/// Slave 事件（用于 UI 更新）
#[derive(Debug, Clone)]
pub enum SlaveEvent {
    Connected { id: SlaveId, info: SlaveInfo },
    Disconnected { id: SlaveId },
    A2lLoaded { id: SlaveId, cal_count: usize, meas_count: usize },
    DaqStarted { id: SlaveId },
    DaqStopped { id: SlaveId },
    Error { id: SlaveId, error: String },
}

impl XcpClient {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            slaves: HashMap::new(),
            current_slave: None,
            event_tx,
        }
    }

    /// 连接到新的 Slave
    pub async fn connect_slave(
        &mut self,
        name: impl Into<String>,
        transport_config: TransportConfig,
    ) -> Result<SlaveId, XcpError> {
        let (slave, id) = XcpSlave::connect(name, transport_config).await?;
        let slave_arc = Arc::new(Mutex::new(slave));

        self.slaves.insert(id, slave_arc.clone());

        // 设置为当前 Slave（如果是第一个）
        if self.current_slave.is_none() {
            self.current_slave = Some(id);
        }

        // 广播事件
        let info = slave_arc.lock().await.info.clone();
        let _ = self.event_tx.send(SlaveEvent::Connected { id, info });

        tracing::info!("Connected to slave '{}' with ID {:?}", info.name, id);

        Ok(id)
    }

    /// 断开指定 Slave
    pub async fn disconnect_slave(&mut self, id: SlaveId) -> Result<(), XcpError> {
        if let Some(slave) = self.slaves.remove(&id) {
            let mut slave = slave.lock().await;
            slave.disconnect().await?;

            // 广播事件
            let _ = self.event_tx.send(SlaveEvent::Disconnected { id });

            // 如果断开的是当前 Slave，重新选择
            if self.current_slave == Some(id) {
                self.current_slave = self.slaves.keys().next().copied();
            }
        }

        Ok(())
    }

    /// 获取指定 Slave
    pub fn get_slave(&self, id: SlaveId) -> Option<Arc<Mutex<XcpSlave>>> {
        self.slaves.get(&id).cloned()
    }

    /// 获取当前 Slave
    pub fn current_slave(&self) -> Option<Arc<Mutex<XcpSlave>>> {
        self.current_slave.and_then(|id| self.get_slave(id))
    }

    /// 设置当前 Slave
    pub fn set_current_slave(&mut self, id: SlaveId) -> bool {
        if self.slaves.contains_key(&id) {
            self.current_slave = Some(id);
            true
        } else {
            false
        }
    }

    /// 获取所有 Slave ID
    pub fn slave_ids(&self) -> Vec<SlaveId> {
        self.slaves.keys().copied().collect()
    }

    /// 获取所有 Slave 信息
    pub async fn slave_infos(&self) -> Vec<SlaveInfo> {
        let mut infos = Vec::new();
        for slave in self.slaves.values() {
            let slave = slave.lock().await;
            infos.push(slave.info.clone());
        }
        infos
    }

    /// 为指定 Slave 加载 A2L
    pub async fn load_a2l_for_slave<P: AsRef<Path>>(
        &mut self,
        id: SlaveId,
        path: P,
    ) -> Result<(), String> {
        let slave = self.slaves.get(&id).ok_or("Slave not found")?;
        let mut slave = slave.lock().await;

        slave.load_a2l(path)?;

        // 广播事件
        let cal_count = slave.cal_manager.params().len();
        let meas_count = slave.daq_manager.lock().await.available_count();
        let _ = self.event_tx.send(SlaveEvent::A2lLoaded {
            id,
            cal_count,
            meas_count,
        });

        Ok(())
    }

    /// 订阅事件
    pub fn subscribe_events(&self) -> broadcast::Receiver<SlaveEvent> {
        self.event_tx.subscribe()
    }
}
```

#### 3.2.5 带设备标识的变量

```rust
// src/variable.rs

/// 设备变量标识
#[derive(Debug, Clone)]
pub struct DeviceVariable {
    /// Slave ID
    pub slave_id: SlaveId,
    /// Slave 名称
    pub slave_name: String,
    /// 变量名称
    pub name: String,
    /// 变量类型
    pub var_type: VariableType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VariableType {
    Calibration,
    Measurement,
}

impl DeviceVariable {
    /// 创建显示名称（带设备前缀）
    pub fn display_name(&self) -> String {
        format!("[{}] {}", self.slave_name, self.name)
    }

    /// 创建搜索键
    pub fn search_key(&self) -> String {
        format!("{} {} {}", self.slave_name, self.name, self.var_type.as_str()).to_lowercase()
    }
}

impl VariableType {
    pub fn as_str(&self) -> &'static str {
        match self {
            VariableType::Calibration => "CAL",
            VariableType::Measurement => "DAQ",
        }
    }
}

/// 变量搜索管理器
pub struct VariableSearchManager {
    client: Arc<Mutex<XcpClient>>,
}

impl VariableSearchManager {
    pub fn new(client: Arc<Mutex<XcpClient>>) -> Self {
        Self { client }
    }

    /// 获取所有设备的所有变量
    pub async fn get_all_variables(&self) -> Vec<DeviceVariable> {
        let client = self.client.lock().await;
        let mut variables = Vec::new();

        for (slave_id, slave) in &client.slaves {
            let slave = slave.lock().await;
            let info = &slave.info;

            // 标定变量
            for param in slave.cal_manager.params() {
                variables.push(DeviceVariable {
                    slave_id: *slave_id,
                    slave_name: info.name.clone(),
                    name: param.name.clone(),
                    var_type: VariableType::Calibration,
                });
            }

            // DAQ 测量量
            let daq = slave.daq_manager.lock().await;
            for meas in daq.all_measurements() {
                variables.push(DeviceVariable {
                    slave_id: *slave_id,
                    slave_name: info.name.clone(),
                    name: meas.name.clone(),
                    var_type: VariableType::Measurement,
                });
            }
        }

        variables
    }

    /// 搜索变量（支持按设备、名称、类型过滤）
    pub async fn search(&self, query: &str) -> Vec<DeviceVariable> {
        let query = query.to_lowercase();
        let all = self.get_all_variables().await;

        all.into_iter()
            .filter(|var| var.search_key().contains(&query))
            .collect()
    }
}
```

---

## 4. UI 适配方案

### 4.1 变量列表显示格式

```
┌─ Calibration Parameters ───────────────────────┐
│ [ECU1] engineSpeed          0 rpm     (F32)   │
│ [ECU1] throttlePosition     45.2 %    (F32)   │
│ [ECU2] batteryVoltage       12.4 V    (F32)   │
│ [ECU2] coolantTemp          85.0 °C   (F32)   │
└────────────────────────────────────────────────┘
```

### 4.2 搜索对话框增强

```
┌─ Search Variables ─────────────────────────────┐
│ Filter: [engine                        ]      │
├────────────────────────────────────────────────┤
│ [All] [ECU1] [ECU2] [CAL] [DAQ]               │
├────────────────────────────────────────────────┤
│ ▼ [ECU1] engineSpeed (F32) - CAL              │
│   [ECU1] engineTemp  (F32) - CAL              │
│   [ECU2] engineSpeed (F32) - CAL              │
│   [ECU2] engineTorque(F32) - DAQ              │
├────────────────────────────────────────────────┤
│ [Add Selected] [Cancel]                       │
└────────────────────────────────────────────────┘
```

### 4.3 状态栏显示

```
Status: Connected | Current: ECU1 [172.100.1.5:5555] | Slaves: 2 | DAQ: Running
```

### 4.4 TUI App 修改要点

```rust
// src/tui/app.rs

/// 搜索过滤器
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    All,
    BySlave(SlaveId),
    ByVarType(VariableType),
}

pub struct App {
    // ... 原有字段 ...

    /// 多 Slave 支持
    client: Arc<Mutex<XcpClient>>,

    /// 当前选中的 Slave ID
    current_slave_id: Option<SlaveId>,

    /// 所有 Slave 列表（用于切换）
    slave_list: Vec<SlaveInfo>,
    slave_selected: usize,

    /// 搜索过滤器
    search_filter: FilterType,

    /// 是否显示设备前缀
    show_device_prefix: bool,
}

impl App {
    /// 切换到指定 Slave
    pub async fn switch_slave(&mut self, id: SlaveId) {
        let client = self.client.lock().await;
        if client.set_current_slave(id) {
            self.current_slave_id = Some(id);
            // 重新加载该 Slave 的数据
            if let Some(slave) = client.get_slave(id) {
                let slave = slave.lock().await;
                self.cal_manager = slave.cal_manager.clone();
                self.daq_manager = slave.daq_manager.clone();
            }
        }
    }

    /// 获取带设备前缀的变量名
    pub fn format_variable_name(&self, slave_id: SlaveId, name: &str) -> String {
        if !self.show_device_prefix {
            return name.to_string();
        }

        if let Some(info) = self.slave_list.iter().find(|s| s.id == slave_id) {
            format!("[{}] {}", info.name, name)
        } else {
            name.to_string()
        }
    }
}
```

---

## 5. 实施阶段计划

### 5.1 阶段1: Transport 抽象层 (2-3天)

#### 目标
- 创建 `Transport` trait
- 重构 `UdpTransport` 实现 trait
- 添加 `CanTransport` 骨架

#### 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/transport/mod.rs` | 修改 | 添加 `Transport` trait 和 `TransportConfig` |
| `src/transport/udp.rs` | 修改 | 实现 `Transport` trait |
| `src/transport/can.rs` | 新建 | CAN transport 骨架 |

#### 关键代码

```rust
// src/transport/mod.rs - 新增/修改

pub use transport::{Transport, TransportConfig, TransportType, TransportFactory};
```

#### 验收标准
- [ ] `UdpTransport` 正确实现 `Transport` trait
- [ ] 原有功能不受影响
- [ ] 单元测试通过

---

### 5.2 阶段2: 多 Slave 管理 (3-4天)

#### 目标
- 创建 `XcpSlave` 结构体
- 重构 `XcpClient` 为多 Slave 管理器
- 添加 Slave ID 和事件系统

#### 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/slave.rs` | 新建 | `XcpSlave`、`SlaveId`、`SlaveInfo` |
| `src/client.rs` | 修改 | 重构为多 Slave 管理器 |
| `src/lib.rs` | 修改 | 添加模块导出 |

#### 关键代码

```rust
// src/lib.rs - 修改
pub mod slave;
pub use slave::{XcpSlave, SlaveId, SlaveInfo};
```

#### 验收标准
- [ ] 可以连接多个 Slave
- [ ] 可以断开指定 Slave
- [ ] 可以切换当前 Slave
- [ ] 事件系统正常工作

---

### 5.3 阶段3: 数据隔离 (2-3天)

#### 目标
- 每个 Slave 独立的 A2L 数据
- 变量名添加设备标识
- DAQ 配置按设备隔离

#### 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/variable.rs` | 新建 | `DeviceVariable`、`VariableSearchManager` |
| `src/calibration.rs` | 修改 | 添加设备关联方法 |
| `src/daq.rs` | 修改 | 更新 DAQ manager 使用方式 |

#### 验收标准
- [ ] 每个 Slave 可以加载独立的 A2L
- [ ] 变量搜索可以区分设备
- [ ] DAQ 数据正确归属到设备

---

### 5.4 阶段4: UI 适配 (3-4天)

#### 目标
- 修改变量显示格式（带设备前缀）
- 添加设备选择界面
- 更新搜索对话框

#### 文件变更

| 文件 | 操作 | 说明 |
|------|------|------|
| `src/tui/app.rs` | 修改 | 添加多 Slave 支持 |
| `src/tui/ui.rs` | 修改 | 更新 UI 渲染 |
| `src/tui/event.rs` | 修改 | 添加设备切换快捷键 |

#### 验收标准
- [ ] UI 显示设备前缀
- [ ] 可以切换当前设备
- [ ] 搜索对话框支持设备筛选
- [ ] 所有原有功能正常工作

---

### 5.5 阶段5: 集成测试 (2-3天)

#### 目标
- 端到端功能测试
- 性能测试
- Bug 修复

#### 测试场景

1. **单设备场景**: 连接一个 Slave，验证所有功能正常
2. **多设备场景**: 连接 2-3 个 Slave，验证数据隔离
3. **切换场景**: 在多个设备间切换，验证状态正确
4. **断开重连**: 断开某个设备，验证其他设备不受影响

---

## 6. 依赖变更

### 新增依赖

```toml
[dependencies]
# 已有依赖保持不变
uuid = { version = "1.6", features = ["v4", "serde"] }
async-trait = "0.1"
```

---

## 7. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 向后兼容性破坏 | 高 | 保持旧 API 作为 wrapper，逐步迁移 |
| 多线程竞态条件 | 高 | 使用 `Arc<Mutex<_>>` 模式，充分测试 |
| UI 复杂度增加 | 中 | 提供设备前缀开关，简化显示 |
| 性能下降 | 低 | 使用 HashMap 索引，批量操作 |

---

## 8. 时间表

| 阶段 | 预计时间 | 里程碑 |
|------|----------|--------|
| 阶段1: Transport 抽象 | 2-3天 | Transport trait 完成 |
| 阶段2: 多 Slave 管理 | 3-4天 | 可以连接多个设备 |
| 阶段3: 数据隔离 | 2-3天 | 数据正确隔离 |
| 阶段4: UI 适配 | 3-4天 | UI 显示正常 |
| 阶段5: 集成测试 | 2-3天 | 所有测试通过 |
| **总计** | **12-17天** | |

---

## 9. 代码审查清单

- [ ] 所有 Slave 访问都通过 Mutex
- [ ] 没有阻塞操作在 async 函数中
- [ ] 错误处理完善
- [ ] 日志记录充分
- [ ] 单元测试覆盖新功能
- [ ] UI 响应性能达标

---

## 10. 附录

### 10.1 旧 API 兼容性层

```rust
// 为了保持向后兼容，可以提供简化的单设备 API

impl XcpClient {
    /// 单设备快捷方式（向后兼容）
    pub async fn connect(&mut self, addr: &str) -> Result<ConnectInfo, XcpError> {
        let config = TransportConfig::Udp { addr: addr.to_string() };
        let id = self.connect_slave("Default", config).await?;
        self.set_current_slave(id);

        let slave = self.get_slave(id).unwrap();
        let slave = slave.lock().await;
        Ok(slave.info.connect_info.clone().unwrap())
    }
}
```

### 10.2 配置文件示例

```json
{
  "slaves": [
    {
      "name": "ECU1",
      "transport": {
        "type": "Udp",
        "addr": "172.100.1.5:5555"
      },
      "a2l_path": "ecu1.a2l"
    },
    {
      "name": "ECU2",
      "transport": {
        "type": "Udp",
        "addr": "172.100.1.6:5555"
      },
      "a2l_path": "ecu2.a2l"
    }
  ]
}
```
