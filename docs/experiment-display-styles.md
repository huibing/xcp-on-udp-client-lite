# 实验卡片显示样式（Display Styles）

> 范围：Tauri 桌面端 `ui/src/components/Experiment.vue` 的变量卡片。
> 首个落地类型是**布尔量**（4 种样式），架构按“以后还会有别的类型”设计。

## 1. 要解决的问题

布尔量在实验页原来只有一种呈现：`1` / `0` 的大号数字。对“使能/开关”这类参数，
一个开关或一盏灯比数字更直观，而且标定量在 Working 页可以直接点一下切换。

需求拆开是四件事，架构必须同时满足：

1. 同一张卡片可以换**呈现方式**（文本 / 开关 / 单选 / 灯泡），由用户选；
2. 选择要**持久化**：刷新不丢（localStorage），保存/加载实验文件也不丢（`.exp.json`）；
3. 标定量在 **Working 页**可以点击直接写值，Reference 页只读；
4. 将来数值量（仪表、进度条、滑杆）、枚举量（分段控件、下拉）也要能加样式，
   **不能每加一种就在 `Experiment.vue` 里再写一层 `v-if`**。

## 2. 三个设计决定

| 决定 | 理由 |
|---|---|
| **kind 是推导出来的，不落盘** | 变量是不是布尔，取决于 A2L 的 `type` / `min` / `max` / `enum_options`。A2L 会随固件重新生成，把推导结果存进实验文件，重连后就可能变成错的（昨天是 0..1，今天是 0..255）。所以 `resolveValueKind(v)` 每次渲染时现算。 |
| **样式 id 是普通字符串，落盘** | 实验文件里只存 `display_style: "bulb"` 这样一个 id。id 不合法（旧文件、被删掉的样式、别人改过的文件）时静默回落到该 kind 的默认样式，绝不抛错。 |
| **渲染器是纯组件** | 每个渲染器只接收 `state / text / labels / editable / busy`，发出 `select(next)`。它不知道 XCP、不知道 `invoke`、不知道 working page。写值逻辑只有一份，在 `Experiment.vue` 里。 |

## 3. 模块结构

```
ui/src/experiment/
├── display/
│   ├── kinds.js       # A2L 元数据 → VALUE_KIND（boolean / enum / numeric / map）
│   ├── boolean.js     # 布尔量的取值归一化、标签、写入值
│   └── styles.js      # 样式注册表：kind → [{id,label,icon,component,interactive}]
└── components/
    ├── CardDisplay.vue    # 宿主：按 kind+styleId 选渲染器，管点击冒泡
    ├── StylePicker.vue    # ◈ 下拉：列出该 kind 的全部样式
    ├── BooleanText.vue    # 0/1 文本（默认，兼容旧行为）
    ├── BooleanToggle.vue  # 拨动开关
    ├── BooleanRadio.vue   # 两个单选按钮
    └── BooleanBulb.vue    # 电灯泡
```

`Experiment.vue` 只做三件事：算 kind、读/存 `v.display_style`、把 `select` 事件接到写入函数上。

## 4. 数据流

```
A2L 变量对象 v ──resolveValueKind(v)──► kind ──┐
                                              ├─► resolveStyle(kind, v.display_style) ─► 渲染器组件
v.display_style（用户选择，可缺省）───────────┘

用户点击控件 ──select(next)──► CardDisplay ──► onBooleanSelect(v, next)
                                                   │
                                    canToggleBoolean(v)? ──否──► 忽略（Reference 页 / 未连接 / 非标定量）
                                                   │是
                                                   ▼
                                        writeCalValue(v, booleanWriteValue(v, next))
                                                   │
                                    cal_write → cal_read 回读 → changedParams / calHistory
```

## 5. 布尔量的三种 A2L 约定

真实 A2L 里“布尔”没有统一写法，`kinds.js` 三种都认：

1. `type` 是 `BOOL` / `Bool`（A2L 里显式声明）；
2. 2 个选项、raw 恰好是 0 和 1 的枚举（`COMPU_VTAB`）；
3. 整数类型（`U8`/`U16`/`I8`…）且 `min == 0 && max == 1` —— 手写 A2L 最常见的情况。

排除项：数组类型（`U8[4]`）、`MAP`/`CURVE`/`VAL_BLK` 特征类型、范围不是 0..1 的整数、浮点。

> 注意 `Number(null) === 0`，所以范围判断必须先判空，否则没写 `min/max` 的 U8 会被误判成布尔。

## 6. 交互与写入

- **可点击条件**（`canToggleBoolean`）：`kind === 'calibration'` 且 kind 是布尔 且
  `pageMode === 'working'` 且设备 `connected`。测量量永远是只读的。
- **点击卡片空白处**仍然打开原来的编辑弹窗（精确输入值）；点击开关/灯泡/单选只切换，
  不弹窗——由 `CardDisplay` 在可交互时 `stopPropagation()` 实现。
