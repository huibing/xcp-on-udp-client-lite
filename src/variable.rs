//! Variable Management with Device Association
//!
//! Provides structures and utilities for managing variables (calibration parameters
//! and DAQ measurements) that are associated with specific XCP slaves.

use crate::calibration::CalibrationParam;
use crate::daq::Measurement;
use crate::slave::SlaveId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Variable type (Calibration or Measurement)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VariableType {
    /// Calibration parameter (writable)
    Calibration,
    /// DAQ measurement (read-only)
    Measurement,
}

impl VariableType {
    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            VariableType::Calibration => "CAL",
            VariableType::Measurement => "DAQ",
        }
    }

    /// Check if calibration
    pub fn is_calibration(&self) -> bool {
        matches!(self, VariableType::Calibration)
    }

    /// Check if measurement
    pub fn is_measurement(&self) -> bool {
        matches!(self, VariableType::Measurement)
    }
}

impl std::fmt::Display for VariableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Device variable with full context
#[derive(Debug, Clone)]
pub struct DeviceVariable {
    /// Slave ID that owns this variable
    pub slave_id: SlaveId,
    /// Slave name (for display)
    pub slave_name: String,
    /// Variable name
    pub name: String,
    /// Variable type
    pub var_type: VariableType,
    /// Full address info (for reference)
    pub address: u32,
    pub address_ext: u8,
    /// Data type string representation
    pub data_type: String,
    /// Unit (if available)
    pub unit: Option<String>,
    /// Description (if available)
    pub description: String,
}

impl DeviceVariable {
    /// Create a new device variable
    pub fn new(
        slave_id: SlaveId,
        slave_name: String,
        name: String,
        var_type: VariableType,
    ) -> Self {
        Self {
            slave_id,
            slave_name,
            name,
            var_type,
            address: 0,
            address_ext: 0,
            data_type: String::new(),
            unit: None,
            description: String::new(),
        }
    }

    /// Create from calibration parameter
    pub fn from_calibration_param(
        slave_id: SlaveId,
        slave_name: &str,
        param: &CalibrationParam,
    ) -> Self {
        Self {
            slave_id,
            slave_name: slave_name.to_string(),
            name: param.name.clone(),
            var_type: VariableType::Calibration,
            address: param.address,
            address_ext: param.address_ext,
            data_type: param.data_type.type_name(),
            unit: param.unit.clone(),
            description: param.description.clone(),
        }
    }

    /// Create from measurement
    pub fn from_measurement(
        slave_id: SlaveId,
        slave_name: &str,
        measurement: &Measurement,
    ) -> Self {
        Self {
            slave_id,
            slave_name: slave_name.to_string(),
            name: measurement.name.clone(),
            var_type: VariableType::Measurement,
            address: measurement.address,
            address_ext: measurement.address_ext,
            data_type: measurement.data_type.type_name(),
            unit: measurement.unit.clone(),
            description: String::new(),
        }
    }

    /// Get display name with device prefix
    /// Format: "[SlaveName] variable_name"
    pub fn display_name(&self) -> String {
        format!("[{}] {}", self.slave_name, self.name)
    }

    /// Get short display name (for compact UI)
    /// Format: "variable_name (SlaveName)"
    pub fn short_display_name(&self) -> String {
        format!("{} ({})", self.name, self.slave_name)
    }

    /// Get search key (lowercase for case-insensitive search)
    pub fn search_key(&self) -> String {
        format!(
            "{} {} {} {} {} {:?}",
            self.slave_name.to_lowercase(),
            self.name.to_lowercase(),
            self.var_type.as_str().to_lowercase(),
            self.data_type.to_lowercase(),
            self.unit.as_deref().unwrap_or("").to_lowercase(),
            self.address
        )
    }

    /// Get unique identifier for this variable across all slaves
    pub fn unique_id(&self) -> String {
        format!("{}:{}:{}", self.slave_id, self.var_type.as_str(), self.name)
    }

