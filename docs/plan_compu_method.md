# 标定变量 COMPU_METHOD 转换实现计划

## 1. 需求分析

### 1.1 问题描述
当前标定变量的显示没有经过 compute method 转换：

- 枚举量（如 `SLC_LC_SD_stLcStMan_c`）显示为原始数字值而不是对应的枚举文字
- Web 界面修改值时需要下拉菜单选择枚举值，而不是直接输入数字

### 1.2 A2L 中的 COMPU_METHOD 示例

```a2l
/* 布尔值转换方法 */
/begin COMPU_METHOD BOOL ""
    TAB_VERB "%.0" "" COMPU_TAB_REF BOOL.table
/end COMPU_METHOD
/begin COMPU_VTAB BOOL.table "" TAB_VERB 2
    0 "false" 1 "true"
/end COMPU_VTAB

/* 枚举状态转换方法示例 */
/begin COMPU_METHOD SLC_LC_SD_stLcStMan_c "" TAB_VERB COMPU_TAB_REF SLC_LC_SD_stLcStMan_c.table /end COMPU_METHOD
/begin COMPU_VTAB SLC_LC_SD_stLcStMan_c.table "" TAB_VERB 5
    0 "LC_MAN_OFF"
    1 "LC_MAN_ON"
    2 "LC_MAN_AUTO"
    3 "LC_MAN_FAULT"
    4 "LC_MAN_RESERVED"
/end COMPU_VTAB
```

## 2. 技术架构

### 2.1 新的模块结构

```
src/
├── a2l/                          # 新增：A2L 解析模块
│   ├── mod.rs                    # 模块导出（从 a2l_reader.rs 迁移）
│   ├── reader.rs                 # A2L 文件读取（从 a2l_reader.rs 迁移）
│   ├── types.rs                  # A2L 数据类型定义
│   └── compute_method.rs         # 新增：COMPU_METHOD 转换核心
│
├── calibration.rs                # 修改：使用 a2l::compute_method
├── daq.rs                        # 修改：使用 a2l::compute_method
├── web/
│   ├── routes.rs                 # 修改：适配新的 CompuMethod API
│   └── templates/
│       └── calibration.hbs       # 修改：支持下拉菜单
│
└── lib.rs                        # 修改：更新模块声明
```

### 2.2 模块依赖关系

```
┌─────────────────────────────────────────────────────────────┐
│                        a2l 模块                              │
│  ┌──────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │    types     │  │  compute_method  │  │    reader    │  │
│  │  A2lDataType │◄─┤   CompuMethod    │◄─┤  A2lReader   │  │
│  │CompuVtabEntry│  │convert_to_display│  │ parse_a2l()  │  │
│  └──────────────┘  │ convert_to_raw() │  └──────────────┘  │
│                    └────────┬─────────┘                    │
└─────────────────────────────┼───────────────────────────────┘
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
    ┌───────────────────┐         ┌───────────────────┐
    │    calibration    │         │       daq         │
    │  CalibrationParam │         │ DaqMeasurement    │
    │  .format_value()  │         │ .convert_bytes()  │
    │  .parse_value()   │         │                   │
    └─────────┬─────────┘         └─────────┬─────────┘
              │                             │
              └───────────────┬─────────────┘
                              ▼
                    ┌───────────────────┐
                    │   web/routes.rs   │
                    │  /api/cal/value   │
                    │  /api/cal/enum    │
                    └───────────────────┘
```

## 3. 实现步骤

### 步骤 1: 创建 a2l 模块文件夹

**新建文件夹**: `src/a2l/`

**文件迁移与创建**:
1. 将 `src/a2l_reader.rs` → `src/a2l/reader.rs`
2. 新建 `src/a2l/types.rs` - 从 reader.rs 提取类型定义
3. 新建 `src/a2l/compute_method.rs` - COMPU_METHOD 核心实现
4. 新建 `src/a2l/mod.rs` - 模块统一导出

### 步骤 2: 创建 a2l/compute_method.rs

**职责**: 定义 COMPU_METHOD 转换接口，与 A2L 解析解耦

