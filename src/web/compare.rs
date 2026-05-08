use crate::calibration::{CalibrationManager, CalibrationParam, CalibrationValue};
use crate::hex::HexImage;
use crate::web::calibration_data::{format_raw_hex, read_axis_values_from_hex, read_calibration_from_hex};
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use dcm_utils::block::Block as DcmBlock;
use dcm_utils::value::Value as DcmValue;
use dcm_utils::DcmData;

const COMPARE_EPSILON: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareSourceKind {
    A2lHex,
    Dcm,
}

impl CompareSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A2lHex => "a2l_hex",
            Self::Dcm => "dcm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::A2lHex => "A2L + HEX",
            Self::Dcm => "DCM",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparePresenceFilter {
    All,
    LeftOnly,
    RightOnly,
    Common,
}

impl ComparePresenceFilter {
    pub fn from_str(value: &str) -> Self {
        match value {
            "left_only" => Self::LeftOnly,
            "right_only" => Self::RightOnly,
            "common" => Self::Common,
            _ => Self::All,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::LeftOnly => "left_only",
            Self::RightOnly => "right_only",
            Self::Common => "common",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareValueKind {
    Scalar,
    Array,
    Matrix,
}

impl CompareValueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Array => "array",
            Self::Matrix => "matrix",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompareAtom {
    pub display: String,
    pub numeric: Option<f64>,
}

impl CompareAtom {
    fn new(display: String, numeric: Option<f64>) -> Self {
        Self { display, numeric }
    }

    fn equivalent(&self, other: &Self) -> bool {
        match (self.numeric, other.numeric) {
            (Some(left), Some(right)) => (left - right).abs() <= COMPARE_EPSILON,
            _ => self.display == other.display,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompareValueGrid {
    pub kind: CompareValueKind,
    pub cells: Vec<Vec<CompareAtom>>,
}

impl CompareValueGrid {
    fn new(kind: CompareValueKind, cells: Vec<Vec<CompareAtom>>) -> Self {
        Self { kind, cells }
    }

    pub fn preview(&self) -> String {
        match self.kind {
            CompareValueKind::Scalar => self
                .cells
                .first()
                .and_then(|row| row.first())
                .map(|cell| cell.display.clone())
                .unwrap_or_default(),
            CompareValueKind::Array => {
                let row = self.cells.first().cloned().unwrap_or_default();
                let items = row
                    .into_iter()
                    .take(4)
                    .map(|cell| cell.display)
                    .collect::<Vec<_>>();
                let suffix = if self.cols() > 4 { ", ..." } else { "" };
                format!("[{}{}]", items.join(", "), suffix)
            }
            CompareValueKind::Matrix => format!("{} x {}", self.rows(), self.cols()),
        }
    }

    pub fn rows(&self) -> usize {
        self.cells.len()
    }

    pub fn cols(&self) -> usize {
        self.cells.first().map_or(0, Vec::len)
    }

    pub fn diff_positions(&self, other: &Self) -> Vec<(usize, usize)> {
        let max_rows = self.rows().max(other.rows());
        let max_cols = self.cols().max(other.cols());
        let mut diff = Vec::new();
        for row in 0..max_rows {
            for col in 0..max_cols {
                let left = self.cells.get(row).and_then(|r| r.get(col));
                let right = other.cells.get(row).and_then(|r| r.get(col));
                let matches = match (left, right) {
                    (Some(left), Some(right)) => left.equivalent(right),
                    (None, None) => true,
                    _ => false,
                };
                if !matches {
                    diff.push((row, col));
                }
            }
        }
        diff
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "kind": self.kind.as_str(),
            "rows": self.rows(),
            "cols": self.cols(),
            "cells": self.cells.iter().map(|row| {
                row.iter().map(|cell| json!({
                    "display": cell.display,
                    "numeric": cell.numeric,
                })).collect::<Vec<_>>()
            }).collect::<Vec<_>>()
        })
    }
}

/// Lightweight metadata for a calibration variable (no value data loaded).
/// Used for the variable picker UI on A2L+HEX sources where values are loaded lazily.
#[derive(Debug, Clone)]
pub struct CompareVarMeta {
    pub type_name: String,
    pub unit: Option<String>,
}

/// A2L+HEX lazy source: stores parsed params and the HEX image but defers
/// reading calibration values until the variable is actually selected for comparison.
#[derive(Debug)]
pub struct HexLazySource {
    /// All calibration parameters from the A2L file, keyed by variable name.
    pub params: HashMap<String, CalibrationParam>,
    /// Parsed Intel HEX memory image.
    pub hex: HexImage,
}

impl Clone for HexLazySource {
    fn clone(&self) -> Self {
        Self {
            params: self.params.clone(),
            hex: self.hex.clone(),
        }
    }
}

impl Clone for CompareSource {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            label: self.label.clone(),
            detail: self.detail.clone(),
            variables: self.variables.clone(),
            hex_lazy: self.hex_lazy.as_ref().map(|l| l.clone()),
        }
    }
}



#[derive(Debug, Clone)]
pub struct CompareVariable {
    pub name: String,
    pub type_name: String,
    pub unit: Option<String>,
    pub description: String,
    pub raw_hex: Option<String>,
    pub axis_values: Vec<Vec<f64>>,
    pub value: CompareValueGrid,
}

impl CompareVariable {
    pub fn to_json(&self, include_raw: bool) -> serde_json::Value {
        json!({
            "name": self.name,
            "type_name": self.type_name,
            "unit": self.unit,
            "description": self.description,
            "value": self.value.to_json(),
            "preview": self.value.preview(),
            "raw_hex": if include_raw { self.raw_hex.clone() } else { None::<String> },
            "axis_values": self.axis_values,
        })
    }
}

#[derive(Debug)]
pub struct CompareSource {
    pub kind: CompareSourceKind,
    pub label: String,
    pub detail: String,
    /// Eagerly-loaded variables (always populated for DCM; empty for A2L+HEX).
    pub variables: HashMap<String, CompareVariable>,
    /// A2L+HEX lazy source: params + hex image. None for DCM sources.
    pub hex_lazy: Option<HexLazySource>,
}

impl CompareSource {
    /// Total number of variables (across both eager and lazy stores).
    pub fn variable_count(&self) -> usize {
        if let Some(lazy) = &self.hex_lazy {
            lazy.params.len()
        } else {
            self.variables.len()
        }
    }

    /// Returns whether this source contains a variable with the given name.
    pub fn has_variable(&self, name: &str) -> bool {
        if let Some(lazy) = &self.hex_lazy {
            lazy.params.contains_key(name)
        } else {
            self.variables.contains_key(name)
        }
    }

    /// Lightweight metadata for a variable (type name + unit) without reading its value.
    pub fn var_meta(&self, name: &str) -> Option<CompareVarMeta> {
        if let Some(lazy) = &self.hex_lazy {
            lazy.params.get(name).map(|p| CompareVarMeta {
                type_name: p.characteristic_type.as_str().to_string(),
                unit: p.unit.clone(),
            })
        } else {
            self.variables.get(name).map(|v| CompareVarMeta {
                type_name: v.type_name.clone(),
                unit: v.unit.clone(),
            })
        }
    }

    /// Iterator over all variable names in this source.
    pub fn all_names(&self) -> Vec<&String> {
        if let Some(lazy) = &self.hex_lazy {
            lazy.params.keys().collect()
        } else {
            self.variables.keys().collect()
        }
    }

    /// Resolve a variable to a `CompareVariable` for comparison.
    /// For DCM sources this is a simple map lookup.
    /// For A2L+HEX sources, this reads the calibration value from the HEX image on demand.
    pub fn resolve_variable(&self, name: &str) -> Option<CompareVariable> {
        // Check eagerly-loaded variables first (always the case for DCM).
        if let Some(var) = self.variables.get(name) {
            return Some(var.clone());
        }
        // Lazy HEX read for A2L+HEX sources.
        if let Some(lazy) = &self.hex_lazy {
            if let Some(param) = lazy.params.get(name) {
                let value = read_calibration_from_hex(&lazy.hex, param).ok()?;
                let raw_bytes = lazy.hex.extract_range(param.address, param.data_type.size() as u32, 0xFF);
                let axis_values = read_axis_values_from_hex(&lazy.hex, param);
                return Some(make_compare_variable(param, value, format_raw_hex(&raw_bytes), axis_values));
            }
        }
        None
    }

    pub fn summary_json(&self) -> serde_json::Value {
        json!({
            "kind": self.kind.as_str(),
            "kind_label": self.kind.label(),
            "label": self.label,
            "detail": self.detail,
            "variable_count": self.variable_count(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CompareSession {
    pub id: String,
    pub name: String,
    pub left_source: Option<CompareSource>,
    pub right_source: Option<CompareSource>,
    pub selected_variables: Vec<String>,
}

impl CompareSession {
    pub fn new(index: usize) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Session {}", index),
            left_source: None,
            right_source: None,
            selected_variables: Vec::new(),
        }
    }

    pub fn source_mut(&mut self, side: CompareSide) -> &mut Option<CompareSource> {
        match side {
            CompareSide::Left => &mut self.left_source,
            CompareSide::Right => &mut self.right_source,
        }
    }

    pub fn source(&self, side: CompareSide) -> &Option<CompareSource> {
        match side {
            CompareSide::Left => &self.left_source,
            CompareSide::Right => &self.right_source,
        }
    }

    pub fn sources_loaded(&self) -> bool {
        self.left_source.is_some() || self.right_source.is_some()
    }

    pub fn page_variables(
        &self,
        search: &str,
        presence: ComparePresenceFilter,
        offset: usize,
        limit: usize,
    ) -> serde_json::Value {
        let left = self.left_source.as_ref();
        let right = self.right_source.as_ref();
        let mut names = BTreeSet::new();
        if let Some(source) = left {
            names.extend(source.all_names().into_iter().cloned());
        }
        if let Some(source) = right {
            names.extend(source.all_names().into_iter().cloned());
        }

        let search = search.trim().to_ascii_lowercase();
        let selected = self.selected_variables.iter().cloned().collect::<BTreeSet<_>>();
        let items = names
            .into_iter()
            .filter_map(|name| {
                let left_meta = left.and_then(|s| s.var_meta(&name));
                let right_meta = right.and_then(|s| s.var_meta(&name));
                let presence_value = match (left_meta.is_some(), right_meta.is_some()) {
                    (true, true) => ComparePresenceFilter::Common,
                    (true, false) => ComparePresenceFilter::LeftOnly,
                    (false, true) => ComparePresenceFilter::RightOnly,
                    (false, false) => return None,
                };
                if presence != ComparePresenceFilter::All && presence_value != presence {
                    return None;
                }
                if !search.is_empty() && !name.to_ascii_lowercase().contains(&search) {
                    return None;
                }
                Some(json!({
                    "name": name,
                    "presence": presence_value.as_str(),
                    "selected": selected.contains(name.as_str()),
                    "left_type": left_meta.as_ref().map(|m| m.type_name.clone()),
                    "right_type": right_meta.as_ref().map(|m| m.type_name.clone()),
                    "left_unit": left_meta.and_then(|m| m.unit),
                    "right_unit": right_meta.and_then(|m| m.unit),
                }))
            })
            .collect::<Vec<_>>();

        let total = items.len();
        let page = items.into_iter().skip(offset).take(limit).collect::<Vec<_>>();
        json!({
            "total": total,
            "offset": offset,
            "limit": limit,
            "items": page,
        })
    }

    pub fn comparison_rows(&self) -> Vec<serde_json::Value> {
        self.selected_variables
            .iter()
            .map(|name| {
                let left = self.left_source.as_ref().and_then(|s| s.resolve_variable(name));
                let right = self.right_source.as_ref().and_then(|s| s.resolve_variable(name));
                let left_ref = left.as_ref();
                let right_ref = right.as_ref();
                let (presence, matches, diff_positions) = match (left_ref, right_ref) {
                    (Some(lv), Some(rv)) => {
                        let diff = lv.value.diff_positions(&rv.value);
                        ("common", diff.is_empty(), diff)
                    }
                    (Some(_), None) => ("left_only", false, Vec::new()),
                    (None, Some(_)) => ("right_only", false, Vec::new()),
                    (None, None) => ("missing", false, Vec::new()),
                };
                json!({
                    "name": name,
                    "presence": presence,
                    "matches": matches,
                    "diff_positions": diff_positions.iter().map(|(row, col)| json!({"row": row, "col": col})).collect::<Vec<_>>(),
                    "left": left_ref.map(|var| var.to_json(!matches)),
                    "right": right_ref.map(|var| var.to_json(!matches)),
                })
            })
            .collect()
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "name": self.name,
            "selected_variables": self.selected_variables,
            "left_source": self.left_source.as_ref().map(CompareSource::summary_json),
            "right_source": self.right_source.as_ref().map(CompareSource::summary_json),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareSide {
    Left,
    Right,
}

impl CompareSide {
    pub fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(format!("Invalid compare side '{}'", value)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

fn format_number(value: f64) -> String {
    if (value.fract()).abs() <= COMPARE_EPSILON {
        format!("{:.0}", value)
    } else {
        format!("{:.6}", value)
    }
}

fn atom_from_i64(raw: i64, param: &CalibrationParam) -> CompareAtom {
    let display = param.compu_method.to_display(raw);
    let numeric = if param.is_enum() {
        None
    } else {
        display.parse::<f64>().ok()
    };
    CompareAtom::new(display, numeric)
}

fn atom_from_f64(value: f64) -> CompareAtom {
    CompareAtom::new(format_number(value), Some(value))
}

fn atom_from_bool(value: bool) -> CompareAtom {
    CompareAtom::new(value.to_string(), None)
}

fn grid_from_calibration_value(param: &CalibrationParam, value: &CalibrationValue) -> CompareValueGrid {
    match value {
        CalibrationValue::U8(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::U16(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::U32(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::U64(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::I8(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::I16(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::I32(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v as i64, param)]]),
        CalibrationValue::I64(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_i64(*v, param)]]),
        CalibrationValue::F32(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_f64(*v as f64)]]),
        CalibrationValue::F64(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_f64(*v)]]),
        CalibrationValue::Bool(v) => CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_bool(*v)]]),
        CalibrationValue::U8Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::U16Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::U32Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::U64Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::I8Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::I16Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::I32Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v as i64, param)).collect()]),
        CalibrationValue::I64Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_i64(*v, param)).collect()]),
        CalibrationValue::F32Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_f64(*v as f64)).collect()]),
        CalibrationValue::F64Array(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_f64(*v)).collect()]),
        CalibrationValue::BoolArray(values) => CompareValueGrid::new(CompareValueKind::Array, vec![values.iter().map(|v| atom_from_bool(*v)).collect()]),
        // CalibrationValue::*Matrix stores values[x][y] (outer=X axis, inner=Y axis).
        // The display convention is cells[y][x] (outer=Y rows, inner=X columns), so we transpose.
        CalibrationValue::U8Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::U16Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::U32Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::U64Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::I8Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::I16Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::I32Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y] as i64, param)).collect()).collect())
        }
        CalibrationValue::I64Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_i64(values[x][y], param)).collect()).collect())
        }
        CalibrationValue::F32Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_f64(values[x][y] as f64)).collect()).collect())
        }
        CalibrationValue::F64Matrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_f64(values[x][y])).collect()).collect())
        }
        CalibrationValue::BoolMatrix(values) => {
            let (xn, yn) = (values.len(), values.first().map(|r| r.len()).unwrap_or(0));
            CompareValueGrid::new(CompareValueKind::Matrix, (0..yn).map(|y| (0..xn).map(|x| atom_from_bool(values[x][y])).collect()).collect())
        }
        CalibrationValue::Ascii(s) => CompareValueGrid::new(
            CompareValueKind::Scalar,
            vec![vec![CompareAtom::new(s.clone(), None)]],
        ),
    }
}

