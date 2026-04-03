//! COMPU_METHOD Conversion Module
//!
//! Provides bidirectional conversion between raw ECU values and display values.
//! Supports conversion types: IDENTICAL, LINEAR, RATIONAL, TAB_VERB, and COMPU_VTAB.

use std::collections::HashMap;

/// COMPU_METHOD conversion method
#[derive(Debug, Clone)]
pub enum CompuMethod {
    /// No conversion: display = raw
    Identical,
    /// Linear conversion: display = factor * raw + offset
    Linear { factor: f64, offset: f64 },
    /// Rational conversion: display = (a * raw + b) / (c * raw + d)
    Rational { a: f64, b: f64, c: f64, d: f64 },
    /// Value table with interpolation (continuous values)
    TabVerb { pairs: Vec<(f64, f64)> },
    /// Enumeration table (discrete values) - NEW
    CompuVtab {
        entries: Vec<CompuVtabEntry>,
        raw_to_text: HashMap<i64, String>,
        text_to_raw: HashMap<String, i64>,
    },
}

/// Enumeration table entry
#[derive(Debug, Clone)]
pub struct CompuVtabEntry {
    pub raw_value: i64,
    pub text_value: String,
}

impl CompuMethod {
    /// Create an IDENTICAL conversion method
    pub fn identical() -> Self {
        Self::Identical
    }

    /// Create a LINEAR conversion method
    pub fn linear(factor: f64, offset: f64) -> Self {
        Self::Linear { factor, offset }
    }

    /// Create a RATIONAL conversion method
    pub fn rational(a: f64, b: f64, c: f64, d: f64) -> Self {
        Self::Rational { a, b, c, d }
    }

    /// Create a TAB_VERB conversion method
    pub fn tab_verb(pairs: Vec<(f64, f64)>) -> Self {
        Self::TabVerb { pairs }
    }

    /// Create a COMPU_VTAB enumeration conversion method
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

    // ==================== Core Conversion Interface ====================

    /// Convert raw value to display string (for integer values)
    ///
    /// # Arguments
    /// - `raw`: Raw integer value read from ECU
    ///
    /// # Returns
    /// Display string (enum text or formatted number)
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

    /// Convert raw bytes to display value (for DAQ real-time data)
    ///
    /// # Arguments
    /// - `bytes`: Raw byte data
    /// - `is_float`: Whether the data type is float (float types skip conversion)
    ///
    /// # Returns
    /// Physical value as f64
    pub fn bytes_to_display(&self, bytes: &[u8], is_float: bool) -> f64 {
        if is_float {
            // Float types are parsed directly, no COMPU_METHOD applied
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
            Self::CompuVtab { .. } => raw as f64, // Return raw value for trend charts
        }
    }

    /// Convert display value or enum text back to raw value
    ///
    /// # Arguments
    /// - `display`: User input display value or enum text
    ///
    /// # Returns
    /// Raw integer value
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
                Err("Rational conversion reverse not supported".to_string())
            }
            Self::TabVerb { .. } => {
                Err("TabVerb conversion reverse not supported".to_string())
            }
            Self::CompuVtab { text_to_raw, .. } => {
                // First try to match enum text
                if let Some(&raw) = text_to_raw.get(display) {
                    return Ok(raw);
                }
                // Fallback: try to parse as number
                display
                    .parse()
                    .map_err(|_| format!("'{}' is not a valid enum value", display))
            }
        }
    }

    // ==================== Helper Methods ====================

    /// Check if this is an enum type
    pub fn is_enum(&self) -> bool {
        matches!(self, Self::CompuVtab { .. })
    }

    /// Get enum options list (for dropdown menu)
    pub fn enum_options(&self) -> Option<&[CompuVtabEntry]> {
        match self {
            Self::CompuVtab { entries, .. } => Some(entries),
            _ => None,
        }
    }

    /// Get number of enum options
    pub fn enum_count(&self) -> usize {
        match self {
            Self::CompuVtab { entries, .. } => entries.len(),
            _ => 0,
        }
    }

    /// Get a specific enum entry by raw value
    pub fn get_enum_text(&self, raw: i64) -> Option<&str> {
        match self {
            Self::CompuVtab { raw_to_text, .. } => {
                raw_to_text.get(&raw).map(|s| s.as_str())
            }
            _ => None,
        }
    }

    /// Get raw value for an enum text
    pub fn get_enum_raw(&self, text: &str) -> Option<i64> {
        match self {
            Self::CompuVtab { text_to_raw, .. } => {
                text_to_raw.get(text).copied()
            }
            _ => None,
        }
    }
}

