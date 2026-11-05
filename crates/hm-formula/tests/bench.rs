//! Fórmulas de reducción de banco. Los esperados se calcularon a mano, fuera
//! del motor.

use std::collections::BTreeMap;

use hm_formula::{BENCH_V1_PACK, Registry};
use hm_units::{Quantity, parse_quantity};

fn evaluate(id: &str, inputs: &[(&str, &str)]) -> f64 {
    let map: BTreeMap<String, Quantity> = inputs
        .iter()
        .map(|(name, value)| (name.to_string(), parse_quantity(value).unwrap()))
        .collect();
    Registry::all().evaluate(id, &map).unwrap().si
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}"
    );
}

#[test]
fn pack_is_audit_clean() {
    let registry = Registry::from_toml(BENCH_V1_PACK).unwrap();
    assert_eq!(registry.len(), 7);
    assert!(registry.audit().is_empty(), "{:?}", registry.audit());
}

#[test]
fn pipe_velocity() {
    let c = evaluate(
        "bench.pipe_velocity",
        &[("Q", "0.0669 m3/s"), ("D", "0.1023 m")],
    );
    close(c, 8.139_26, 1e-4);
}

#[test]
fn total_head() {
    let head = evaluate(
        "bench.total_head",
        &[
            ("p_d", "800000 Pa"),
            ("p_s", "-35000 Pa"),
            ("rho", "995.6 kg/m3"),
            ("z_d", "0.4 m"),
            ("z_s", "0.15 m"),
            ("c_d", "8.14 m/s"),
            ("c_s", "5.18 m/s"),
        ],
    );
    close(head, 87.782_8, 1e-3);
}

#[test]
fn shaft_power() {
    let power = evaluate("bench.shaft_power", &[("n", "1780 rpm"), ("T", "425 N*m")]);
    close(power, 79_220.5, 0.1);
}

#[test]
fn npsha() {
    let npsha = evaluate(
        "bench.npsha",
        &[
            ("p_abs", "66325 Pa"),
            ("p_v", "4250 Pa"),
            ("rho", "995.6 kg/m3"),
            ("z_s", "0.15 m"),
            ("c_s", "5.18 m/s"),
        ],
    );
    close(npsha, 7.875_9, 1e-3);
}

#[test]
fn similar() {
    let speeds = [("n", "1780 rpm"), ("n_sp", "1750 rpm")];
    let with = |name, value| [(name, value), speeds[0], speeds[1]];
    close(
        evaluate("bench.similar_flow", &with("Q", "0.0669 m3/s")),
        0.065_772,
        1e-5,
    );
    close(
        evaluate("bench.similar_head", &with("H", "84 m")),
        81.192_4,
        1e-3,
    );
    close(
        evaluate("bench.similar_power", &with("P", "80000 W")),
        76_022.8,
        0.1,
    );
}
