//! Shared slave labelling for the config-file driven subcommands
//! (`daq-record`, `daq-print`).
//!
//! A slave is identified by an explicit `name` in the config or, when omitted,
//! by the stem of its A2L file. Labels appear in log lines and (for
//! `daq-record`) can be embedded into output file names, so they are made
//! unique here: duplicates get `#2`, `#3`, … suffixes.

use std::collections::HashMap;

/// Label for a slave without an explicit name: the A2L file stem.
pub fn default_slave_name(a2l: &str, idx: usize) -> String {
    std::path::Path::new(a2l)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("slave{}", idx + 1))
}

/// Allocates unique slave labels while resolving a config file.
#[derive(Debug, Default)]
pub struct SlaveNamer {
    used: HashMap<String, usize>,
}

impl SlaveNamer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve the label for the `idx`-th slave (0-based).
    ///
    /// Uses `explicit` when it is a non-empty name, otherwise the A2L file
    /// stem. Repeated labels are suffixed with `#2`, `#3`, …
    pub fn label(&mut self, explicit: Option<&str>, a2l: &str, idx: usize) -> String {
        let base = explicit
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default_slave_name(a2l, idx));

        let count = self.used.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            base
        } else {
            format!("{}#{}", base, count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_slave_name() {
        assert_eq!(default_slave_name("dir/ecu.a2l", 0), "ecu");
        assert_eq!(default_slave_name("ecu.A2L", 0), "ecu");
        assert_eq!(default_slave_name("", 2), "slave3");
    }

    #[test]
    fn test_namer_uses_explicit_name() {
        let mut namer = SlaveNamer::new();
        assert_eq!(namer.label(Some("inverter"), "ecu.a2l", 0), "inverter");
    }

    #[test]
    fn test_namer_trims_and_ignores_blank_names() {
        let mut namer = SlaveNamer::new();
        assert_eq!(namer.label(Some("  charger  "), "ecu.a2l", 0), "charger");
        assert_eq!(namer.label(Some("   "), "dir/pms.a2l", 1), "pms");
    }

    #[test]
    fn test_namer_deduplicates() {
        let mut namer = SlaveNamer::new();
        assert_eq!(namer.label(None, "a/ecu.a2l", 0), "ecu");
        assert_eq!(namer.label(None, "b/ecu.a2l", 1), "ecu#2");
        assert_eq!(namer.label(Some("ecu"), "c/other.a2l", 2), "ecu#3");
        assert_eq!(namer.label(Some("other"), "d/other.a2l", 3), "other");
    }
}