```rust
//! COMPU_METHOD 转换模块
//!
//! 提供原始值与显示值之间的双向转换
//! 支持类型：IDENTICAL, LINEAR, RATIONAL, TAB_VERB, COMPU_VTAB

use std::collections::HashMap;

/// COMPU_METHOD 转换方法
#[derive(Debug, Clone)]
pub enum CompuMethod {
    /// 无转换：physical = raw
    Identical,
    /// 线性转换：physical = factor * raw + offset
    Linear { factor: f64, offset: f64 },
    /// 有理函数转换：physical = (a * raw + b) / (c * raw + d)
    Rational { a: f64, b: f64, c: f64, d: f64 },
    /// 数值表插值（连续值）
    TabVerb { pairs: Vec<(f64, f64)> },
    /// 枚举值表（离散值）- 新增
    CompuVtab {
        entries: Vec<CompuVtabEntry>,
        raw_to_text: HashMap<i64, String>,
        text_to_raw: HashMap<String, i64>,
    },
}

/// 枚举表条目
#[derive(Debug, Clone)]
pub struct CompuVtabEntry {
    pub raw_value: i64,
    pub text_value: String,
}

impl CompuMethod {
    /// 创建 IDENTICAL 转换方法
    pub fn identical() -> Self {
        Self::Identical
    }

    /// 创建 LINEAR 转换方法
    pub fn linear(factor: f64, offset: f64) -> Self {
        Self::Linear { factor, offset }
    }

    /// 创建 COMPU_VTAB 枚举转换方法
    pub fn compu_vtab(entries: Vec<(i64, String)>) -> Self {
        let mut raw_to_text = HashMap::new();
        let mut text_to_raw = HashMap::new();
        let entries: Vec<CompuVtabEntry> = entries
            .into_iter()
            .map(|(raw, text)| {
                raw_to_text.insert(raw, text.clone());
                text_to_raw.insert(text.clone(), raw);
                CompuVtabEntry {
                    raw_value: raw,
                    text_value: text,
                }
            })
            .collect();

        Self::CompuVtab {
            entries,
            raw_to_text,
            text_to_raw,
        }
    }

    // ==================== 核心转换接口 ====================

    /// 将原始值转换为显示值（整数输入，字符串输出）
    ///
    /// # 参数
    /// - `raw`: 从 ECU 读取的原始整数值
    ///
    /// # 返回
    /// 转换后的显示字符串（枚举文字或格式化数字）
    pub fn to_display(&self, raw: i64) -> String {
        match self {
            Self::Identical => raw.to_string(),
            Self::Linear { factor, offset } => {
                let phys = *factor * raw as f64 + *offset;
                format!("{:.6}", phys)
            }
            Self::Rational { a, b, c, d } => {
                let denom = c * raw as f64 + d;
                if denom.abs() < f64::EPSILON {
                    raw.to_string()
                } else {
                    let phys = (a * raw as f64 + b) / denom;
                    format!("{:.6}", phys)
                }
            }
            Self::TabVerb { pairs } => {
                // 线性插值逻辑
                let raw_f = raw as f64;
                for i in 0..pairs.len().saturating_sub(1) {
                    if raw_f >= pairs[i].0 && raw_f <= pairs[i + 1].0 {
                        let t = (raw_f - pairs[i].0) / (pairs[i + 1].0 - pairs[i].0);
                        let phys = pairs[i].1 + t * (pairs[i + 1].1 - pairs[i].1);
                        return format!("{:.6}", phys);
                    }
                }
                pairs.first().map(|p| p.1.to_string()).unwrap_or_else(|| raw.to_string())
            }
            Self::CompuVtab { raw_to_text, .. } => {
                raw_to_text.get(&raw).cloned().unwrap_or_else(|| raw.to_string())
            }
        }
    }

    /// 将原始字节转换为显示值（用于 DAQ 实时数据）
    ///
    /// # 参数
    /// - `bytes`: 原始字节数据
    /// - `is_float`: 是否为浮点类型（浮点类型不应用转换）
    pub fn bytes_to_display(&self, bytes: &[u8], is_float: bool) -> f64 {
        if is_float {
            // 浮点类型直接解析，不应用 COMPU_METHOD
            return parse_float_bytes(bytes);
        }

        let raw = parse_int_bytes(bytes) as i64;
        match self {
            Self::Identical => raw as f64,
            Self::Linear { factor, offset } => *factor * raw as f64 + *offset,
            Self::Rational { a, b, c, d } => {
                let denom = c * raw as f64 + d;
                if denom.abs() < f64::EPSILON {
                    raw as f64
                } else {
                    (a * raw as f64 + b) / denom
                }
            }
            Self::TabVerb { pairs } => {
                let raw_f = raw as f64;
                for i in 0..pairs.len().saturating_sub(1) {
                    if raw_f >= pairs[i].0 && raw_f <= pairs[i + 1].0 {
                        let t = (raw_f - pairs[i].0) / (pairs[i + 1].0 - pairs[i].0);
                        return pairs[i].1 + t * (pairs[i + 1].1 - pairs[i].1);
                    }
                }
                pairs.first().map(|p| p.1).unwrap_or(raw as f64)
            }
            Self::CompuVtab { .. } => raw as f64, // 枚举值返回原始值用于趋势图
        }
    }

    /// 将显示值/枚举文字转换为原始值
    ///
    /// # 参数
    /// - `display`: 用户输入的显示值或枚举文字
    ///
    /// # 返回
    /// 转换后的原始整数值
    pub fn to_raw(&self, display: &str) -> Result<i64, String> {
        match self {
            Self::Identical => display
                .parse()
                .map_err(|_| format!("'{}' is not a valid number", display)),
            Self::Linear { factor, offset } => {
                let phys: f64 = display
                    .parse()
                    .map_err(|_| format!("'{}' is not a valid number", display))?;
                let raw = ((phys - *offset) / *factor).round() as i64;
                Ok(raw)
            }
            Self::Rational { .. } => {
                // 有理函数逆向求解较复杂，先返回错误
                Err("Rational conversion reverse not supported".to_string())
            }
            Self::TabVerb { .. } => {
                Err("TabVerb conversion reverse not supported".to_string())
            }
            Self::CompuVtab { text_to_raw, .. } => {
                // 先尝试匹配枚举文字
                if let Some(&raw) = text_to_raw.get(display) {
                    return Ok(raw);
                }
                // 回退：尝试解析为数字
                display
                    .parse()
                    .map_err(|_| format!("'{}' is not a valid enum value", display))
            }
        }
    }

    // ==================== 辅助方法 ====================

    /// 检查是否为枚举类型
    pub fn is_enum(&self) -> bool {
        matches!(self, Self::CompuVtab { .. })
    }

    /// 获取枚举选项列表（用于下拉菜单）
    pub fn enum_options(&self) -> Option<&[CompuVtabEntry]> {
        match self {
            Self::CompuVtab { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// 获取枚举选项数量
    pub fn enum_count(&self) -> usize {
        match self {
            Self::CompuVtab { entries, .. } => entries.len(),
            _ => 0,
        }
    }
}

// 辅助函数：解析字节为整数
fn parse_int_bytes(bytes: &[u8]) -> i64 {
    match bytes.len() {
        1 => bytes[0] as i64,
        2 => i16::from_le_bytes([bytes[0], bytes[1]]) as i64,
        4 => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64,
        8 => i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        _ => 0,
    }
}

// 辅助函数：解析字节为浮点数
fn parse_float_bytes(bytes: &[u8]) -> f64 {
    match bytes.len() {
        4 => {
            let val = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            val as f64
        }
        8 => f64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        _ => 0.0,
    }
}

/// COMPU_METHOD 工厂 - 从 A2L 解析结果创建
pub struct CompuMethodFactory;

impl CompuMethodFactory {
    /// 从 A2L COMPU_METHOD 和 COMPU_VTAB 创建转换方法
    pub fn from_a2l(
        compu_method: &a2lfile::CompuMethod,
        vtabs: &HashMap<String, &a2lfile::CompuVtab>,
    ) -> CompuMethod {
        match compu_method.conversion_type {
            a2lfile::ConversionType::Identical => CompuMethod::Identical,
            a2lfile::ConversionType::Linear => {
                if let Some(ref coeffs) = compu_method.coeffs_linear {
                    CompuMethod::linear(coeffs.a, coeffs.b)
                } else {
                    CompuMethod::Identical
                }
            }
            a2lfile::ConversionType::TabVerb => {
                // 查找 COMPU_VTAB 引用
                if let Some(ref ref_name) = compu_method.compu_tab_ref {
                    if let Some(vtab) = vtabs.get(ref_name) {
                        let entries: Vec<(i64, String)> = vtab
                            .value_pairs
                            .iter()
                            .map(|vp| (vp.in_val as i64, vp.out_val.clone()))
                            .collect();
                        return CompuMethod::compu_vtab(entries);
                    }
                }
                CompuMethod::Identical
            }
            _ => CompuMethod::Identical,
        }
    }
}
```

