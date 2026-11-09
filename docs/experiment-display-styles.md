# 实验卡片显示样式（Display Styles）

> 范围：Tauri 桌面端 `ui/src/components/Experiment.vue` 的变量卡片。
> 已落地两类：**布尔量**（4 种样式）与**数值量**（3 种样式 + 可配置范围/颜色/方向）。
> 架构按“以后还会有别的类型”设计。

## 1. 要解决的问题

布尔量在实验页原来只有一种呈现：`1` / `0` 的大号数字。数值量同样只有一行数字。
对“使能/开关”和“转速/温度/电量”这类量，图形化比数字直观得多。需求拆开是：

1. 同一张卡片可以换**呈现方式**（布尔：文本 / 开关 / 单选 / 灯泡；数值：文本 / 仪表盘 / 电量条），由用户选；
2. 数值量的仪表盘和电量条需要**用户指定显示范围**（A2L 没写 LOWER/UPPER_LIMIT 时必须能手动给）；
3. 电量条还要能选**横向/纵向**、**颜色**、以及分段（电池格子）数量；
4. 选择要**持久化**：刷新不丢（localStorage），保存/加载实验文件也不丢（`.exp.json`）；
5. 标定量在 **Working 页**可以点击直接写值，Reference 页只读；
6. 将来枚举量（分段控件、下拉）也要能加样式，**不能每加一种就在 `Experiment.vue` 里再写一层 `v-if`**。

## 2. 四个设计决定

| 决定 | 理由 |
|---|---|
| **kind 是推导出来的，不落盘** | 变量是不是布尔/数值，取决于 A2L 的 `type` / `min` / `max` / `enum_options`。A2L 会随固件重新生成，把推导结果存进实验文件，重连后就可能变成错的（昨天是 0..1，今天是 0..255）。所以 `resolveValueKind(v)` 每次渲染时现算。 |
| **样式 id 是普通字符串，落盘** | 实验文件里只存 `display_style: "bulb"` / `"gauge"` 这样一个 id。id 不合法（旧文件、被删掉的样式、别人改过的文件）时静默回落到该 kind 的默认样式，绝不抛错。 |
| **样式参数是另一个扁平对象，也落盘** | `v.display_config = { min, max, orientation, color, segments }`，每个键都可缺省。缺省即“用默认值”，所以没动过参数的文件与旧版本输出逐字节相同。 |
| **渲染器是纯组件** | 每个渲染器只接收 `state / text / labels / editable / busy`（数值量再加 `value / config / unit`），发出 `select(next)`。它不知道 XCP、不知道 `invoke`、不知道 working page。写值逻辑只有一份，在 `Experiment.vue` 里。 |

## 3. 模块结构

```
ui/src/experiment/
├── display/
│   ├── kinds.js       # A2L 元数据 → VALUE_KIND（boolean / enum / numeric / map）
│   ├── boolean.js     # 布尔量的取值归一化、标签、写入值
│   ├── numeric.js     # 数值量的取值解析、显示范围、范围占比、配置归一化
│   └── styles.js      # 样式注册表：kind → [{id,label,icon,component,interactive,numeric,options}]
└── components/
    ├── CardDisplay.vue    # 宿主：按 kind+styleId 选渲染器，管点击冒泡与参数透传
    ├── StylePicker.vue    # ◈ 下拉：列出该 kind 的全部样式
    ├── DisplayOptions.vue # ⚙ 下拉：按 styles.js 里的 options 描述渲染参数表单
    ├── BooleanText.vue    # 0/1 文本（默认，兼容旧行为）
    ├── BooleanToggle.vue  # 拨动开关
    ├── BooleanRadio.vue   # 两个单选按钮
    ├── BooleanBulb.vue    # 电灯泡
    ├── NumericGauge.vue   # 240° 圆弧仪表盘
    └── NumericBar.vue     # 电池式电量条（横/纵、可选格子）
```

`Experiment.vue` 只做四件事：算 kind、读/存 `v.display_style` 与 `v.display_config`、
把范围配置算好交给卡片、把 `select` 事件接到写入函数上。

## 4. 数据流