- **写入值**：枚举布尔写**选项文本**（`cal_write` 会用 COMPU_METHOD 反解成 raw），
  普通布尔写**数字 1/0**，这样 working page 的 HEX 补丁是字节精确的。
- **并发**：每个变量一个 `toggleBusy[key]`，写入中禁用控件，避免连点打出一串 XCP 命令。
- **失败**：`writeCalValue` 返回 `{ok,error}`，失败弹 toast，不改变显示值（值只在回读成功后更新）。

## 7. 持久化

### 7.1 会话内（localStorage）

`exp_tauri_state_v3` 里存整个 `tabs` 数组，变量对象上多一个可选字段：

```json
{ "kind": "calibration", "name": "Cal_EnableFan", "idx": 0, "display_style": "bulb" }
```

`watch(tabs, saveState, {deep:true})` 会自动捕获 `display_style` 的变化，不需要额外监听。

### 7.2 实验文件（`.exp.json`）

保存时版本号 1 → 2，变量条目多一个可选字段：

```json
{
  "version": 2,
  "title": "Fan control",
  "tabs": [{
    "id": "tab-1",
    "title": "T1",
    "variables": [
      { "kind": "calibration", "name": "Cal_EnableFan", "device_id": "dev1",
        "device_name": "ECU1", "display_style": "bulb" },
      { "kind": "calibration", "name": "Cal_PidKp", "device_id": "dev1",
        "device_name": "ECU1" }
    ]
  }]
}
```

规则：

- **只有非默认样式才写**（`...(v.display_style ? {display_style} : {})`）。
  没动过样式的文件与 v1 输出逐字节相同，方便 diff。
- **v1 文件照样能读**：缺 `display_style` → 用该 kind 的默认样式。
- **未知 id 静默回落**（比如别人用了新版本加的样式，你这边还没有）。
- 加载时变量对象会按名字+设备从**当前** A2L 重新解析（idx、地址必须来自当前 A2L），
  所以 `display_style` 要单独搬运：`{...found, ...styleCarry(v)}`。
  同一处理也用在启动时的 `remapStaleDeviceIds()`——它会把变量换成后端的新对象，
  不搬就会在重启后把用户的选择冲掉。

## 8. 给别的类型加样式（这是架构的重点）

以数值量的“进度条”为例，三步，**不改 `Experiment.vue`**：

```js
// 1. 写渲染器 ui/src/experiment/components/NumericBar.vue
//    契约：props { state, text, labels, editable, busy }，需要写值时 emit('select', next)
//    只读展示则忽略 editable，不 emit。

// 2. 注册 ui/src/experiment/display/styles.js
import NumericBar from '../components/NumericBar.vue'

export const DISPLAY_STYLES = {
  [VALUE_KIND.NUMERIC]: [
    { id: 'text', label: 'Text', icon: '123', component: BooleanText, interactive: false },
    { id: 'bar',  label: 'Bar',  icon: '▭',   component: NumericBar,  interactive: false },
  ],
}
```

3. 完成。`kindOf()` 已经把 F32/U16 判成 `numeric`，卡片、选择器、实验文件自动生效。

要遵守的两条约定：

- **id 一旦发出去就不能改名**。要改名就新增一个 id，旧 id 当别名保留。
- **每个 kind 的第一个条目就是默认样式**，且必须与 `DEFAULT_STYLE_ID` 一致。

## 9. 验证

- 纯逻辑（kind 判定、取值归一化、写入值）用 node 直接跑，无依赖：
  `kinds.js` / `boolean.js` 不 import 任何 `.vue`，可以独立单测。
- 界面验证用浏览器 harness（`ui/harness/`）：

  ```bash
  cd ui && npm run dev:harness     # → http://localhost:5200/harness/index.html
  ```

  它把 `@tauri-apps/api/core`、`@tauri-apps/api/event`、`plugin-dialog`、`vue-router`
  alias 成桩（`vite.harness.config.js`），在浏览器里跑**真实**的 `Experiment.vue`，
  自带一台假 ECU（`Cal_EnableFan` 0..1、`Cal_EnumBool` 枚举布尔、`Cal_PidKp` F32、`Flg_Boost` 测量量），
  可以验证选择器、点击写入、保存/加载往返，不需要 Tauri 外壳，也不需要硬件。
- 生产构建：`cd ui && npm run build`。

## 10. 已知边界

- 布尔量目前只给了 4 种样式；`enum` / `numeric` 的注册表位置已经留好但没有条目，
  因此非布尔量的卡片不会出现 ◈ 按钮（`hasStyleChoices(kind)` 为 false）。
- `booleanWriteValue` 对“非枚举但带非线性 COMPU_METHOD”的布尔按物理值 1/0 写入；
  若某个 A2L 用 RAT_FUNC 把 0/1 映射成别的物理量，需要在 `boolean.js` 里按 raw 写入。
- 样式只影响渲染，不改变 DAQ 采样、记录或 DCM 导出。