### 步骤 3: 重构 a2l 模块 (a2l/mod.rs)

```rust
//! A2L 解析模块
//!
//! 提供 A2L 文件解析和 COMPU_METHOD 转换功能

pub mod compute_method;
pub mod reader;
pub mod types;

// 统一导出常用类型
pub use compute_method::{CompuMethod, CompuVtabEntry, CompuMethodFactory};
pub use reader::{A2lReader, A2lMeasurement, A2lCharacteristic};
pub use types::{A2lDataType, AxisInfo, CharacteristicType, StorageOrder};
```

### 步骤 4: 修改 lib.rs 模块声明

```rust
// 修改前
pub mod a2l_reader;

// 修改后
pub mod a2l;  // 使用文件夹模块
```

### 步骤 5: 修改 calibration.rs 适配新接口

```rust
// 修改前
use crate::a2l_reader::{A2lCharacteristic, A2lDataType, A2lReader};
use crate::daq::CompuMethod;

// 修改后
use crate::a2l::{A2lCharacteristic, A2lDataType, A2lReader, CompuMethod};

// CalibrationParam 保持不变，使用新的 CompuMethod
impl CalibrationParam {
    /// 获取显示值（使用新的 to_display 接口）
    pub fn format_value(&self, raw: i64) -> String {
        self.compu_method.to_display(raw)
    }

    /// 解析输入值为原始值
    pub fn parse_value(&self, display: &str) -> Result<i64, String> {
        self.compu_method.to_raw(display)
    }

    /// 是否为枚举类型
    pub fn is_enum(&self) -> bool {
        self.compu_method.is_enum()
    }

    /// 获取枚举选项
    pub fn enum_options(&self) -> Option<&[crate::a2l::CompuVtabEntry]> {
        self.compu_method.enum_options()
    }
}
```

