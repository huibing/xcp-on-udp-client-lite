//! End-to-end test for structural measurement flattening via A2lReader::load.

use xcp_client::{A2lDataType, A2lReader};

#[test]
fn loads_and_flattens_structural_measurement_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("structural_measurement.a2l");
    let reader = A2lReader::load(&path).expect("load fixture");
    let measurements = reader.get_measurements();
    let by_name: std::collections::HashMap<_, _> =
        measurements.iter().map(|m| (m.name.clone(), m)).collect();

    // Plain MEASUREMENT preserved.
    let counter = by_name.get("counter").expect("counter measurement present");
    assert_eq!(counter.address, 0xFFF0);
    assert_eq!(counter.address_ext, 2);
    assert!(matches!(counter.data_type, A2lDataType::U32));

    // Three flattened components.
    let lin = by_name.get("lookup.input").expect("lookup.input present");
    assert_eq!(lin.address, 0xFFF4);
    assert_eq!(lin.address_ext, 2);
    assert!(matches!(lin.data_type, A2lDataType::F32));

    let lo = by_name
        .get("lookup.output_linear")
        .expect("lookup.output_linear present");
    assert_eq!(lo.address, 0xFFF8);

    let sp = by_name
        .get("lookup.output_spline")
        .expect("lookup.output_spline present");
    assert_eq!(sp.address, 0xFFFC);
}
