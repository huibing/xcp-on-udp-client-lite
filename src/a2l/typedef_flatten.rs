//! Flattens A2L INSTANCE + TYPEDEF_STRUCTURE + TYPEDEF_MEASUREMENT/CHARACTERISTIC
//! into synthetic A2lMeasurement / A2lCharacteristic entries.
//!
//! See `docs/superpowers/specs/2026-06-15-structural-measurements-design.md`.

/// Maximum nesting depth before we declare a cycle and stop recursing.
#[allow(dead_code)]
const MAX_DEPTH: usize = 8;

/// Builds a dotted path from an instance name and a chain of component names.
#[allow(dead_code)]
pub(super) fn build_path(instance: &str, components: &[&str]) -> String {
    let mut out = String::with_capacity(
        instance.len() + components.iter().map(|c| c.len() + 1).sum::<usize>(),
    );
    out.push_str(instance);
    for c in components {
        out.push('.');
        out.push_str(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_path_flat() {
        assert_eq!(build_path("lookup", &["input"]), "lookup.input");
    }

    #[test]
    fn build_path_nested() {
        assert_eq!(build_path("outer", &["foo", "x"]), "outer.foo.x");
    }

    #[test]
    fn build_path_no_components() {
        assert_eq!(build_path("solo", &[]), "solo");
    }
}