fn grid_from_dcm_scalar(value: &DcmValue) -> Result<CompareValueGrid, String> {
    match value {
        DcmValue::WERT(values) if values.len() == 1 => {
            Ok(CompareValueGrid::new(CompareValueKind::Scalar, vec![vec![atom_from_f64(values[0])]]))
        }
        DcmValue::TEXT(values) if values.len() == 1 => Ok(CompareValueGrid::new(
            CompareValueKind::Scalar,
            vec![vec![CompareAtom::new(values[0].clone(), None)]],
        )),
        DcmValue::WERT(values) => Err(format!("Expected scalar DCM value, got {}", values.len())),
        DcmValue::TEXT(values) => Err(format!("Expected scalar DCM text, got {}", values.len())),
    }
}

fn grid_from_dcm_array(value: &DcmValue) -> CompareValueGrid {
    match value {
        DcmValue::WERT(values) => CompareValueGrid::new(
            CompareValueKind::Array,
            vec![values.iter().map(|value| atom_from_f64(*value)).collect()],
        ),
        DcmValue::TEXT(values) => CompareValueGrid::new(
            CompareValueKind::Array,
            vec![values
                .iter()
                .map(|value| CompareAtom::new(value.clone(), None))
                .collect()],
        ),
    }
}

fn grid_from_dcm_map(map: &dcm_utils::blocks::GRUPPENKENNFELD) -> Result<CompareValueGrid, String> {
    let (x_count, y_count) = map.dim;
    if map.value.is_empty() {
        return Ok(CompareValueGrid::new(CompareValueKind::Matrix, Vec::new()));
    }

    let has_numeric = map.value.iter().any(|row| matches!(row, DcmValue::WERT(_)));
    if has_numeric {
        // dcm_utils may stop reading early on unrecognised lines, so actual data can be
        // smaller than the declared dim. Filter to only numeric rows.
        let dcm_rows: Vec<Vec<f64>> = map
            .value
            .iter()
            .filter_map(|row| match row {
                DcmValue::WERT(values) => Some(values.clone()),
                DcmValue::TEXT(_) => None,
            })
            .collect();
        let used_nrow = if dcm_rows.is_empty() { y_count } else { dcm_rows.len() };
        let used_ncol = dcm_rows.first().map(|r| r.len()).filter(|&n| n > 0).unwrap_or(x_count);
        if dcm_rows.len() != y_count || used_ncol != x_count {
            tracing::warn!(
                "DCM MAP '{}' declared {}×{} but only {}×{} values parsed; padding missing cells with 0",
                map.name, x_count, y_count, dcm_rows.len(), used_ncol,
            );
        }
        // DCM stores dcm_rows[y][x]: nrow WERT rows of ncol values each.
        // Display convention is cells[y][x], so iterate y outer, x inner.
        let mut cells = Vec::with_capacity(used_nrow);
        for y in 0..used_nrow {
            let mut row = Vec::with_capacity(used_ncol);
            for x in 0..used_ncol {
                let value = dcm_rows
                    .get(y)
                    .and_then(|source_row| source_row.get(x))
                    .copied()
                    .unwrap_or(0.0);
                row.push(atom_from_f64(value));
            }
            cells.push(row);
        }
        return Ok(CompareValueGrid::new(CompareValueKind::Matrix, cells));
    }

    let text_rows: Vec<Vec<String>> = map
        .value
        .iter()
        .filter_map(|row| match row {
            DcmValue::TEXT(values) => Some(values.clone()),
            DcmValue::WERT(_) => None,
        })
        .collect();
    let used_nrow = if text_rows.is_empty() { y_count } else { text_rows.len() };
    let used_ncol = text_rows.first().map(|r| r.len()).filter(|&n| n > 0).unwrap_or(x_count);
    if text_rows.len() != y_count || used_ncol != x_count {
        tracing::warn!(
            "DCM MAP '{}' text declared {}×{} but only {}×{} values parsed; padding with '?'",
            map.name, x_count, y_count, text_rows.len(), used_ncol,
        );
    }
    let mut cells = Vec::with_capacity(used_nrow);
    for y in 0..used_nrow {
        let mut row = Vec::with_capacity(used_ncol);
        for x in 0..used_ncol {
            let value = text_rows
                .get(y)
                .and_then(|source_row| source_row.get(x))
                .cloned()
                .unwrap_or_else(|| String::from("?"));
            row.push(CompareAtom::new(value, None));
        }
        cells.push(row);
    }
    Ok(CompareValueGrid::new(CompareValueKind::Matrix, cells))
}