impl Default for CompuMethod {
    fn default() -> Self {
        Self::Identical
    }
}

// ==================== Helper Functions ====================

/// Parse bytes as signed integer
fn parse_int_bytes(bytes: &[u8]) -> i64 {
    match bytes.len() {
        1 => bytes[0] as i8 as i64,
        2 => i16::from_le_bytes([bytes[0], bytes[1]]) as i64,
        4 => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64,
        8 => i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        _ => 0,
    }
}

/// Parse bytes as float
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

/// COMPU_METHOD Factory - create from A2L parse results
pub struct CompuMethodFactory;

impl CompuMethodFactory {
    /// Create CompuMethod from A2L COMPU_METHOD and available COMPU_VTABs
    ///
    /// TODO: Implement full COMPU_VTAB lookup when a2lfile crate types are known
    pub fn from_a2l(
        compu_method: &a2lfile::CompuMethod,
        _vtabs: &HashMap<String, &a2lfile::CompuVtab>,
    ) -> CompuMethod {
        use a2lfile::ConversionType;

        match compu_method.conversion_type {
            ConversionType::Identical => CompuMethod::Identical,
            ConversionType::Linear => {
                if let Some(ref coeffs) = compu_method.coeffs_linear {
                    CompuMethod::linear(coeffs.a, coeffs.b)
                } else {
                    CompuMethod::Identical
                }
            }
            ConversionType::TabVerb => {
                // TODO: Implement COMPU_VTAB lookup
                // This requires knowing the exact type of compu_tab_ref
                CompuMethod::Identical
            }
            _ => CompuMethod::Identical,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_conversion() {
        let method = CompuMethod::identical();
        assert_eq!(method.to_display(42), "42");
        assert_eq!(method.to_raw("42").unwrap(), 42);
        assert!(!method.is_enum());
    }

    #[test]
    fn test_linear_conversion() {
        // phys = 0.5 * raw + 10
        let method = CompuMethod::linear(0.5, 10.0);
        assert_eq!(method.to_display(0), "10.000000");
        assert_eq!(method.to_display(100), "60.000000");
        assert_eq!(method.to_raw("60").unwrap(), 100);
    }

    #[test]
    fn test_compu_vtab_conversion() {
        let entries = vec![
            (0, "OFF".to_string()),
            (1, "ON".to_string()),
            (2, "AUTO".to_string()),
        ];
        let method = CompuMethod::compu_vtab(entries);

        assert!(method.is_enum());
        assert_eq!(method.enum_count(), 3);

        // Forward conversion
        assert_eq!(method.to_display(0), "OFF");
        assert_eq!(method.to_display(1), "ON");
        assert_eq!(method.to_display(2), "AUTO");
        assert_eq!(method.to_display(99), "99"); // Unknown raw value

        // Reverse conversion
        assert_eq!(method.to_raw("OFF").unwrap(), 0);
        assert_eq!(method.to_raw("ON").unwrap(), 1);
        assert_eq!(method.to_raw("AUTO").unwrap(), 2);
        assert_eq!(method.to_raw("1").unwrap(), 1); // Numeric fallback

        // Enum options
        let options = method.enum_options().unwrap();
        assert_eq!(options[0].raw_value, 0);
        assert_eq!(options[0].text_value, "OFF");
    }

    #[test]
    fn test_bytes_to_display_integer() {
        let method = CompuMethod::linear(2.0, 0.0); // phys = 2 * raw
        let bytes = [10u8, 0]; // raw = 10
        assert_eq!(method.bytes_to_display(&bytes, false), 20.0);
    }

    #[test]
    fn test_bytes_to_display_float() {
        let method = CompuMethod::linear(2.0, 0.0);
        let bytes = 3.14f32.to_le_bytes();
        // Float types skip conversion
        assert!((method.bytes_to_display(&bytes, true) - 3.14).abs() < 0.001);
    }

    #[test]
    fn test_default_is_identical() {
        let method: CompuMethod = Default::default();
        assert!(matches!(method, CompuMethod::Identical));
    }
}
