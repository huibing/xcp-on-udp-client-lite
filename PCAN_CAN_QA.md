# PCAN / CAN Transport 问答记录

本文记录本次会话中关于「在 xcp-on-udp-client-lite 中支持 PCAN、CAN 传输层与设备驱动」的原始问题与最终答案。

---

## 1. `PCAN_BASIC_DLL` 是 dll 所在目录，还是包含 dll 的完整路径？

**原始问题**

> PCAN_BASIC_DLL是dll所在的路径还是包含dll的全部path

**最终答案**

最初实现要求是**包含文件名的完整路径**，因为它被直接交给 `LoadLibrary`。现已增强为**两者都支持**（`expand_dll_candidate`）：

- 值是一个**已存在的目录** → 自动追加 `\PCANBasic.dll`；
- 值以 `.dll` 结尾 → 原样使用；
- 其他（无法判断）→ 先当文件试，再当目录拼 `PCANBasic.dll` 试。

例如以下两种写法均可：

```
PCAN_BASIC_DLL=E:\workspace\c\PCANBasic\x64\PCANBasic.dll
PCAN_BASIC_DLL=E:\workspace\c\PCANBasic\x64
```

未设置时的查找顺序：可执行文件同目录 → `E:\workspace\c\PCANBasic\x64` / `x86` → 系统 PATH。

---

## 2. 波特率 / CAN / CAN-FD 能否从 A2L 读出并让上层直接传递？

**原始问题**

> 波特率 can或者can fd这些信息应该可以从a2l中读取到 能否让上层直接传递而不再自己尝试

**最终答案**

可以，已改为「上层显式传递，传输层不再自行探测」：

- A2L 解析（`XCP_ON_CAN` 块）：
  - `BAUDRATE` → 仲裁/标称波特率（bit/s）
  - `CAN_FD_DATA_TRANSFER_BAUDRATE`（兼容 `CAN_FD_DATA_BAUDRATE` / `DATA_BAUDRATE`）→ 数据相位波特率，**存在即表示 CAN-FD**
  - 顺带解析 `CAN_ID_SLAVE`、`DAQ_LIST_CAN_ID`、`MAX_DLC_REQUIRED`
- 链路：`A2lTransportConfig::Can{baudrate,data_baudrate}` → `TransportConfig::Can` → `CanTransportConfig` → 驱动层。
- 初始化策略（不再“先试 FD 再回退经典”）：
  - `data_baudrate` 有值 → CAN-FD 初始化；
  - 仅 `baudrate` → 经典 CAN（bps 映射到 PCAN BTR0/BTR1）；
  - 都没有 → 回退 `PCAN_BITRATE` 环境变量，再回退经典 500k。
- CAN-FD 时序由仲裁/数据 bps 推导（默认 80 MHz 时钟、约 80% 采样点，`PCAN_FD_CLOCK` 可覆盖）。

---

## 3. 与 UDP 对比，检查 tauri 链路上用 PCAN 操作 XCP 是否有问题？

**原始问题**

> 和udp对比一下 检查一下can transport实现之后 tauri app的软件链路上使用pcan设备操作xcp是否会有问题

**最终答案**

链路本身是通的：`A2L → TransportConfig → CanTransportConfig → CanTransport`，`MessageRouter` 按 PID（≥0xFC→CTO，<0xFC→DTO）分流，命令走 `send_command_only` + `cto_rx`；DAQ 识别字段默认 `AbosulteODT`，与 XCP-on-CAN 一致。

与 UDP 的关键差异：

| 维度 | UDP | CAN |
|---|---|---|
| 长度 | 报文自带 LEN 字段（精确） | 靠 CAN DLC，无长度字段 |
| 寻址 | 每从机独立 socket（单播） | CAN ID + 共享总线 |
| 接收过滤 | 内核只给本 socket | 容易收到总线上全部帧 |
| 多从机 | 多 socket 天然隔离 | 每 transport 独占一个 PCAN 通道 |
| 填充 | 无 | 可能按 DLC 填充 |

发现的隐患（当时）：

- **P0** 接收未按 CAN ID 过滤 → 共享总线其它报文污染 XCP 流。
- **P1** 同一 PCAN 通道独占 → tauri 多 CAN 设备时第二个连接失败/争抢接收队列。
- **P1** `MAX_DLC_REQUIRED` 未处理 → 短帧可能被从机丢弃。
- **P1** `CAN_ID_SLAVE` 未解析 → 无法过滤/校验。
- **P2** 响应长度=DLC，可能带入填充字节（`read_memory`/`GET_ID` 会多出数据）。
- **P2** CAN-FD 时序由 bps 推导，未采用 A2L `SAMPLE_POINT`。
- **P2** 通道固定 `auto`，无通道选择入口。
- **P3** 忙轮询、无总线状态上报；EV 归类等。