fn axis_values_from_dcm_block(block: &DcmBlock) -> Vec<Vec<f64>> {
    match block {
        DcmBlock::Table(table) => vec![table.axis.clone()],
        DcmBlock::Map(map) => vec![map.x_axis.clone(), map.y_axis.clone()],
        _ => Vec::new(),
    }
}

fn make_compare_variable(
    param: &CalibrationParam,
    value: CalibrationValue,
    raw_hex: String,
    axis_values: Vec<Vec<f64>>,
) -> CompareVariable {
    CompareVariable {
        name: param.name.clone(),
        type_name: param.data_type.type_name().to_string(),
        unit: param.unit.clone(),
        description: param.description.clone(),
        raw_hex: Some(raw_hex),
        axis_values,
        value: grid_from_calibration_value(param, &value),
    }
}

fn write_temp_file(prefix: &str, filename: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir().join("xcp_client_compare");
    std::fs::create_dir_all(&temp_dir).map_err(|error| format!("Failed to create temp dir: {}", error))?;
    let safe_name = filename.replace(['/', '\\', ':'], "_");
    let path = temp_dir.join(format!("{}_{}_{}", prefix, uuid::Uuid::new_v4(), safe_name));
    std::fs::write(&path, bytes).map_err(|error| format!("Failed to write temp file: {}", error))?;
    Ok(path)
}

