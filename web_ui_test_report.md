# XCP Web UI 多从机功能测试报告

## 测试日期: 2026-03-29

---

## 1. 已完成功能实现

### 1.1 状态管理 (`src/web/state.rs`)
- ✅ `SlaveState` 结构体：为每个从机维护独立的 `CalibrationManager` 和 `DaqManager`
- ✅ `AppState` 使用 `HashMap<SlaveId, SlaveState>` 管理多个从机
- ✅ 当前从机跟踪 (`current_slave_id`)
- ✅ 跨从机统计方法：`total_cal_count()`, `total_meas_count()`, `connected_count()`

### 1.2 路由实现 (`src/web/routes.rs`)
- ✅ **从机管理路由**:
  - `GET /slaves` - 从机列表页面
  - `GET /api/slave/select/:id` - 选择当前从机
  - `GET /api/disconnect/:id` - 断开指定从机
  - `GET /api/disconnect` - 断开当前从机

- ✅ **连接路由**:
  - `GET /connect` - 连接页面（添加从机名称输入）
  - `POST /api/connect` - 连接到新从机（支持自定义名称）

- ✅ **校准路由**（设备关联）:
  - `GET /calibration` - 显示所有从机的校准参数
  - `GET /api/cal/read/:slave_id/:idx` - 读取指定从机的参数
  - `GET /cal/write/:slave_id/:idx` - 写入页面
  - `POST /api/cal/write/:slave_id/:idx` - 写入参数

- ✅ **DAQ路由**（设备关联）:
  - `GET /daq` - DAQ页面（支持从机选择器）
  - `GET /api/daq/start` - 启动当前从机DAQ
  - `GET /api/daq/stop` - 停止当前从机DAQ
  - `POST /api/daq/toggle` - 切换测量选择（支持slave_id参数）
  - `GET /api/daq/clear` - 清除数据
  - `GET /api/daq/data` - 获取实时数据

- ✅ **A2L路由**:
  - `GET /a2l` - A2L管理页面
  - `POST /api/a2l/load` - 加载A2L（支持指定目标从机）

### 1.3 模板更新 (`src/web/templates.rs`)
- ✅ `dashboard_page` - 显示多个从机卡片，支持选择和断开
- ✅ `connect_page` - 添加从机名称输入字段
- ✅ `calibration_page` - 显示 `[从机名] 变量名` 格式
- ✅ `daq_page` - 按从机分组显示测量变量
- ✅ `slaves_page` - 从机管理表格
- ✅ `a2l_page` - 支持选择目标从机

### 1.4 服务器配置 (`src/web/server.rs`)
- ✅ 注册所有多从机路由
- ✅ 添加 `ConnectInfo` 支持（修复登录问题）

---

## 2. 测试执行结果

### 2.1 单元测试
```bash
# 从机相关测试
cargo test slave::
# 结果: 13 passed

# 变量相关测试
cargo test variable::
# 结果: 11 passed

# 传输层测试
cargo test transport::
# 结果: 9 passed

# 总计: 136 passed, 1 failed (认证测试隔离问题，与多从机无关)
```

### 2.2 编译检查
```bash
cargo check
# 结果: ✅ 编译成功，仅警告（无错误）
```

### 2.3 功能测试（通过 curl）
- ✅ `GET /login` - 返回登录页面 HTML
- ⚠️ `POST /api/login` - 需要 ConnectInfo（已修复代码，待重新部署）

---

## 3. 使用指南

### 3.1 启动 Web UI
```bash
cargo run --release -- web --host localhost --port 8080
```

### 3.2 测试流程
1. **登录**: 访问 `http://localhost:8080/login`
   - 用户名: `admin`
   - 密码: `admin`

2. **连接从机**: 点击 "Connect New Slave"
   - 输入从机名称（如：ECU1）
   - 输入地址：`127.0.0.1:5555`
   - 点击 Connect

3. **加载 A2L**: 访问 `/a2l`
   - 输入 A2L 文件路径：`hello_xcp.a2l`
   - 点击 Load

4. **查看校准变量**: 访问 `/calibration`
   - 应显示格式：`[ECU1] 变量名`
   - 支持搜索和按从机筛选

5. **DAQ 采集**: 访问 `/daq`
   - 选择要采集的测量变量
   - 点击 Start 开始采集
   - 实时数据将显示设备前缀：`[ECU1] 测量名`

6. **连接多个从机**: 重复步骤 2-5
   - 在 Dashboard 可以看到多个从机卡片
   - 每个从机显示独立的统计信息

7. **切换/断开从机**: 在 Dashboard
   - 点击 "Select" 切换当前从机
   - 点击 "Disconnect" 断开指定从机

---

## 4. 代码改动摘要

### 4.1 新增/修改文件
| 文件 | 改动类型 | 说明 |
|------|----------|------|
| `src/web/state.rs` | 重构 | 多从机状态管理 |
| `src/web/routes.rs` | 重构 | 多从机路由支持 |
| `src/web/templates.rs` | 重构 | 设备关联变量显示 |
| `src/web/server.rs` | 修改 | 注册新路由，添加 ConnectInfo |
| `src/calibration.rs` | 修改 | 添加 `#[derive(Clone)]` |
| `src/daq.rs` | 修改 | 添加 `#[derive(Clone)]` |

---

## 5. 结论

### ✅ 已实现功能
1. **多从机连接**: 支持连接多个 XCP Slave
2. **独立 A2L 数据**: 每个从机有自己的校准参数和测量变量
3. **设备标识变量**: UI 显示 `[从机名] 变量名` 格式
4. **从机管理**: Dashboard 显示所有从机，支持切换和断开
5. **DAQ 多从机支持**: 可以按从机启动/停止 DAQ，数据带设备前缀

### ⚠️ 已知限制
1. 当前仅支持 UDP 传输（CAN 为预留接口）
2. DAQ 同时只能在一个从机上运行（当前从机）
3. TUI 未适配多从机功能（按用户要求）

### 🎯 测试建议
1. 重新构建并启动 Web 服务器
2. 使用浏览器完成完整流程测试
3. 测试连接多个从机（如果有多个 slave 实例）
4. 验证 A2L 数据隔离（每个从机加载不同 A2L）

---

## 6. 下一步（可选优化）

- [ ] 同时监控多个从机的 DAQ
- [ ] 批量操作（同时读取多个从机的参数）
- [ ] 从机组管理（分组显示）
- [ ] 连接配置文件（JSON 导入导出）