```
A2L 变量对象 v ──resolveValueKind(v)──► kind ──┐
                                              ├─► resolveStyle(kind, v.display_style) ─► 渲染器组件
v.display_style（用户选择，可缺省）───────────┘
                                              ▲
v.display_config（范围/颜色/方向）──resolveNumericConfig(v, config)──► {min,max,color,…}

用户点 ⚙ ──► DisplayOptions ──update:modelValue──► setDisplayConfig(v, cfg) ──► v.display_config ──► saveState()
                                                                                    │
                                                                     watch(tabs, saveState, {deep:true})

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

## 6. 数值量的三种样式

| 样式 id | 外观 | 可配置项（`display_config`） |
|---|---|---|
| `text` | 大号数字（默认，兼容旧行为） | — |
| `gauge` | 240° 圆弧仪表盘，弧长 = 范围占比，两端标注 min/max | `min`、`max`、`color`（弧色） |
| `bar` | 电池式电量条：外壳 + 电量填充 + 可选格子 | `min`、`max`、`orientation`（`horizontal`/`vertical`）、`color`、`segments`（0=整条） |

### 6.1 显示范围如何决定

`numeric.js` 的 `numericRange(v, config)` 按优先级取值：

1. 用户填的 `display_config.min` / `display_config.max`（可只填一个，另一个继续往下找）；
2. A2L 的 `LOWER_LIMIT` / `UPPER_LIMIT`（`v.min` / `v.max`，物理值）——
   标定量本来就有；测量量由后端 `experiment_variables` 一并返回（`Measurement.min/max`，来自 `A2L_MEASUREMENT`）；
3. 都没有时退到 `0 … 100`。

异常处理：`max <= min` 时范围修正为 `min … min+1`，避免除零；`min`/`max` 为 `null`/非有限数的 A2L 视为“没写”。
返回值带 `source: 'user' | 'a2l' | 'default'`，渲染器据此在 `title` 上标注“(auto range)”。

### 6.2 数值从哪来

- 测量量：DAQ 流里的数字（`liveValues`）；
- 标定量：`cal_read` 回读的显示字符串（可能是 `"42.5"`、`"Enabled"`、`"—"`）。

`numericValue(raw)` 统一解析成数字或 `null`（`"—"`、空串、非数字 → `null`）。
卡片上的**文字读数**仍然用原有的格式化结果（`getLiveValue` / `cal_read` 字符串），
渲染器只额外拿一个数字算占比，所以不会出现“卡片文字和小数位数不一致”的问题。

### 6.3 渲染尺寸

- 仪表盘：固定宽 104px（`viewBox 0 0 100 76`），值文本用 HTML 覆盖层居中，避免 SVG 文本换行；
- 电量条：横向 62×19px + 电极，纵向 19×52px + 电极；值文本在右侧（横向）或下方（纵向）。

两者都是只读展示，不参与写值，`interactive: false`。

## 7. 参数弹窗（DisplayOptions）

样式参数不在 `Experiment.vue` 里写死，而是在 `styles.js` 的样式条目上用 `options` 声明：

```js
{ key: 'min',  label: 'Range min', type: 'number', step: 'any', auto: 'min' },
{ key: 'max',  label: 'Range max', type: 'number', step: 'any', auto: 'max' },
{ key: 'orientation', label: 'Orientation', type: 'select',
  choices: [{ value: 'horizontal', label: 'Horizontal' },
            { value: 'vertical',   label: 'Vertical' }], default: 'horizontal' },
{ key: 'color', label: 'Level color', type: 'color', default: '#00ff66' },
{ key: 'segments', label: 'Cells', type: 'number', step: 1, placeholder: 'solid' },
```

`DisplayOptions.vue` 只认识 `number` / `select` / `color` 三种控件：

- `auto: 'min' | 'max'` → 输入框留空时用占位符显示“当前会自动用的边界值”（A2L 或 0/100）；
- 数字框用 `@change`（回车/失焦才提交），避免每敲一个字符就重算范围；
- 清空某个键 = 删除该键 = 回到默认值；有任何一个键被改过时，弹窗底部出现 “Reset to defaults”；
- 该样式没有 `options` 时不渲染 ⚙ 按钮（文本样式就是这种情况）。

## 8. 持久化

### 8.1 会话内（localStorage）

`exp_tauri_state_v3` 里存整个 `tabs` 数组，变量对象上多两个可选字段：

```json
{ "kind": "measurement", "name": "Sig_Rpm", "idx": 1,
  "display_style": "bar", "display_config": { "max": 4000, "orientation": "vertical" } }