### 步骤 6: 修改 daq.rs 适配新接口

```rust
// 修改前：daq.rs 中定义 CompuMethod
use crate::a2l_reader::{A2lDataType, A2lMeasurement, A2lReader};

#[derive(Debug, Clone)]
pub enum CompuMethod { ... }

// 修改后：使用 a2l::compute_method
use crate::a2l::{A2lDataType, A2lMeasurement, A2lReader, CompuMethod};

// DaqMeasurement 中的转换方法更新
impl DaqMeasurement {
    /// 转换原始字节为物理值
    pub fn convert_bytes(&self, bytes: &[u8]) -> f64 {
        let is_float = matches!(self.data_type, A2lDataType::F32 | A2lDataType::F64);
        self.compu_method.bytes_to_display(bytes, is_float)
    }
}
```

### 步骤 7: 修改 web/routes.rs 提供枚举 API

**新增 API 端点**:

```rust
/// GET /api/cal/enum/:slave_id/:idx
/// 获取变量的枚举选项列表
async fn cal_enum_options(
    State(state): State<AppState>,
    Path((slave_id, idx)): Path<(String, usize)>,
) -> Json<serde_json::Value> {
    let state_guard = state.slave_state.read().await;

    // 查找变量
    let slave = match state_guard.get(&slave_id) {
        Some(s) => s,
        None => {
            return json!({
                "success": false,
                "error": "Device not found"
            }).into();
        }
    };

    let param = match slave.cal_manager.params().get(idx) {
        Some(p) => p,
        None => {
            return json!({
                "success": false,
                "error": "Parameter not found"
            }).into();
        }
    };

    // 返回枚举选项
    if let Some(options) = param.enum_options() {
        let options_json: Vec<serde_json::Value> = options
            .iter()
            .map(|opt| json!({
                "raw": opt.raw_value,
                "text": opt.text_value,
            }))
            .collect();

        json!({
            "success": true,
            "is_enum": true,
            "options": options_json
        }).into()
    } else {
        json!({
            "success": true,
            "is_enum": false,
            "options": []
        }).into()
    }
}

/// 修改现有的 /api/cal/value 端点
/// 返回转换后的显示值
async fn cal_value(
    State(state): State<AppState>,
    Path((slave_id, idx)): Path<(String, usize)>,
) -> Json<serde_json::Value> {
    // ... 读取原始值 ...

    let display_value = param.format_value(raw_value);

    json!({
        "success": true,
        "value": display_value,        // 转换后的显示值
        "raw_value": raw_value,        // 原始值
        "is_enum": param.is_enum(),
    }).into()
}
```

### 步骤 8: 前端界面修改 (calibration.hbs)

**关键修改**:

