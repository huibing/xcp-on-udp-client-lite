//! Array/matrix measurements: A2L parsing and expansion into scalar DAQ
//! entries (`NAME[i]`, `NAME[r][c]`).

use std::path::PathBuf;

use xcp_client::{A2lDataType, A2lReader, DaqManager, Measurement};

fn fixture_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("block_measurement.a2l")
}

#[test]
fn reader_flattens_block_components_with_matrix_dim() {
    let reader = A2lReader::load(fixture_path()).expect("load fixture");
    let by_name: std::collections::HashMap<_, _> = reader
        .get_measurements()
        .into_iter()
        .map(|m| (m.name.clone(), m))
        .collect();

    assert!(matches!(
        by_name.get("counter").expect("counter").data_type,
        A2lDataType::U32
    ));

    let dtc = by_name.get("block.dtc").expect("block.dtc present");
    assert_eq!(dtc.data_type, A2lDataType::U16Array(3));
    assert_eq!(dtc.address, 0x1000);
    assert_eq!(dtc.address_ext, 2);

    let grid = by_name.get("block.grid").expect("block.grid present");
    assert_eq!(grid.data_type, A2lDataType::U16Matrix(2, 2));
    assert_eq!(grid.address, 0x1006);

    let state = by_name.get("block.state").expect("block.state present");
    assert_eq!(state.data_type, A2lDataType::U16);
    assert_eq!(state.address, 0x100E);
}

#[test]
fn daq_manager_expands_block_measurements_into_scalar_entries() {
    let manager = DaqManager::from_a2l(fixture_path()).expect("load fixture");
    let by_name: std::collections::HashMap<_, _> = manager
        .all_measurements()
        .iter()
        .map(|m| (m.name.clone(), m))
        .collect();

    // Scalars stay untouched.
    assert_eq!(by_name["counter"].address, 0xFFF0);
    assert_eq!(by_name["counter"].data_type, A2lDataType::U32);
    assert_eq!(by_name["block.state"].address, 0x100E);

    // No block entry survives: everything is a scalar element.
    assert!(!by_name.contains_key("block.dtc"));
    assert!(!by_name.contains_key("block.grid"));
    for measurement in manager.all_measurements() {
        assert!(
            measurement.data_type.element_type().is_none(),
            "{} is still a block",
            measurement.name
        );
    }

    // 1D array → NAME[i], addresses advance by the element size (2 bytes).
    for (index, expected_addr) in [(0, 0x1000), (1, 0x1002), (2, 0x1004)] {
        let name = format!("block.dtc[{}]", index);
        let element = by_name.get(&name).expect("array element present");
        assert_eq!(element.address, expected_addr, "{}", name);
        assert_eq!(element.size, 2);
        assert_eq!(element.data_type, A2lDataType::U16);
        assert_eq!(element.address_ext, 2);
    }

    // 2D matrix → NAME[r][c] in row-major order.
    for (name, expected_addr) in [
        ("block.grid[0][0]", 0x1006),
        ("block.grid[0][1]", 0x1008),
        ("block.grid[1][0]", 0x100A),
        ("block.grid[1][1]", 0x100C),
    ] {
        let element = by_name.get(name).expect("matrix element present");
        assert_eq!(element.address, expected_addr, "{}", name);
    }
}

#[test]
fn selecting_a_block_by_base_name_selects_all_elements() {
    let mut manager = DaqManager::from_a2l(fixture_path()).expect("load fixture");
    manager.deselect_all();

    assert_eq!(manager.select_by_name("block.grid", 1), 4);
    assert_eq!(manager.selected_count(), 4);
    let names: Vec<String> = manager
        .selected_measurements()
        .iter()
        .map(|m: &Measurement| m.name.clone())
        .collect();
    assert_eq!(
        names,
        vec![
            "block.grid[0][0]",
            "block.grid[0][1]",
            "block.grid[1][0]",
            "block.grid[1][1]"
        ]
    );

    // Exact scalar selection still works, and unknown names select nothing.
    assert_eq!(manager.select_by_name("counter", 0), 1);
    assert_eq!(manager.select_by_name("missing", 0), 0);
}