```

`watch(tabs, saveState, {deep:true})` 会自动捕获这两个字段的变化，不需要额外监听。

### 8.2 实验文件（`.exp.json`）

保存时版本号 2 → 3，变量条目多一个可选字段：

```json
{
  "version": 3,
  "title": "Fan control",
  "tabs": [{
    "id": "tab-1",
    "title": "T1",
    "variables": [
      { "kind": "calibration", "name": "Cal_EnableFan", "device_id": "dev1",
        "device_name": "ECU1", "display_style": "bulb" },
      { "kind": "measurement", "name": "Sig_Rpm", "device_id": "dev1",
        "device_name": "ECU1", "display_style": "gauge", "display_config": { "max": 4000 } },
      { "kind": "calibration", "name": "Cal_PidKp", "device_id": "dev1",
        "device_name": "ECU1" }
    ]
  }]
}
```

规则：

- **只有非默认值才写**（`...(v.display_style ? {display_style} : {})`、
  `...(Object.keys(v.display_config || {}).length ? {display_config} : {})`）。
  没动过样式的文件与 v1 输出逐字节相同，方便 diff。
- **v1/v2 文件照样能读**：缺 `display_style` → 用该 kind 的默认样式；缺 `display_config` → 用 A2L 范围。
- **未知 id 静默回落**（比如别人用了新版本加的样式，你这边还没有）。
- **切换样式时不清空 `display_config`**：bar→gauge→bar 会记住原来的方向和颜色；
  未被当前样式使用的键只是躺在文件里，不影响渲染（`DisplayOptions` 只关心自己声明的键）。
- 加载时变量对象会按名字+设备从**当前** A2L 重新解析（idx、地址必须来自当前 A2L），
  所以 `display_style` / `display_config` 要单独搬运：`{...found, ...styleCarry(v)}`。
  同一处理也用在启动时的 `remapStaleDeviceIds()`——它会把变量换成后端的新对象，
  不搬就会在重启后把用户的选择冲掉。

## 9. 给别的类型加样式（这是架构的重点）

以枚举量的“分段控件”为例，三步，**不改 `Experiment.vue`**：

```js
// 1. 写渲染器 ui/src/experiment/components/EnumSegmented.vue
//    契约：props { state, text, labels, editable, busy }（需要写值时 emit('select', next)）
//    需要更多输入时在 styles.js 里声明 `numeric: true` 之类的透传标记。

// 2. 注册 ui/src/experiment/display/styles.js
import EnumSegmented from '../components/EnumSegmented.vue'

export const DISPLAY_STYLES = {
  [VALUE_KIND.ENUM]: [
    { id: 'text', label: 'Text', icon: '123', component: BooleanText, interactive: false },
    { id: 'segmented', label: 'Segmented', icon: '⬚', component: EnumSegmented,
      interactive: true, options: [{ key: 'color', label: 'Color', type: 'color' }] },
  ],
}
```

3. 完成。`kindOf()` 已经把带 `enum_options` 的量判成 `enum`，卡片、选择器、参数弹窗、
实验文件自动生效。

要遵守的约定：

- **id 一旦发出去就不能改名**。要改名就新增一个 id，旧 id 当别名保留。
- **每个 kind 的第一个条目就是默认样式**，且必须与 `DEFAULT_STYLE_ID` 一致。
- 新参数只需要在 `options` 里加一条描述，`DisplayOptions.vue` 不用改（除非要一种新控件类型）。

## 10. 验证

- 纯逻辑（kind 判定、取值归一化、写入值、范围解析）不 import 任何 `.vue`，可以独立单测：
  `kinds.js` / `boolean.js` / `numeric.js`。
- 界面验证用浏览器 harness（`ui/harness/`）：

  ```bash
  cd ui && npm run dev:harness     # → http://localhost:5200/harness/index.html
  ```

  它把 `@tauri-apps/api/core`、`@tauri-apps/api/event`、`plugin-dialog`、`vue-router`
  alias 成桩（`vite.harness.config.js`），在浏览器里跑**真实**的 `Experiment.vue`，
  自带一台假 ECU：

  | 变量 | 类型 | 用途 |
  |---|---|---|
  | `Cal_EnableFan` | U8 0..1 | 布尔标定（4 种样式、可点击写值） |
  | `Cal_EnumBool` | 枚举布尔 0/1 | 枚举布尔的标签（Disabled/Enabled） |
  | `Cal_PidKp` | F32 0..100 | 数值标定（仪表盘/电量条 + 范围） |
  | `Flg_Boost` | U8 0..1 | 布尔测量 |
  | `Sig_Rpm` | U16 0..8000 rpm | 数值测量（A2L 有上下限，仪表盘自动 0..8000） |

  harness 还会每 250ms 推一条假的 `daq-data` 事件（`Sig_Rpm` 三角波 + `Flg_Boost` 翻转），
  所以仪表盘/电量条能直接看到跟随变化；控制台里也可以用
  `__emit('daq-data', { device_id: 'dev1', values: { Sig_Rpm: 1234 } })` 手动灌值。
- 生产构建：`cd ui && npm run build`。

## 11. 已知边界

- 布尔量 4 种样式、数值量 3 种样式；`enum` 的注册表位置已经留好但没有条目，
  因此枚举量的卡片不会出现 ◈ 按钮（`hasStyleChoices(kind)` 为 false）。
- 仪表盘/电量条**不自动缩放**：范围来自用户或 A2L，超出范围的值只会被夹到两端。
  A2L 完全没写上下限的测量量默认 `0…100`，需要用户在 ⚙ 里手动指定。
- 范围解析用的是 A2L 的物理上下限；带非线性 COMPU_METHOD 的量，显示值仍是转换后的物理值。
- `booleanWriteValue` 对“非枚举但带非线性 COMPU_METHOD”的布尔按物理值 1/0 写入；
  若某个 A2L 用 RAT_FUNC 把 0/1 映射成别的物理量，需要在 `boolean.js` 里按 raw 写入。
- 样式只影响渲染，不改变 DAQ 采样、记录或 DCM 导出。