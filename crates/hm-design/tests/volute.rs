use hm_design::{DesignInput, DesignOptions, design, design_with_options};
use hm_units::parse_quantity;

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("0.016 m3/s").unwrap(),
        h: parse_quantity("13.8 m").unwrap(),
        n: parse_quantity("1800 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
        nu: None,
    }
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn volute_follows_d2_2() {
    // Selecciones publicadas de D1.A1, con el ojo automático. Frente a D2.2
    // el libro toma C3 con Hth∞ (sin deslizamiento). El motor usa el cu2 de
    // Gülich con τ2 > 1, así que cu2, C3, A360 y D360 bajan respecto a τ = 1.
    let options = DesignOptions {
        psi: Some(0.87),
        phi: Some(0.13),
        beta2_rad: Some(23.5f64.to_radians()),
        // k3 de Srinivasan (ec. 5.7, 1,03..1,05); por defecto el motor usa
        // el mínimo de Gülich (tabla 10.2), 1,11 en este caso.
        k3: Some(1.04),
        // El ηv de Srinivasan (0,9722); el motor usa el de Gülich 3.5.10.
        eta_v: Some(0.9722),
        // La garganta de Srinivasan: c3q = c2. El motor usa 0,8 (Gülich).
        volute_ratio: Some(1.0),
        ..DesignOptions::default()
    };
    let result = design_with_options(&input(), &options).unwrap();
    let volute = &result.volute;
    let model = result.model_volute.as_ref().unwrap();

    close(result.slip, 0.82151, 1e-4);
    close(result.cu2.value(), 8.9692, 0.001);
    close(volute.base_radius.value(), 0.097317, 0.0001);
    close(volute.width.value(), 0.021565, 0.0001);
    close(volute.inlet_velocity.value(), 9.2577, 0.001);
    close(model.throat_area.value(), 0.001728, 1e-6);
    close(model.throat_diameter.value(), 0.05160, 1e-4);
    close(model.outer_radius.value(), 0.12312, 1e-4);
}

#[test]
fn section_diameter_follows_k_eff() {
    let result = design_with_options(
        &input(),
        &DesignOptions {
            k_eff: Some(1.25),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let section = &result.model_volute.as_ref().unwrap().sections[4];
    let equivalent = (4.0 * section.area.si / std::f64::consts::PI).sqrt();
    close(section.diameter.si / equivalent, 1.25, 1e-9);
    close(
        result.model_volute.as_ref().unwrap().sections[8]
            .diameter
            .si,
        result.model_volute.as_ref().unwrap().throat_diameter.si,
        1e-12,
    );
}

#[test]
fn sections_are_consistent() {
    let result = design(&input()).unwrap();
    let volute = &result.volute;
    let model = result.model_volute.as_ref().unwrap();

    assert_eq!(model.sections.len(), 9);
    close(model.sections[0].angle_deg, 0.0, 1e-12);
    close(model.sections[8].angle_deg, 360.0, 1e-12);
    close(model.sections[0].diameter.value(), 0.0, 1e-12);
    close(
        model.sections[0].outer_radius.value(),
        volute.base_radius.value(),
        1e-12,
    );
    close(
        model.sections[8].area.value(),
        model.throat_area.value(),
        1e-12,
    );
    close(
        model.sections[8].diameter.value(),
        model.throat_diameter.value(),
        1e-12,
    );
    close(
        model.sections[8].outer_radius.value(),
        model.outer_radius.value(),
        1e-12,
    );

    for pair in model.sections.windows(2) {
        assert!(pair[0].area.value() < pair[1].area.value());
        assert!(pair[0].diameter.value() < pair[1].diameter.value());
        assert!(pair[0].outer_radius.value() < pair[1].outer_radius.value());
    }
}