fn remove_temp_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn load_a2l_hex_source(
    a2l_filename: &str,
    a2l_bytes: &[u8],
    hex_filename: &str,
    hex_bytes: &[u8],
) -> Result<CompareSource, String> {
    let a2l_path = write_temp_file("compare_a2l", a2l_filename, a2l_bytes)?;
    let result = (|| {
        let cal_manager = CalibrationManager::from_a2l(&a2l_path)?;
        let hex_content = std::str::from_utf8(hex_bytes)
            .map_err(|error| format!("Invalid UTF-8 in HEX file: {}", error))?;
        let hex = HexImage::from_str(hex_content).map_err(|error| format!("HEX parse error: {}", error))?;

        // Build a name → param map for lazy value reading.
        let params: HashMap<String, CalibrationParam> = cal_manager
            .params()
            .into_iter()
            .map(|p| (p.name.clone(), p.clone()))
            .collect();

        Ok(CompareSource {
            kind: CompareSourceKind::A2lHex,
            label: format!("{} + {}", a2l_filename, hex_filename),
            detail: format!("A2L: {} | HEX: {}", a2l_filename, hex_filename),
            variables: HashMap::new(), // populated lazily via resolve_variable()
            hex_lazy: Some(HexLazySource { params, hex }),
        })
    })();
    remove_temp_file(&a2l_path);
    result
}