随后按优先级修复了 P0/P1（见下方第 4、9 节与对应提交）。

---

## 4. `cargo tauri dev` 报错（`A2lTransportConfig::Can` 缺少字段）

**原始问题（报错）**

```
error[E0027]: pattern does not mention fields `baudrate`, `data_baudrate`
  --> src-tauri\src\commands\a2l.rs:15:9
  --> src-tauri\src\state.rs:380:9
```

**最终答案**

`src-tauri` 是独立 workspace 成员，也有对 `A2lTransportConfig::Can` 的匹配，需要同步。两处都改为：

```rust
A2lTransportConfig::Can { can_id, .. } => format!("CAN 0x{:X}", can_id),
```

`cargo check -p xcp-client-desktop --offline` 通过。

---

## 5. PCAN Basic API 中有列出所有设备及通道的 API 吗？

**原始问题**

> pcan basic api中有list所有设备及通道的api吗

**最终答案**

有，但没有单独的“列出所有设备”函数，是通过 `CAN_GetValue` 的两个参数实现的：

1. **枚举所有已接入通道**（`PCANBasic.h:163-164`）
   ```c
   CAN_GetValue(PCAN_NONEBUS, PCAN_ATTACHED_CHANNELS_COUNT, &count, sizeof(DWORD));
   CAN_GetValue(PCAN_NONEBUS, PCAN_ATTACHED_CHANNELS, buffer, count * sizeof(TPCANChannelInformation));
   ```
   返回 `TPCANChannelInformation`：`channel_handle / device_type / controller_number / device_features / device_name[33] / device_id / channel_condition`。

2. **按条件查找单个通道**：`CAN_LookUpChannel("devicetype=PCAN_USB", &handle)`，支持键 `devicetype/deviceid/controllernumber/ipaddress/deviceguid`，只返回第一个匹配。

3. **单通道可用性**：`CAN_GetValue(handle, PCAN_CHANNEL_CONDITION, &cond, 4)` → `UNAVAILABLE/OCCUPIED/AVAILABLE/PCANVIEW`。

注意：没有单独“列出设备驱动/型号”的 API，设备型号只能从每个通道的 `device_type` / `device_name` 得到；它反映当前已接入的通道，DLL/服务不可用时列表为空。

---

## 6. 如何获取某个 channel 的 handle，使用什么 API？

**原始问题**

> 如何获取某个channel 的handle 使用什么api

**最终答案**

handle 就是传给 `CAN_Initialize` 的 16 位 `TPCANHandle`（如 `PCAN_USBBUS1 = 0x51`）。三种获取方式：

1. **枚举**：`CAN_GetValue(PCAN_NONEBUS, PCAN_ATTACHED_CHANNELS, ...)`，每个条目的 `channel_handle` 即 handle。
2. **按条件查**：`CAN_LookUpChannel("devicetype=PCAN_USB,controllernumber=1", &h)` 等。
3. **已知常量/名字映射**：handle 编码固定——
   - PCI：`0x41..0x48`（bus1-8）、`0x409..0x410`（bus9-16）
   - USB：`0x51..0x58`、`0x509..0x510`
   - LAN：`0x801..0x808`、`0x809..0x810`
   没有“字符串 → handle”的 API，需要自己换算（代码里 `parse_channel_handle` / `channel_handle_name`）。

拿到 handle 后可反查：`PCAN_CHANNEL_CONDITION`(0x0D)、`PCAN_HARDWARE_NAME`(0x0E)、`PCAN_CHANNEL_VERSION`(0x06)、`PCAN_DEVICE_ID`(0x01)、`PCAN_IP_ADDRESS`(0x1C)。

---

## 7. 插 3 个 PCAN 设备时 channel list 长什么样？用户选择如何映射到 handle？

**原始问题**

> 假如用户插了3个PCAN设备  channel list是怎么样的呢 获取到每个channel handle之后 如何将用户的选择映射到handle变量上呢

**最终答案**

`PCAN_ATTACHED_CHANNELS` 返回的是**通道（控制器）列表**，不是物理设备列表：

- 3 × 单通道（普通 PCAN-USB）：3 条 → `0x51/0x52/0x53`（`PCAN_USBBUS1/2/3`），`device_id` 各不相同。
- 3 × 双通道（PCAN-USB Pro / FD Pro）：6 条 → `0x51..0x56`，同一 `device_id` 的两条对应同一台设备的两个控制器（`controller_number` 0/1）。

> USB1/2/3 编号由驱动按控制器序号分配，**重插/重启后可能变化**；稳定标识是 `device_id`（或 USB 的 `device_guid`）。

