use hm_design::{DesignInput, design};
use hm_units::parse_quantity;

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("0.016 m3/s").unwrap(),
        h: parse_quantity("13.8 m").unwrap(),
        n: parse_quantity("1800 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn volute_matches_d2_2() {
    let result = design(&input()).unwrap();
    let volute = &result.volute;

    close(result.slip, 0.8383, 0.001);
    close(result.cu2.value(), 10.363, 0.02);
    close(volute.base_radius.value(), 0.097292, 0.0001);
    close(volute.width.value(), 0.021555, 0.0001);
    close(volute.inlet_velocity.value(), 10.613, 0.02);
    close(volute.throat_area.value(), 0.0015075, 1e-6);
    close(volute.throat_diameter.value(), 0.04819, 1e-4);
    close(volute.outer_radius.value(), 0.12139, 1e-4);
}

#[test]
fn sections_are_consistent() {
    let result = design(&input()).unwrap();
    let volute = &result.volute;

    assert_eq!(volute.sections.len(), 9);
    close(volute.sections[0].angle_deg, 0.0, 1e-12);
    close(volute.sections[8].angle_deg, 360.0, 1e-12);
    close(volute.sections[0].diameter.value(), 0.0, 1e-12);
    close(
        volute.sections[0].outer_radius.value(),
        volute.base_radius.value(),
        1e-12,
    );
    close(
        volute.sections[8].area.value(),
        volute.throat_area.value(),
        1e-12,
    );
    close(
        volute.sections[8].diameter.value(),
        volute.throat_diameter.value(),
        1e-12,
    );
    close(
        volute.sections[8].outer_radius.value(),
        volute.outer_radius.value(),
        1e-12,
    );

    for pair in volute.sections.windows(2) {
        assert!(pair[0].area.value() < pair[1].area.value());
        assert!(pair[0].diameter.value() < pair[1].diameter.value());
        assert!(pair[0].outer_radius.value() < pair[1].outer_radius.value());
    }
}