    /// Format for list display with type info
    pub fn list_display(&self) -> String {
        let unit_str = self.unit.as_deref().unwrap_or("-");
        format!(
            "[{}] {} ({}) - {} [{}]",
            self.slave_name, self.name, self.data_type, self.var_type, unit_str
        )
    }

    /// Create compact display string
    pub fn compact_display(&self) -> String {
        format!("[{}] {}", self.slave_name, self.name)
    }
}

impl std::fmt::Display for DeviceVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Variable filter options
#[derive(Debug, Clone, Default)]
pub struct VariableFilter {
    /// Filter by slave ID
    pub slave_id: Option<SlaveId>,
    /// Filter by variable type
    pub var_type: Option<VariableType>,
    /// Filter by name pattern (case-insensitive substring)
    pub name_pattern: Option<String>,
    /// Filter by data type pattern
    pub data_type_pattern: Option<String>,
}

impl VariableFilter {
    /// Create empty filter (matches everything)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by slave ID
    pub fn with_slave(mut self, slave_id: SlaveId) -> Self {
        self.slave_id = Some(slave_id);
        self
    }

    /// Filter by variable type
    pub fn with_type(mut self, var_type: VariableType) -> Self {
        self.var_type = Some(var_type);
        self
    }

    /// Filter by name pattern
    pub fn with_name(mut self, pattern: impl Into<String>) -> Self {
        self.name_pattern = Some(pattern.into().to_lowercase());
        self
    }

    /// Filter by data type pattern
    pub fn with_data_type(mut self, pattern: impl Into<String>) -> Self {
        self.data_type_pattern = Some(pattern.into().to_lowercase());
        self
    }

    /// Check if a variable matches this filter
    pub fn matches(&self, var: &DeviceVariable) -> bool {
        // Check slave ID
        if let Some(slave_id) = self.slave_id {
            if var.slave_id != slave_id {
                return false;
            }
        }

        // Check variable type
        if let Some(var_type) = self.var_type {
            if var.var_type != var_type {
                return false;
            }
        }

        // Check name pattern
        if let Some(ref pattern) = self.name_pattern {
            if !var.name.to_lowercase().contains(pattern) {
                return false;
            }
        }

        // Check data type pattern
        if let Some(ref pattern) = self.data_type_pattern {
            if !var.data_type.to_lowercase().contains(pattern) {
                return false;
            }
        }

        true
    }
}

/// Search result with relevance score
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub variable: DeviceVariable,
    pub relevance_score: u32,
}

/// Variable search manager
///
/// Aggregates variables from all connected slaves and provides
/// search/filter functionality.
pub struct VariableSearchManager {
    /// Reference to the XCP client
    client: Arc<Mutex<crate::client::XcpClient>>,
}

impl VariableSearchManager {
    /// Create a new search manager
    pub fn new(client: Arc<Mutex<crate::client::XcpClient>>) -> Self {
        Self { client }
    }

    /// Get all variables from all slaves
    pub async fn get_all_variables(&self) -> Vec<DeviceVariable> {
        let client = self.client.lock().await;
        let mut variables = Vec::new();

        for (slave_id, slave) in client.iter_slaves() {
            let slave = slave.lock().await;
            let info = &slave.info;

            // Add calibration parameters
            for param in slave.cal_manager.params() {
                variables.push(DeviceVariable::from_calibration_param(
                    slave_id,
                    &info.name,
                    param,
                ));
            }

            // Add DAQ measurements
            let daq = slave.daq_manager.lock().await;
            for measurement in daq.all_measurements() {
                variables.push(DeviceVariable::from_measurement(
                    slave_id,
                    &info.name,
                    measurement,
                ));
            }
        }

        variables
    }

    /// Search variables by query string
    ///
    /// The query is matched against:
    /// - Variable name
    /// - Slave name
    /// - Variable type (CAL/DAQ)
    /// - Data type
    /// - Unit
    pub async fn search(&self, query: &str) -> Vec<DeviceVariable> {
        let query = query.to_lowercase();
        let all = self.get_all_variables().await;

        all.into_iter()
            .filter(|var| var.search_key().contains(&query))
            .collect()
    }

