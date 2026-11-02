use std::collections::BTreeMap;

use hm_formula::{Registry, Validation};
use hm_units::{Quantity, parse_quantity};

fn evaluate(id: &str, inputs: &[(&str, &str)]) -> f64 {
    let registry = Registry::builtin();
    let map: BTreeMap<String, Quantity> = inputs
        .iter()
        .map(|(name, value)| (name.to_string(), parse_quantity(value).unwrap()))
        .collect();
    let result = registry.evaluate(id, &map).unwrap();
    result.value()
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn pack_is_audit_clean() {
    let issues = Registry::builtin().audit();
    assert!(issues.is_empty(), "problemas de auditoría: {issues:?}");
}

#[test]
fn pack_has_ten_formulas() {
    assert_eq!(Registry::builtin().len(), 10);
}

#[test]
fn pack_validation_levels() {
    let registry = Registry::builtin();
    assert!(
        registry
            .iter()
            .all(|formula| formula.validation != Validation::Bench)
    );
    assert_eq!(registry.validation_counts(), (4, 6, 0));
}

#[test]
fn head_euler() {
    let h = evaluate(
        "impeller.head_euler",
        &[
            ("u2", "30 m/s"),
            ("cu2", "20 m/s"),
            ("u1", "10 m/s"),
            ("cu1", "5 m/s"),
        ],
    );
    close(h, 56.0844, 1e-3);
}

#[test]
fn peripheral_speed() {
    let u2 = evaluate(
        "impeller.peripheral_speed",
        &[("d2", "0.3 m"), ("n", "2900 rpm")],
    );
    close(u2, 45.5531, 1e-3);
}

#[test]
fn flow_coefficient() {
    let phi = evaluate(
        "impeller.flow_coefficient",
        &[("cm2", "3 m/s"), ("u2", "30 m/s")],
    );
    close(phi, 0.1, 1e-12);
}

#[test]
fn head_coefficient() {
    let psi = evaluate(
        "impeller.head_coefficient",
        &[("H", "45 m"), ("u2", "45.5531 m/s")],
    );
    close(psi, 0.42534, 1e-4);
}

#[test]
fn meridional_velocity() {
    let cm2 = evaluate(
        "impeller.meridional_velocity",
        &[("Q", "120 m3/h"), ("d2", "0.3 m"), ("b2", "0.02 m")],
    );
    close(cm2, 1.76839, 1e-4);
}

#[test]
fn nq() {
    let nq = evaluate(
        "similarity.nq",
        &[("n", "2900 rpm"), ("Q", "120 m3/h"), ("H", "45 m")],
    );
    close(nq, 30.474, 0.01);
}

#[test]
fn hydraulic_power() {
    let power = evaluate(
        "perf.hydraulic_power",
        &[("rho", "998.2 kg/m3"), ("Q", "120 m3/h"), ("H", "45 m")],
    );
    close(power, 14683.5, 0.5);
}

#[test]
fn efficiency() {
    let eta = evaluate(
        "perf.efficiency",
        &[("P_hyd", "14683.5 W"), ("P_shaft", "18000 W")],
    );
    close(eta, 0.81575, 1e-4);
}

#[test]
fn npsha() {
    let npsha = evaluate(
        "npsh.npsha",
        &[
            ("p_s", "101325 Pa"),
            ("p_v", "2339 Pa"),
            ("rho", "998.2 kg/m3"),
            ("z", "2 m"),
            ("hf", "0.5 m"),
        ],
    );
    close(npsha, 11.6119, 1e-3);
}

#[test]
fn shaft_torque() {
    let torque = evaluate("mech.shaft_torque", &[("P", "18000 W"), ("n", "2900 rpm")]);
    close(torque, 59.2716, 1e-2);
}

#[test]
fn missing_input_is_reported() {
    let registry = Registry::builtin();
    let map = BTreeMap::new();
    assert!(registry.evaluate("impeller.head_euler", &map).is_err());
}

#[test]
fn wrong_dimension_is_reported() {
    let registry = Registry::builtin();
    let mut map = BTreeMap::new();
    map.insert("u2".to_string(), parse_quantity("30 m/s").unwrap());
    map.insert("cu2".to_string(), parse_quantity("20 m/s").unwrap());
    map.insert("u1".to_string(), parse_quantity("10 m/s").unwrap());
    map.insert("cu1".to_string(), parse_quantity("5 kg").unwrap());
    assert!(registry.evaluate("impeller.head_euler", &map).is_err());
}

#[test]
fn unknown_formula_is_reported() {
    let registry = Registry::builtin();
    assert!(registry.evaluate("no.existe", &BTreeMap::new()).is_err());
}