pub fn load_dcm_source(filename: &str, bytes: &[u8]) -> Result<CompareSource, String> {
    let dcm_path = write_temp_file("compare_dcm", filename, bytes)?;
    let result = (|| {
        let dcm = DcmData::new(Path::new(&dcm_path));
        let mut variables = HashMap::new();
        for (name, block) in &dcm.blocks {
            let (type_name, value) = match block {
                DcmBlock::Constant(value) => (
                    "FESTWERT".to_string(),
                    grid_from_dcm_scalar(&value.value)?,
                ),
                DcmBlock::ConstantBlock(value) => (
                    "FESTWERTEBLOCK".to_string(),
                    grid_from_dcm_array(&value.value),
                ),
                DcmBlock::Table(value) => (
                    "GRUPPENKENNLINIE".to_string(),
                    grid_from_dcm_array(&value.value),
                ),
                DcmBlock::Distribution(value) => (
                    "STUETZSTELLENVERTEILUNG".to_string(),
                    grid_from_dcm_array(&value.value),
                ),
                DcmBlock::Map(value) => (
                    "GRUPPENKENNFELD".to_string(),
                    grid_from_dcm_map(value)?,
                ),
            };

            variables.insert(
                name.clone(),
                CompareVariable {
                    name: name.clone(),
                    type_name,
                    unit: None,
                    description: String::new(),
                    raw_hex: None,
                    axis_values: axis_values_from_dcm_block(block),
                    value,
                },
            );
        }

        Ok(CompareSource {
            kind: CompareSourceKind::Dcm,
            label: filename.to_string(),
            detail: format!("DCM: {}", filename),
            variables,
            hex_lazy: None,
        })
    })();
    remove_temp_file(&dcm_path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2l::{CompuMethod, StorageOrder};
    use crate::calibration::{CalibrationParam, CalibrationType};

    #[test]
    fn test_compare_atom_numeric_equivalence() {
        let left = CompareAtom::new("1.000000".to_string(), Some(1.0));
        let right = CompareAtom::new("1".to_string(), Some(1.0));
        assert!(left.equivalent(&right));
    }

    #[test]
    fn test_compare_grid_diff_positions() {
        let left = CompareValueGrid::new(
            CompareValueKind::Matrix,
            vec![
                vec![CompareAtom::new("1".to_string(), Some(1.0))],
                vec![CompareAtom::new("2".to_string(), Some(2.0))],
            ],
        );
        let right = CompareValueGrid::new(
            CompareValueKind::Matrix,
            vec![
                vec![CompareAtom::new("1".to_string(), Some(1.0))],
                vec![CompareAtom::new("3".to_string(), Some(3.0))],
            ],
        );
        assert_eq!(left.diff_positions(&right), vec![(1, 0)]);
    }

    #[test]
    fn test_grid_from_calibration_value_uses_compu_method() {
        let param = CalibrationParam {
            name: "speed".to_string(),
            description: String::new(),
            address: 0,
            address_ext: 0,
            data_type: CalibrationType::U16,
            unit: Some("km/h".to_string()),
            min: None,
            max: None,
            compu_method: CompuMethod::linear(0.5, 0.0),
            characteristic_type: crate::a2l::CharacteristicType::Value,
            axis_info: Vec::new(),
            storage_order: StorageOrder::RowDir,
        };
        let grid = grid_from_calibration_value(&param, &CalibrationValue::U16(20));
        assert_eq!(grid.preview(), "10.000000");
    }

    #[test]
    fn test_session_page_variables_presence_filter() {
        let left_source = CompareSource {
            kind: CompareSourceKind::Dcm,
            label: "left".to_string(),
            detail: String::new(),
            variables: HashMap::from([(
                "A".to_string(),
                CompareVariable {
                    name: "A".to_string(),
                    type_name: "FESTWERT".to_string(),
                    unit: None,
                    description: String::new(),
                    raw_hex: None,
                    axis_values: Vec::new(),
                    value: CompareValueGrid::new(
                        CompareValueKind::Scalar,
                        vec![vec![CompareAtom::new("1".to_string(), Some(1.0))]],
                    ),
                },
            )]),
            hex_lazy: None,
        };
        let right_source = CompareSource {
            kind: CompareSourceKind::Dcm,
            label: "right".to_string(),
            detail: String::new(),
            variables: HashMap::from([
                (
                    "A".to_string(),
                    CompareVariable {
                        name: "A".to_string(),
                        type_name: "FESTWERT".to_string(),
                        unit: None,
                        description: String::new(),
                        raw_hex: None,
                        axis_values: Vec::new(),
                        value: CompareValueGrid::new(
                            CompareValueKind::Scalar,
                            vec![vec![CompareAtom::new("1".to_string(), Some(1.0))]],
                        ),
                    },
                ),
                (
                    "B".to_string(),
                    CompareVariable {
                        name: "B".to_string(),
                        type_name: "FESTWERT".to_string(),
                        unit: None,
                        description: String::new(),
                        raw_hex: None,
                        axis_values: Vec::new(),
                        value: CompareValueGrid::new(
                            CompareValueKind::Scalar,
                            vec![vec![CompareAtom::new("2".to_string(), Some(2.0))]],
                        ),
                    },
                ),
            ]),
            hex_lazy: None,
        };

        let session = CompareSession {
            id: "s1".to_string(),
            name: "Session 1".to_string(),
            left_source: Some(left_source),
            right_source: Some(right_source),
            selected_variables: vec!["A".to_string()],
        };

        let page = session.page_variables("", ComparePresenceFilter::RightOnly, 0, 50);
        let items = page.get("items").and_then(|value| value.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].get("name").and_then(|value| value.as_str()), Some("B"));
    }
}