    /// Search with relevance scoring
    pub async fn search_with_scores(&self, query: &str) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        let all = self.get_all_variables().await;

        let mut results: Vec<SearchResult> = all
            .into_iter()
            .filter_map(|var| {
                let score = Self::calculate_relevance(&var, &query_lower);
                if score > 0 {
                    Some(SearchResult {
                        variable: var,
                        relevance_score: score,
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by relevance (highest first)
        results.sort_by(|a, b| b.relevance_score.cmp(&a.relevance_score));
        results
    }

    /// Calculate relevance score for a variable
    fn calculate_relevance(var: &DeviceVariable, query: &str) -> u32 {
        let mut score = 0u32;
        let name_lower = var.name.to_lowercase();
        let slave_lower = var.slave_name.to_lowercase();

        // Exact name match (highest priority)
        if name_lower == query {
            score += 1000;
        }
        // Name starts with query
        else if name_lower.starts_with(query) {
            score += 500;
        }
        // Name contains query
        else if name_lower.contains(query) {
            score += 300;
        }

        // Slave name exact match
        if slave_lower == query {
            score += 400;
        }
        // Slave name contains query
        else if slave_lower.contains(query) {
            score += 200;
        }

        // Type match
        if var.var_type.as_str().to_lowercase() == query {
            score += 100;
        }

        score
    }

    /// Get filtered variables
    pub async fn get_filtered_variables(&self, filter: &VariableFilter) -> Vec<DeviceVariable> {
        let all = self.get_all_variables().await;
        all.into_iter()
            .filter(|var| filter.matches(var))
            .collect()
    }

    /// Get variables by slave ID
    pub async fn get_variables_by_slave(&self, slave_id: SlaveId) -> Vec<DeviceVariable> {
        self.get_filtered_variables(&VariableFilter::new().with_slave(slave_id))
            .await
    }

    /// Get calibration parameters only
    pub async fn get_calibration_params(&self) -> Vec<DeviceVariable> {
        self.get_filtered_variables(&VariableFilter::new().with_type(VariableType::Calibration))
            .await
    }

    /// Get DAQ measurements only
    pub async fn get_daq_measurements(&self) -> Vec<DeviceVariable> {
        self.get_filtered_variables(&VariableFilter::new().with_type(VariableType::Measurement))
            .await
    }

    /// Get variable by unique ID
    pub async fn get_variable_by_id(&self, unique_id: &str) -> Option<DeviceVariable> {
        let all = self.get_all_variables().await;
        all.into_iter().find(|var| var.unique_id() == unique_id)
    }

    /// Get variable count summary
    pub async fn get_summary(&self) -> VariableSummary {
        let all = self.get_all_variables().await;
        let mut cal_count = 0usize;
        let mut meas_count = 0usize;
        let mut slave_counts: HashMap<SlaveId, usize> = HashMap::new();

        for var in &all {
            match var.var_type {
                VariableType::Calibration => cal_count += 1,
                VariableType::Measurement => meas_count += 1,
            }
            *slave_counts.entry(var.slave_id).or_insert(0) += 1;
        }

        VariableSummary {
            total_variables: all.len(),
            calibration_count: cal_count,
            measurement_count: meas_count,
            slave_count: slave_counts.len(),
        }
    }

    /// Check if a variable name exists on any slave
    pub async fn name_exists(&self, name: &str) -> bool {
        let all = self.get_all_variables().await;
        all.iter().any(|var| var.name == name)
    }

    /// Get all unique slave names that have variables
    pub async fn get_slave_names_with_variables(&self) -> Vec<(SlaveId, String, usize)> {
        let all = self.get_all_variables().await;
        let mut map: HashMap<SlaveId, (String, usize)> = HashMap::new();

        for var in all {
            let entry = map.entry(var.slave_id).or_insert((var.slave_name, 0));
            entry.1 += 1;
        }

        map.into_iter()
            .map(|(id, (name, count))| (id, name, count))
            .collect()
    }
}

/// Variable summary statistics
#[derive(Debug, Clone)]
pub struct VariableSummary {
    pub total_variables: usize,
    pub calibration_count: usize,
    pub measurement_count: usize,
    pub slave_count: usize,
}

impl VariableSummary {
    /// Format for display
    pub fn display(&self) -> String {
        format!(
            "Total: {} (CAL: {}, DAQ: {}) from {} slaves",
            self.total_variables,
            self.calibration_count,
            self.measurement_count,
            self.slave_count
        )
    }
}

/// Grouped variables by slave
#[derive(Debug, Clone)]
pub struct GroupedVariables {
    pub slave_id: SlaveId,
    pub slave_name: String,
    pub calibrations: Vec<DeviceVariable>,
    pub measurements: Vec<DeviceVariable>,
}

impl GroupedVariables {
    /// Get total count
    pub fn total(&self) -> usize {
        self.calibrations.len() + self.measurements.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.calibrations.is_empty() && self.measurements.is_empty()
    }
}

/// Utility function to group variables by slave
pub fn group_variables_by_slave(variables: Vec<DeviceVariable>) -> Vec<GroupedVariables> {
    use std::collections::HashMap;

    let mut map: HashMap<SlaveId, (String, Vec<DeviceVariable>, Vec<DeviceVariable>)> =
        HashMap::new();

    for var in variables {
        let entry = map
            .entry(var.slave_id)
            .or_insert_with(|| (var.slave_name.clone(), Vec::new(), Vec::new()));

        match var.var_type {
            VariableType::Calibration => entry.1.push(var),
            VariableType::Measurement => entry.2.push(var),
        }
    }

    map.into_iter()
        .map(|(id, (name, cals, meas))| GroupedVariables {
            slave_id: id,
            slave_name: name,
            calibrations: cals,
            measurements: meas,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_type_as_str() {
        assert_eq!(VariableType::Calibration.as_str(), "CAL");
        assert_eq!(VariableType::Measurement.as_str(), "DAQ");
    }

    #[test]
    fn test_variable_type_is_methods() {
        assert!(VariableType::Calibration.is_calibration());
        assert!(!VariableType::Calibration.is_measurement());
        assert!(VariableType::Measurement.is_measurement());
        assert!(!VariableType::Measurement.is_calibration());
    }

    #[test]
    fn test_device_variable_display_name() {
        let var = DeviceVariable::new(
            SlaveId::new(),
            "ECU1".to_string(),
            "engineSpeed".to_string(),
            VariableType::Measurement,
        );
        assert_eq!(var.display_name(), "[ECU1] engineSpeed");
    }

    #[test]
    fn test_device_variable_short_display_name() {
        let var = DeviceVariable::new(
            SlaveId::new(),
            "ECU1".to_string(),
            "engineSpeed".to_string(),
            VariableType::Measurement,
        );
        assert!(var.short_display_name().contains("engineSpeed"));
        assert!(var.short_display_name().contains("ECU1"));
    }

    #[test]
    fn test_device_variable_unique_id() {
        let slave_id = SlaveId::new();
        let var = DeviceVariable::new(
            slave_id,
            "ECU1".to_string(),
            "testParam".to_string(),
            VariableType::Calibration,
        );
        let unique_id = var.unique_id();
        assert!(unique_id.contains("CAL"));
        assert!(unique_id.contains("testParam"));
    }

    #[test]
    fn test_variable_filter_matches() {
        let var = DeviceVariable::new(
            SlaveId::new(),
            "ECU1".to_string(),
            "engineSpeed".to_string(),
            VariableType::Measurement,
        );

        // Empty filter matches everything
        let filter = VariableFilter::new();
        assert!(filter.matches(&var));

        // Type filter
        let filter = VariableFilter::new().with_type(VariableType::Measurement);
        assert!(filter.matches(&var));

        let filter = VariableFilter::new().with_type(VariableType::Calibration);
        assert!(!filter.matches(&var));

        // Name pattern filter
        let filter = VariableFilter::new().with_name("engine");
        assert!(filter.matches(&var));

        let filter = VariableFilter::new().with_name("speed");
        assert!(filter.matches(&var));

        let filter = VariableFilter::new().with_name("temp");
        assert!(!filter.matches(&var));
    }

    #[test]
    fn test_variable_filter_combined() {
        let slave_id = SlaveId::new();
        let var = DeviceVariable::new(
            slave_id,
            "ECU1".to_string(),
            "engineSpeed".to_string(),
            VariableType::Measurement,
        );

        // Combined filter
        let filter = VariableFilter::new()
            .with_slave(slave_id)
            .with_type(VariableType::Measurement)
            .with_name("engine");
        assert!(filter.matches(&var));

        // Wrong slave ID
        let filter = VariableFilter::new()
            .with_slave(SlaveId::new())
            .with_type(VariableType::Measurement);
        assert!(!filter.matches(&var));
    }

    #[test]
    fn test_variable_summary_display() {
        let summary = VariableSummary {
            total_variables: 100,
            calibration_count: 40,
            measurement_count: 60,
            slave_count: 3,
        };
        let display = summary.display();
        assert!(display.contains("100"));
        assert!(display.contains("CAL: 40"));
        assert!(display.contains("DAQ: 60"));
        assert!(display.contains("3 slaves"));
    }

    #[test]
    fn test_group_variables_by_slave() {
        let slave1 = SlaveId::new();
        let slave2 = SlaveId::new();

        let vars = vec![
            DeviceVariable::new(slave1, "ECU1".to_string(), "var1".to_string(), VariableType::Calibration),
            DeviceVariable::new(slave1, "ECU1".to_string(), "var2".to_string(), VariableType::Measurement),
            DeviceVariable::new(slave2, "ECU2".to_string(), "var3".to_string(), VariableType::Calibration),
        ];

        let grouped = group_variables_by_slave(vars);
        assert_eq!(grouped.len(), 2);

        // Find ECU1 group
        let ecu1 = grouped.iter().find(|g| g.slave_name == "ECU1").unwrap();
        assert_eq!(ecu1.calibrations.len(), 1);
        assert_eq!(ecu1.measurements.len(), 1);

        // Find ECU2 group
        let ecu2 = grouped.iter().find(|g| g.slave_name == "ECU2").unwrap();
        assert_eq!(ecu2.calibrations.len(), 1);
        assert_eq!(ecu2.measurements.len(), 0);
    }

    #[test]
    fn test_grouped_variables_total() {
        let group = GroupedVariables {
            slave_id: SlaveId::new(),
            slave_name: "Test".to_string(),
            calibrations: vec![DeviceVariable::new(SlaveId::new(), "Test".to_string(), "cal1".to_string(), VariableType::Calibration)],
            measurements: vec![DeviceVariable::new(SlaveId::new(), "Test".to_string(), "meas1".to_string(), VariableType::Measurement)],
        };
        assert_eq!(group.total(), 2);
        assert!(!group.is_empty());
    }

    #[test]
    fn test_device_variable_list_display() {
        let var = DeviceVariable {
            slave_id: SlaveId::new(),
            slave_name: "ECU1".to_string(),
            name: "engineSpeed".to_string(),
            var_type: VariableType::Measurement,
            address: 0x1000,
            address_ext: 0,
            data_type: "F32".to_string(),
            unit: Some("rpm".to_string()),
            description: "Engine speed".to_string(),
        };
        let display = var.list_display();
        assert!(display.contains("ECU1"));
        assert!(display.contains("engineSpeed"));
        assert!(display.contains("F32"));
        assert!(display.contains("DAQ"));
        assert!(display.contains("rpm"));
    }
}