```javascript
// 读取变量时处理枚举显示
async function readVar(deviceId, idx) {
    const response = await fetch(`/api/cal/read/${deviceId}/${idx}`);
    const data = await response.json();

    if (data.success) {
        const valueCell = document.getElementById(`value-${key}`);
        valueCell.textContent = data.value;  // 显示转换后的值
        valueCell.dataset.isEnum = data.is_enum;
        valueCell.dataset.rawValue = data.raw_value;
    }
}

// Inline 编辑时根据类型选择输入方式
async function makeEditable(deviceId, idx, cell) {
    const isEnum = cell.dataset.isEnum === 'true';

    if (isEnum) {
        createEnumSelect(deviceId, idx, cell);
    } else {
        createEditInput(deviceId, idx, cell);
    }
}

// 创建枚举下拉菜单
async function createEnumSelect(deviceId, idx, cell) {
    const response = await fetch(`/api/cal/enum/${deviceId}/${idx}`);
    const data = await response.json();

    if (!data.success || !data.is_enum) return;

    const select = document.createElement('select');
    data.options.forEach(opt => {
        const option = document.createElement('option');
        option.value = opt.text;  // 提交枚举文字
        option.textContent = `${opt.text} (${opt.raw})`;
        if (opt.text === cell.textContent) {
            option.selected = true;
        }
        select.appendChild(option);
    });

    // ... 事件处理 ...
}
```

## 4. 文件变更清单

| 操作 | 路径 | 说明 |
|------|------|------|
| 新建文件夹 | `src/a2l/` | A2L 模块目录 |
| 移动 | `src/a2l_reader.rs` → `src/a2l/reader.rs` | 保持原有解析逻辑 |
| 新建 | `src/a2l/mod.rs` | 模块导出 |
| 新建 | `src/a2l/types.rs` | 类型定义（从 reader.rs 提取） |
| 新建 | `src/a2l/compute_method.rs` | COMPU_METHOD 核心实现 |
| 修改 | `src/lib.rs` | 更新模块声明 |
| 修改 | `src/calibration.rs` | 使用 `crate::a2l::CompuMethod` |
| 修改 | `src/daq.rs` | 使用 `crate::a2l::CompuMethod` |
| 修改 | `src/web/routes.rs` | 新增 `/api/cal/enum` 端点 |
| 修改 | `src/web/templates.rs` | 适配新的枚举数据 |
| 修改 | `src/web/templates/calibration.hbs` | 支持下拉菜单 |
| 修改 | `src/web/templates/cal_write.hbs` | 支持下拉菜单选择 |

## 5. 接口设计说明

### 5.1 为什么将 compute_method 独立成模块？

1. **单一职责**: A2L 解析和数值转换是两个不同的职责
2. **可测试性**: CompuMethod 可以独立测试，不依赖 A2L 文件
3. **可复用性**: 转换逻辑可以被 calibration 和 daq 同时使用
4. **扩展性**: 新增转换类型只需要修改 compute_method.rs

### 5.2 统一接口设计

```rust
// 统一的核心接口
pub trait ValueConverter {
    fn to_display(&self, raw: i64) -> String;
    fn to_raw(&self, display: &str) -> Result<i64, String>;
    fn bytes_to_display(&self, bytes: &[u8], is_float: bool) -> f64;
}

// 类型判断
pub trait TypeChecker {
    fn is_enum(&self) -> bool;
    fn enum_options(&self) -> Option<&[CompuVtabEntry]>;
}
```

### 5.3 数据流向

```
A2L File
    │
    ▼
┌─────────────────────┐     ┌─────────────────────┐
│  a2l::reader::      │────▶│  a2l::compute_method│
│  A2lReader::load()  │     │  CompuMethodFactory │
└─────────────────────┘     └──────────┬──────────┘
                                       │
              ┌────────────────────────┼────────────────────────┐
              │                        │                        │
              ▼                        ▼                        ▼
    ┌──────────────────┐    ┌──────────────────┐    ┌──────────────────┐
    │ calibration::    │    │ daq::            │    │ web::routes::    │
    │ CalibrationParam │    │ DaqMeasurement   │    │ cal_value()      │
    │ .format_value()  │    │ .convert_bytes() │    │ /api/cal/enum    │
    └──────────────────┘    └──────────────────┘    └──────────────────┘
```

## 6. 实施顺序建议

1. **Phase 1**: 创建 `a2l/` 文件夹结构，迁移 `a2l_reader.rs` → `a2l/reader.rs`
2. **Phase 2**: 实现 `a2l/compute_method.rs` 核心转换逻辑
3. **Phase 3**: 修改 `calibration.rs` 和 `daq.rs` 适配新接口
4. **Phase 4**: 新增 Web API 端点 `/api/cal/enum`
5. **Phase 5**: 修改前端模板支持下拉菜单
6. **Phase 6**: 集成测试和优化

## 7. 向后兼容性

- 无 COMPU_METHOD 的变量使用 `Identical`（原样显示）
- API 响应增加 `is_enum` 字段，前端可根据此字段判断显示方式
- 旧代码可以继续使用 `parse_value` 方法（行为不变）