映射到 handle 的两种策略：

1. **按 handle/名字（会话内可靠）**：枚举条目已带 `channel_handle`，用户选择后取其 handle（或名字回推），`CAN_Initialize(handle, ...)`。
2. **按 `device_id` / `device_guid`（推荐，重插后仍对得上）**：
   ```c
   CAN_LookUpChannel("deviceid=3,controllernumber=1", &h);
   // 或 CAN_LookUpChannel("deviceguid={...}", &h);
   CAN_Initialize(h, ...);
   ```

---

## 8. 扩展成设备级 + 通道级，按 `device_id` 定位

**原始问题**

> 好的 扩展成设备级加通道级 按照device_id 定位

**最终答案**

已实现：

- `CanDeviceInfo` 拆成两层：
  - 设备级：`driver`、`device_id`、`name`、`device_type`
  - 通道级：`controller`、`channel`
  - 选择值 `selection_value() = "driver:device_id:controller"`（稳定，不受句柄重编号影响）；`display_label()` 供 UI。
- `CanDeviceConfig` / `CanTransportConfig` / `TransportConfig::Can` 新增 `device_id`、`controller`。
- PCAN 驱动 `open` 解析句柄优先级：
  1. 显式 `channel` 名/句柄；
  2. `device_id` + controller → `CAN_LookUpChannel("deviceid=..,controllernumber=..")`；
  3. 第一个可用通道。
- `list_channels` 从 `TPCANChannelInformation` 填 `device_id`/`controller`/`channel`/`available`，并新增 `PcanApi::get_device_id()`（`PCAN_DEVICE_ID`）。
- Tauri `list_can_devices` 返回 `device_id/controller/label/value`；`parse_can_selection` 支持 `driver:device_id:controller`、`driver:channel`、`channel`；`apply_can_selection` 结构化时写 `device_id`+`controller` 并把 `channel` 置 `auto`。
- Vue CAN 下拉显示后端 `label`（含设备/通道/device_id、`[FD]`、`(in use)`），`value` 即结构化 token。

---

## 9. 软件架构重构：区分标定设备驱动与 CAN transport 层，并支持搜索 CAN 设备

**原始问题**

> 需要一次软件架构重构 不涉及新的功能但是要理顺标定设备驱动和can transport层之间的区别  PCAN Basic是设备驱动 不能在can.rs中实现，将来会有更多的can设备驱动支持，需要抽象一个can device trait来为未来的设备驱动开发做准备。 另外，tauri app中需要能够搜索当前的can设备 并告诉用户当前支持哪些can 设备 能查到当前已连接的can设备并将用户选的通道传递给下面的设备驱动层

**最终答案**

已重构为三层：

```
src/transport/can/
├── mod.rs      # CanTransport —— XCP-on-CAN 传输层（分帧/ID过滤/MAX_DLC_REQUIRED/异步队列）
├── device.rs   # CanDevice / CanDriver trait + 通用类型 + 驱动注册表
└── pcan.rs     # PcanDriver / PcanDevice —— PCAN-Basic 设备驱动
```

- `device.rs`：`CanFrame`、`CanDeviceInfo`、`CanDeviceConfig`、`CanDeviceError`；`trait CanDevice{ send_frame / recv_frame / close / info / is_fd / bitrate_description }`；`trait CanDriver{ name / description / list_channels / open / default_channel }`；注册表 `drivers() / find_driver() / driver_names() / list_can_devices()`。
- `pcan.rs`：全部 PCANBasic FFI、DLL 加载、波特率/时序、通道枚举与解析，实现上述 trait。
- `mod.rs`：`CanTransport` 只依赖 `Box<dyn CanDevice>`，负责 XCP 协议层。

扩展新硬件：实现 `CanDriver`+`CanDevice` 并在 `device::drivers()` 注册即可，传输层无需改动。

Tauri / Vue：

- 新增命令 `list_can_devices`（返回已编译驱动 + 当前可枚举的 CAN 通道）。
- `connect_device` / `connect_all_devices` 接受 CAN 选择（`driver:device_id:controller`）并下发到设备驱动层。
- `DeviceManager.vue` 在设备选中 "can" 时显示 CAN 设备/通道下拉，选择随连接一起传递。

---

## 相关提交

| 提交 | 内容 |
|---|---|
| `2b199e1` | feat(transport): 用 PCAN 实现 XCP on CAN |
| `2422086` | feat(transport): 从 A2L 传递波特率与 CAN-FD 模式到 PCAN |
| `cf10fc9` | feat(transport): 按 CAN ID 过滤并处理 MAX_DLC_REQUIRED |
| `5f2fe01` | refactor(transport): 拆分 CAN 设备驱动与传输层，新增设备级选择 |
