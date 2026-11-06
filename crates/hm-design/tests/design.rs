use hm_design::geometry::{BLADE_STEPS, blade_length, camber_line};
use hm_design::{
    CYLINDRICAL_BLADE_MAX_NQ, DesignInput, DesignOptions, PARAM_SPECS, REFERENCE_WATER_NU,
    blade_loading_allowable, default_mechanical_efficiency, design, design_with_options,
    designed_si, gulich_b2_star, inlet_throat_area, inlet_throat_ratio, param_spec, required_npsha,
    seal_rotation_is_laminar, stored_si_matches, validate_duty, water_density,
};
use hm_units::parse_quantity;

fn input(q: &str, h: &str, n: &str) -> DesignInput {
    let q = parse_quantity(q).unwrap();
    let n = parse_quantity(n).unwrap();
    DesignInput {
        // Con entradas inválidas la estimación falla; los tests de error
        // necesitan llegar a `design` para ver su rechazo.
        eta_m: default_mechanical_efficiency(&q, &n).unwrap_or(0.98),
        q,
        h: parse_quantity(h).unwrap(),
        n,
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        npsha: None,
        nu: None,
    }
}

#[track_caller]
fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

#[test]
fn d1a1_reference() {
    // Las selecciones publicadas de D1.A1: el test valida la cadena de
    // fórmulas contra el libro, no la selección automática del motor.
    let result = design_with_options(
        &input("0.016 m3/s", "13.8 m", "1800 rpm"),
        &d1a1_selections(),
    )
    .unwrap();

    close(result.nq, 31.7997, 0.05);
    close(result.specific_diameter_mm, 93.24, 0.5);
    close(result.selection.psi, 0.87, 1e-12);
    close(result.selection.phi, 0.13, 1e-12);
    close(result.selection.beta2_deg, 23.5, 1e-12);
    assert_eq!(result.selection.blade_count, 7);

    close(result.d2.value(), 0.1872, 0.001);
    close(result.b2.value(), 0.0122, 0.0002);
    close(result.d0.value(), 0.0764, 0.001);
    close(result.d1.value(), 0.0787, 0.001);
    close(result.b1.value(), 0.0297, 0.0005);
    close(result.u2.value(), 17.64, 0.02);
    close(result.cm2.value(), 2.293, 0.02);

    // v0.7: el ηh del método es el estadístico de Gülich (0,890 aquí); el
    // libro usa la correlación de Srinivasan por el diámetro específico,
    // 0,870, que sigue contrastada en hm-formula (hydraulic_efficiency).
    close(result.eta_h, 0.8896, 0.001);
    close(result.eta_v, 0.9722, 0.002);
    // El libro da η = 0,813 con su cadena ηh·ηv·ηm, y de ahí P = 2658,5 W.
    // Desde end_suction_v0.4 η sale del análisis de pérdidas de Gülich, y
    // desde la v0.9 el ηv del libro (0,9722) también entra en él: el motor da
    // 0,806, −0,9 % en η y +0,9 % en potencia y par. Es la diferencia del
    // modelo (pérdidas por fuga y rozamiento de discos frente a la cadena de
    // rendimientos), no un error de fórmula: la tolerancia es del 1,2 %. La
    // potencia y el par máximos escalan la P del libro con la razón P_max/P
    // del motor (curva de potencia, no publicada en D1.A1).
    let book_eta = 0.813;
    let book_p_shaft = 2161.4 / book_eta;
    let book_scale = book_p_shaft / 2683.0;
    let rel = 0.012;
    close(result.eta, book_eta, book_eta * rel);
    close(result.p_hyd.value(), 2161.4, 5.0);
    close(result.p_shaft.value(), book_p_shaft, book_p_shaft * rel);
    close(
        result.p_max.value(),
        2951.3 * book_scale,
        2951.3 * book_scale * rel,
    );
    close(
        result.torque.value(),
        14.23 * book_scale,
        14.23 * book_scale * rel,
    );
    close(
        result.torque_max.value(),
        15.66 * book_scale,
        15.66 * book_scale * rel,
    );
    // NPSH3 con cm0 = 2,25 m/s (la meridional del álabe), no con c0 = 3,6.
    // El libro no publica el NPSH de D1.A1.
    close(result.npshr_design.value(), 0.895, 0.01);
    // Gülich (3.28) da 18,0 m; la fig. D1.A2 de Srinivasan, ≈19 m (−5 %).
    close(result.shutoff_head.value(), 18.03, 0.05);
    // Con β2 = 23,5° y τ2 > 1, Euler da +4,0 % (14,35 m frente a 13,8).
    // El τ2 aplana la pendiente de §4.3.5 y la parábola con H0 estadístico
    // tiene un máximo a caudal parcial (HM-DES-031): es la parábola, no
    // una inestabilidad medida. v0.9: HM-DES-035 informa de que w1q/w1m queda
    // fuera de 0,75..0,85 en el punto de diseño de D1.A1.
    assert_eq!(
        codes(&result),
        ["HM-DES-031", "HM-DES-019", "HM-DES-035", "HM-DES-012"]
    );
}

fn d1a1_selections() -> DesignOptions {
    // Ojo y Z publicados de D1.A1. Sin ellos el motor elige el ojo que
    // minimiza el NPSH y un Z que mete β2 en 15..45°, y las medidas del
    // libro dejan de salir.
    DesignOptions {
        psi: Some(0.87),
        phi: Some(0.13),
        beta2_rad: Some(23.5f64.to_radians()),
        blade_count: Some(7),
        c0: Some(3.6),
        cm0: Some(2.25),
        j: Some(1.03),
        // El ηv de Srinivasan (0,9722). El motor usa el de Gülich 3.5.10.
        eta_v: Some(0.9722),
        ..DesignOptions::default()
    }
}

fn codes(result: &hm_design::DesignResult) -> Vec<&str> {
    result
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

#[test]
fn d1a1_duty_with_default_selections() {
    // ψ de Gülich (3.26), φ para la anchura media de Gülich (7.1) y β2 por
    // Euler: la altura de Euler coincide con la pedida.
    let input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    let result = design(&input).unwrap();

    close(result.selection.psi, 0.9472, 1e-4);
    close(result.selection.phi, 0.1040, 1e-3);
    // v0.8: τ2 y d1/d2 entran en Euler. Con Z = 7, β2 queda en 20,1°.
    close(result.selection.beta2_deg, 19.91, 0.05);
    close(result.b2.si / result.d2.si, gulich_b2_star(result.nq), 1e-6);
    let euler_head = result.eta_h * result.u2.si * result.cu2.si / 9.80665;
    close(euler_head, input.h.si, 1e-6);
    assert!(!codes(&result).contains(&"HM-DES-019"));
}

#[test]
fn existing_case_is_consistent() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();

    close(result.nq, 30.474, 0.05);
    close(result.selection.psi, 0.9569, 1e-4);
    // v0.8: Z = 7, τ2 en el triángulo de salida. β2 = 19,0°.
    close(result.selection.beta2_deg, 18.83, 0.02);
    close(result.d2.value(), 0.2000, 0.001);
    close(result.b2.value(), 0.01794, 0.0003);
    // Ojo que minimiza el NPSH3 con el cubo restado: d1 = 106 mm, b1 = 24 mm.
    close(result.d1.value(), 0.1057, 0.001);
    close(result.b1.value(), 0.0242, 0.001);
    // end_suction_v0.9 (análisis de pérdidas con el ηv de Gülich 3.5.10): 0,841;
    // v0.8 daba 0,843 y v0.3 0,816. Gülich ec. 3.9.1 da 0,812 ± 0,038.
    close(result.eta, 0.841, 0.003);
    close(result.p_hyd.value(), 14681.5, 5.0);
    close(result.p_shaft.value(), 14681.5 / 0.841437, 20.0);
    close(
        result.torque.value(),
        14681.5 / 0.841437 / (2.0 * std::f64::consts::PI * 2900.0 / 60.0),
        0.1,
    );
    // v0.8: Gülich ec. 6.10 con el cm0 del ojo óptimo.
    close(result.npshr_design.value(), 3.81, 0.02);
    close(result.shutoff_head.value(), 53.65, 0.1);
    // H0/Hd = 1,19: por debajo del 1,2 de Gülich, informativo. Desde la v0.5
    // la pendiente de la tangente (§4.3.5) es mayor de lo que admite ese H0 y
    // la curva tiene un máximo a caudal parcial (HM-DES-031). v0.9: HM-DES-035
    // (w1q/w1m fuera de 0,75..0,85, informativo).
    assert_eq!(
        codes(&result),
        ["HM-DES-017", "HM-DES-031", "HM-DES-035", "HM-DES-012"]
    );
}

#[test]
fn automatic_eye_minimizes_npsh_and_excludes_the_hub() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    assert!(result.eye_excludes_hub);
    let hub_sq = result.d1.si.powi(2) - result.d0.si.powi(2);
    assert!(
        hub_sq > 0.0,
        "d1 {:.4} d0 {:.4}",
        result.d1.si,
        result.d0.si
    );
    let hub = hub_sq.sqrt();
    let qth = result.qth.si;
    let n = input.n.si;
    let npsh_shape = |d1: f64| {
        let free = d1 * d1 - hub * hub;
        let c = qth / (std::f64::consts::PI / 4.0 * free);
        let u = std::f64::consts::PI * d1 * n;
        1.1 * c * c + 0.2 * (c * c + u * u)
    };
    let at = npsh_shape(result.d1.si);
    assert!(npsh_shape(result.d1.si * 0.98) > at);
    assert!(npsh_shape(result.d1.si * 1.02) > at);
}

#[test]
fn options_override_selections() {
    let input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    let default = design(&input).unwrap();
    let options = DesignOptions {
        psi: Some(0.5),
        blade_count: Some(9),
        ..DesignOptions::default()
    };
    let custom = design_with_options(&input, &options).unwrap();

    assert!((custom.selection.psi - 0.5).abs() < 1e-12);
    assert_eq!(custom.selection.blade_count, 9);
    assert!((custom.d2.si - default.d2.si).abs() > 1e-3);
}

#[test]
fn schema_formulas_exist() {
    let registry = hm_formula::Registry::all();
    for spec in PARAM_SPECS {
        if let Some(formula) = spec.formula {
            assert!(
                registry.get(formula).is_some(),
                "{} apunta a {formula}",
                spec.name
            );
        }
        assert!(param_spec(spec.name).is_some());
    }
    assert!(param_spec("no_existe").is_none());
}

#[test]
fn loss_breakdown_closes() {
    let result = design(&input("0.016 m3/s", "13.8 m", "1800 rpm")).unwrap();
    let total = result.p_hyd.si
        + result.p_hyd_loss.si
        + result.p_vol_loss.si
        + result.p_disk_loss.si
        + result.p_mech_loss.si;

    close(total, result.p_shaft.si, 0.5);
    assert!(result.p_hyd_loss.si > 0.0);
    assert!(result.p_vol_loss.si > 0.0);
    assert!(result.p_disk_loss.si > 0.0);
    assert!(result.p_mech_loss.si > 0.0);
}

#[test]
fn efficiency_is_the_loss_analysis() {
    // end_suction_v0.4: η = ηh ηv ηm/(1 + ηh ηv PRR/Pu) con el ηh estadístico,
    // la fuga estimada y el ηm de la entrada (sin rozamiento de discos).
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let chain = result.statistical_eta_h * result.leakage_eta_v;
    close(
        result.eta,
        chain * input.eta_m / (1.0 + chain * result.disk_friction_ratio),
        1e-12,
    );
    close(result.loss_eta, result.eta, 1e-12);
    close(
        result.p_disk_loss.si,
        result.p_hyd.si * result.disk_friction_ratio,
        1e-6,
    );
}

#[test]
fn sample_duty_with_fifteen_metres_of_npsha_has_no_npsh_findings() {
    // En 1,3 Qd: NPSH3 = 5,14·1,3³ = 11,29 m y la tabla 6.2 pide 14,1 m.
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("15 m").unwrap());
    let result = design(&input).unwrap();
    assert!(!result.diagnostics.has_errors());
    assert!(has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-011"));
}

#[test]
fn stored_snapshot_tolerates_rounding_only() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();
    let rounded: f64 = format!("{:.4}", result.specific_diameter_mm)
        .parse()
        .unwrap();
    assert!(stored_si_matches(result.specific_diameter_mm, rounded));
    assert!(!stored_si_matches(result.d2.si, 9.0));
    assert!(!stored_si_matches(
        result.model_volute.as_ref().unwrap().throat_area.si,
        0.0018
    ));
    assert_eq!(designed_si(&result, "d2"), Some(result.d2.si));
    assert!(designed_si(&result, "psi").is_none());
}

fn has(result: &hm_design::DesignResult, code: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == code)
}

#[test]
fn npsh_margin_is_enforced_inside_the_preferred_region() {
    // v0.9: la subida del NPSH3 empieza en QPS ≈ 1,1·Qd, no en Qc, y la región
    // 0,7..1,2 Qd pide ≈ 6,8 m (tabla 6.2 de Gülich en 1,2 Qd); 6 m no llega.
    // Con la voluta de la v0.8 pedía 8,6 m.
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("6 m").unwrap());
    let result = design(&input).unwrap();
    assert!(has(&result, "HM-DES-007"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn npsh_shortfall_outside_the_preferred_region_only_warns() {
    // 8 m cubre la región 0,7..1,2 Qd (≈ 6,8 m) pero no 1,3 Qd (≈ 8,6 m).
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("8 m").unwrap());
    let result = design(&input).unwrap();
    assert!(!result.diagnostics.has_errors());
    assert!(has(&result, "HM-DES-006"));
    assert!(has(&result, "HM-DES-011"));
}

#[test]
fn npsh_margin_is_checked() {
    let mut input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    input.npsha = Some(parse_quantity("1 m").unwrap());
    let result = design(&input).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HM-DES-007" && diagnostic.is_error())
    );

    input.npsha = Some(parse_quantity("6 m").unwrap());
    let result = design(&input).unwrap();
    assert!(has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-011"));
}

#[test]
fn missing_npsha_is_reported() {
    let result = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();
    assert!(has(&result, "HM-DES-012"));
    assert!(!has(&result, "HM-DES-006"));
    assert!(!has(&result, "HM-DES-007"));
}

#[test]
fn blade_angle_and_count_change_slip_and_cu2() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let base = design(&input).unwrap();

    let steeper = design_with_options(
        &input,
        &DesignOptions {
            beta2_rad: Some(35f64.to_radians()),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(steeper.slip < base.slip);
    assert!((steeper.cu2.si - base.cu2.si).abs() > 0.1);

    let fewer = design_with_options(
        &input,
        &DesignOptions {
            blade_count: Some(4),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(fewer.slip < base.slip);
    assert_eq!(designed_si(&fewer, "slip"), Some(fewer.slip));
    assert_eq!(designed_si(&fewer, "cu2"), Some(fewer.cu2.si));
}

#[test]
fn selections_outside_their_domain_are_rejected() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let cases = [
        DesignOptions {
            psi: Some(-1.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            phi: Some(0.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            beta2_rad: Some(90f64.to_radians()),
            ..DesignOptions::default()
        },
        DesignOptions {
            blade_count: Some(0),
            ..DesignOptions::default()
        },
        DesignOptions {
            margin: Some(0.5),
            ..DesignOptions::default()
        },
        DesignOptions {
            k3: Some(1.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            k_eff: Some(0.0),
            ..DesignOptions::default()
        },
        DesignOptions {
            j: Some(f64::NAN),
            ..DesignOptions::default()
        },
    ];
    for options in cases {
        assert!(
            design_with_options(&input, &options).is_err(),
            "se aceptó {options:?}"
        );
    }
}

#[test]
fn eta_m_bounds_are_exclusive() {
    let mut input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    input.eta_m = 0.0;
    assert!(design(&input).is_err());

    input.eta_m = 1.0;
    assert!(design(&input).is_err());
}

#[test]
fn zero_flow_is_rejected() {
    assert!(design(&input("0 m3/s", "45 m", "2900 rpm")).is_err());
}

#[test]
fn wrong_dimension_is_rejected() {
    assert!(design(&input("120 m", "45 m", "2900 rpm")).is_err());
}

#[test]
fn water_density_follows_kell() {
    for (celsius, expected) in [
        (0.0, 999.840),
        (4.0, 999.972),
        (20.0, 998.204),
        (80.0, 971.798),
        (100.0, 958.364),
    ] {
        let density = water_density(celsius + 273.15).unwrap();
        close(density, expected, 0.002);
    }
    assert!(water_density(273.15 - 1.0).is_none());
    assert!(water_density(273.15 + 151.0).is_none());
    assert!(water_density(f64::NAN).is_none());
}

#[test]
fn required_npsha_follows_gulich_table_6_2() {
    // u1 < 10 m/s: max(1,25·NPSH3; NPSH3 + 0,6 m). El cuarto argumento es qSF.
    close(required_npsha(2.0, 7.4, 1.0, 1.0), 2.6, 1e-12);
    close(required_npsha(4.0, 7.4, 1.0, 1.0), 5.0, 1e-12);

    // 10..50 m/s: NPSH3 + FR·{0,05 + 2(u1/100)³}·u1²/2g, con el mínimo.
    let u1: f64 = 30.0;
    let term = (0.05 + 2.0 * 0.3f64.powi(3)) * u1.powi(2) / (2.0 * 9.80665);
    close(required_npsha(2.0, u1, 1.0, 1.0), 2.0 + term, 1e-12);
    close(required_npsha(2.0, u1, 1.2, 1.2), 2.0 + 1.2 * term, 1e-12);
    // 0,8 < q ≤ 1,1 y q ≤ qSF: FR = 1.
    close(required_npsha(2.0, u1, 1.05, 1.05), 2.0 + term, 1e-12);
    // q > qSF, aunque q esté entre 0,8 y 1,1: FR = 1,2.
    close(required_npsha(2.0, u1, 1.05, 1.0), 2.0 + 1.2 * term, 1e-12);
    close(required_npsha(2.0, u1, 0.9, 1.0), 2.0 + term, 1e-12);
    // q = 0,8 exacto queda fuera del intervalo abierto.
    close(required_npsha(2.0, u1, 0.8, 1.0), 2.0 + 1.2 * term, 1e-12);
    // Con NPSH3 grande domina el mínimo de 1,25·NPSH3.
    close(required_npsha(20.0, u1, 1.0, 1.0), 25.0, 1e-12);

    // Por encima de 50 m/s solo el mínimo.
    close(required_npsha(10.0, 60.0, 1.0, 1.0), 12.5, 1e-12);
}

#[test]
fn b2_star_reference_follows_gulich_7_1() {
    close(gulich_b2_star(31.8), 0.0925, 1e-4);
    close(gulich_b2_star(100.0), 0.2083, 1e-4);
}

#[test]
fn high_nq_widths_are_no_longer_flagged() {
    // Antes HM-DES-003 marcaba todo b2/d2 > 0,15; con nq = 62 la media de
    // Gülich ya es 0,151.
    let input = input("500 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    assert!(result.nq > 60.0);
    assert!(!has(&result, "HM-DES-003"), "{:?}", result.diagnostics);
}

#[test]
fn wide_blade_angles_inside_gulich_range_are_accepted() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design_with_options(
        &input,
        &DesignOptions {
            beta2_rad: Some(40f64.to_radians()),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(!has(&result, "HM-DES-004"));
}

#[test]
fn de_haller_ratio_is_reported() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let ratio = result.w2.si / result.w1.si;
    assert!(ratio > 0.7, "w2/w1 = {ratio}");
    assert!(!has(&result, "HM-DES-014"));

    // c0 = 2,2 m/s agranda el ojo y w2/w1 baja de 0,7. Más lento (c0 = 1 m/s)
    // deja d1/d2 tan alto que el kw de Wiesner ya no alcanza la altura.
    let slow = design_with_options(
        &input,
        &DesignOptions {
            c0: Some(2.2),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let ratio = slow.w2.si / slow.w1.si;
    assert!(ratio < 0.7, "w2/w1 = {ratio}");
    assert!(has(&slow, "HM-DES-014"));
}

#[test]
fn shutoff_head_below_design_head_is_an_error() {
    // ψo(nq = 30,5) = 1,142; con ψ = 1,2, H0/Hd = 0,95.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design_with_options(
        &input,
        &DesignOptions {
            psi: Some(1.2),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(result.shutoff_head.si < input.h.si);
    assert!(has(&result, "HM-DES-016"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn flat_head_rise_warns_below_nq_40() {
    // Con ψ = 1,0, H0/Hd = 1,142/1,0 = 1,14 < 1,2.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design_with_options(
        &input,
        &DesignOptions {
            psi: Some(1.0),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(has(&result, "HM-DES-017"));
    assert!(!has(&result, "HM-DES-016"));
    assert!(!result.diagnostics.has_errors());

    // Con ψ y ψo medios de Gülich, H0/Hd queda en 1,19: solo informa.
    let default = design(&input).unwrap();
    assert!(has(&default, "HM-DES-017"));
    assert!(!default.diagnostics.has_errors());
}

#[test]
fn inlet_speed_above_50_m_s_is_reported() {
    // A 2000 m3/h y 3000 rpm el ojo que minimiza el NPSH3 se queda en
    // u1 ≈ 44 m/s. Hay que subir el caudal para pasar de 50 m/s.
    let mut input = input("4000 m3/h", "200 m", "3000 rpm");
    input.npsha = Some(parse_quantity("100 m").unwrap());
    let result = design(&input).unwrap();
    assert!(result.u1.si > 50.0, "u1 = {}", result.u1.si);
    assert!(has(&result, "HM-DES-015"));
}

#[test]
fn manual_beta2_reports_the_euler_head_mismatch() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let solved = design(&input).unwrap();
    let off = design_with_options(
        &input,
        &DesignOptions {
            beta2_rad: Some((solved.selection.beta2_deg + 8.0).to_radians()),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(has(&off, "HM-DES-019"));
    assert!(!has(&solved, "HM-DES-019"));
}

#[test]
fn unreachable_head_is_an_error() {
    // Con ψ = 1,4, ηh = 0,904 y Z = 7 haría falta cu2/u2 = 0,77 y el máximo
    // es 0,73.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let error = design_with_options(
        &input,
        &DesignOptions {
            psi: Some(1.4),
            ..DesignOptions::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ningún β2"), "{error}");
}

#[test]
fn transition_comes_from_the_geometry() {
    // Gülich ec. 6.17-6.20 con el diseño por defecto de 120 m3/h y 45 m:
    // con la garganta de 0,8 la voluta deja de mandar y la subida empieza en QPS.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let qd = input.q.si;
    close(result.shockless_flow.si / qd, 1.119, 0.005);
    // v0.9: Qc por 6.22a (con c2m) con la garganta de c3q/c2 = 0,8: 1,15·Qd.
    // Con 6.19 y la garganta de Srinivasan salía 0,989·Qd.
    close(result.volute_matching_flow.si / qd, 1.151, 0.005);
    // v0.9: τ1 sube β1B y divide QSF; la garganta de QW también se cierra
    // con el espesor (A1·sen β1B/τ1), así que QW queda por debajo de QSF.
    assert!(
        result.throat_flow.si < result.shockless_flow.si,
        "QW {:.3} QSF {:.3}",
        result.throat_flow.si,
        result.shockless_flow.si
    );
    // Qc queda por encima de QPS = (QSF + QW)/2: la subida empieza en QPS.
    let pressure_side = 0.5 * (result.shockless_flow.si + result.throat_flow.si);
    assert!(pressure_side < result.volute_matching_flow.si);
    close(result.transition_ratio, pressure_side / qd, 1e-12);
    assert!(!has(&result, "HM-DES-025"));
    close(result.recirculation_onset, 0.513, 0.002);
    assert!(!has(&result, "HM-DES-026"));
}

#[test]
fn high_nq_recirculation_is_reported() {
    // nq ≈ 98: recirculación desde 0,63·Qd (±0,1). Con 6.22a (a > 1) el Qc
    // de la voluta sale en torno al de diseño (≥ 0,95·Qd), así que no hay
    // aviso de transición (con 6.19 salía 0,93·Qd y HM-DES-025).
    let input = input("954 m3/h", "15 m", "1450 rpm");
    let result = design(&input).unwrap();
    assert!(
        result.transition_ratio >= 0.95,
        "{}",
        result.transition_ratio
    );
    assert!(!has(&result, "HM-DES-025"));
    assert!(has(&result, "HM-DES-026"));
}

#[test]
fn loss_estimates_follow_gulich() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    // v0.9: ηv de Gülich 3.5.10; con nq ≥ 27, 1/(1 + 0,15·nq^−0,6). Y
    // 1 − 0,0045·(1/Q)^0,4·(1500/n)^0,3 para ηm.
    close(
        result.leakage_eta_v,
        1.0 / (1.0 + 0.15 * result.nq.powf(-0.6)),
        1e-12,
    );
    close(result.mechanical_eta_estimate, 0.98561, 1e-4);
}

#[test]
fn curve_follows_the_registered_formula() {
    // curve_head es la fórmula design.curve_head_ratio con h0 = H0/Hd y la
    // pendiente de la tangente.
    use hm_formula::Registry;
    use std::collections::BTreeMap;
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let registry = Registry::all();
    for ratio in [0.0, 0.3, 0.7, 1.0, 1.3] {
        let inputs: BTreeMap<String, hm_units::Quantity> = [
            ("h0", result.shutoff_head.si / input.h.si),
            ("slope", result.head_slope),
            ("q", ratio),
        ]
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                parse_quantity(&value.to_string()).unwrap(),
            )
        })
        .collect();
        let expected = registry
            .evaluate("design.curve_head_ratio", &inputs)
            .unwrap()
            .value();
        close(
            hm_design::curve_head(&input, &result, ratio) / input.h.si,
            expected,
            1e-12,
        );
    }
    close(
        hm_design::curve_head(&input, &result, 0.0),
        result.shutoff_head.si,
        1e-9,
    );
    close(
        hm_design::curve_head(&input, &result, 1.0),
        input.h.si,
        1e-9,
    );
}

#[test]
fn seal_leakage_follows_the_ring_geometry() {
    // Gülich tabla 3.7: más diámetro de anillo (ojo mayor) o más holgura,
    // más fuga y menos ηv.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let base = design(&input).unwrap();
    assert!((0.9..1.0).contains(&base.seal_eta_v), "{}", base.seal_eta_v);
    close(
        base.seal_eta_v,
        1.0 / (1.0 + base.seal_leakage_ratio),
        1e-12,
    );

    let wider_eye = design_with_options(
        &input,
        &DesignOptions {
            j: Some(1.2),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(wider_eye.seal_eta_v < base.seal_eta_v);

    let worn = design_with_options(
        &input,
        &DesignOptions {
            seal_clearance: Some(1.0e-3),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(worn.seal_eta_v < base.seal_eta_v - 0.01);
    // Es un contraste: el η del método no cambia.
    close(worn.eta, base.eta, 1e-12);
}

#[test]
fn viscous_ring_leakage_still_converges() {
    // 20 cSt baja el Reynolds del anillo al régimen laminar. La sustitución
    // directa de λ oscila; el diseño tiene que cerrar igual.
    let water = input("120 m3/h", "45 m", "2900 rpm");
    let sized = design(&water).unwrap();
    let mut viscous = water;
    viscous.nu = Some(20.0e-6);
    let result = design_with_options(
        &viscous,
        &DesignOptions {
            psi: Some(sized.selection.psi),
            phi: Some(sized.selection.phi),
            beta2_rad: Some(sized.selection.beta2_deg.to_radians()),
            blade_count: Some(sized.selection.blade_count),
            c0: Some(sized.selection.c0),
            cm0: Some(sized.selection.cm0),
            j: Some(sized.selection.j),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(
        result.seal_eta_v.is_finite() && (0.0..=1.0).contains(&result.seal_eta_v),
        "{}",
        result.seal_eta_v
    );
    assert!(result.eta.is_finite() && result.eta > 0.0, "{}", result.eta);
}

#[test]
fn one_hydraulic_efficiency_for_euler_and_power() {
    // end_suction_v0.7: el ηh que cierra Euler (β2) es el mismo que entra en
    // el análisis de pérdidas, el estadístico de Gülich (ec. 3.9.6). Con él,
    // ηh·u2·cu2/g da la altura pedida.
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("120 m3/h", "45 m", "2900 rpm"),
        ("0.016 m3/s", "13.8 m", "1800 rpm"),
    ] {
        let input = input(q, h, n);
        let result = design(&input).unwrap();
        close(result.eta_h, result.statistical_eta_h, 1e-12);
        let euler_head = result.eta_h * result.u2.si * result.cu2.si / 9.806_65;
        close(euler_head, input.h.si, 1e-6 * input.h.si);
    }
}

#[test]
fn euler_closes_head_where_slip_peaks_before_89_degrees() {
    // cu2/u2 = γ(β2) − τ·φ/tan β2 no crece siempre con β2: el deslizamiento de
    // Wiesner (√sen β2) lo hace pasar por un máximo hacia 25°. A nq ≈ 10 la
    // altura pedida (0,727·u2) cae entre el valor a 89° (0,700 con Z = 6) y
    // ese máximo (0,729), y el motor la daba por inalcanzable.
    let input = input("60 m3/h", "120 m", "2900 rpm");
    let result = design(&input).expect("hay un β2 que da la altura");
    let beta2 = result.selection.beta2_deg;
    assert!(beta2 > 5.0 && beta2 < 60.0, "β2 {beta2}");
    let euler_head = result.eta_h * result.u2.si * result.cu2.si / 9.806_65;
    close(euler_head, input.h.si, 1e-6 * input.h.si);
}

#[test]
fn blade_count_search_goes_up_when_seven_blades_fall_short() {
    // A nq ≈ 9 con Z = 7 el máximo de cu2/u2 (0,761) no llega a lo pedido
    // (0,766). Con más álabes sí llega: el recorrido de Z tiene que seguir
    // subiendo en vez de abandonar con el error del primer Z.
    let input = input("60 m3/h", "150 m", "2900 rpm");
    let result = design(&input).expect("Z = 8 o 9 alcanzan la altura");
    assert!(
        result.selection.blade_count > 7,
        "Z {}",
        result.selection.blade_count
    );
    let euler_head = result.eta_h * result.u2.si * result.cu2.si / 9.806_65;
    close(euler_head, input.h.si, 1e-6 * input.h.si);
}

#[test]
fn statistical_efficiency_warns_outside_its_flow_range() {
    // El ηh de Gülich (ec. 3.9.6) vale para 0,005 ≤ Q ≤ 1 m³/s. Con 15 m³/h
    // (0,0042 m³/s) el diseño sale, pero con ηh extrapolado: tiene que avisar.
    let small = design(&input("15 m3/h", "10 m", "2900 rpm")).unwrap();
    assert!(
        small.diagnostics.iter().any(|d| d.code == "HM-DES-033"),
        "{:?}",
        codes(&small)
    );
    let inside = design(&input("120 m3/h", "45 m", "2900 rpm")).unwrap();
    assert!(!inside.diagnostics.iter().any(|d| d.code == "HM-DES-033"));
}

#[test]
fn failed_euler_names_the_flow_range_of_the_efficiency() {
    // A 5 m³/h el ηh extrapolado sale ≈ 0,37 y Euler no cierra. El error tiene
    // que decir que el caudal está fuera del rango del ηh, no solo «aumenta Z».
    let error = design(&input("5 m3/h", "60 m", "2900 rpm")).unwrap_err();
    let text = error.to_string();
    assert!(text.contains("0,005") && text.contains("ηh"), "{text}");
}

#[test]
fn non_positive_statistical_efficiency_is_named_in_the_error() {
    // A 4 m³/h y 150 m (nq ≈ 2) la extrapolación da ηh ≈ −0,01. Con ηh ≤ 0 la
    // potencia sale negativa y el error llegaba como «el eje necesita un par
    // positivo»: tiene que nombrar ηh y el rango de su fórmula.
    for (q, h, n) in [
        ("4 m3/h", "150 m", "2900 rpm"),
        ("3 m3/h", "80 m", "1450 rpm"),
    ] {
        let text = design(&input(q, h, n)).unwrap_err().to_string();
        assert!(text.contains("ηh") && text.contains("0,005"), "{text}");
        assert!(!text.contains("eje"), "{text}");
    }
}

#[test]
fn one_volumetric_efficiency_for_sizing_and_losses() {
    // end_suction_v0.9: el ηv que dimensiona Qth, φ y b2 es el mismo que entra
    // en el análisis de pérdidas, el de Gülich 3.5.10 (nq < 27: 4,1/nq^1,6).
    let q = input("1060 gpm", "83 m", "1750 rpm");
    let result = design(&q).unwrap();
    close(result.eta_v, result.leakage_eta_v, 1e-12);
    close(result.eta_v, 0.95566, 1e-4);
    close(result.qth.si, q.q.si / result.eta_v, 1e-9);
}

#[test]
fn balance_holes_double_the_leakage_ratio() {
    // zH = 2 con agujeros de equilibrio (Gülich 3.5.10): Qsp/Q se duplica.
    let q = input("1060 gpm", "83 m", "1750 rpm");
    let base = design(&q).unwrap();
    let holes = design_with_options(
        &q,
        &DesignOptions {
            balance_holes: true,
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let ratio = |eta_v: f64| 1.0 / eta_v - 1.0;
    close(ratio(holes.eta_v), 2.0 * ratio(base.eta_v), 1e-9);
    close(holes.eta_v, holes.leakage_eta_v, 1e-12);
}

#[test]
fn explicit_volumetric_efficiency_replaces_the_estimate() {
    let q = input("120 m3/h", "45 m", "2900 rpm");
    let result = design_with_options(
        &q,
        &DesignOptions {
            eta_v: Some(0.95),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    close(result.eta_v, 0.95, 1e-12);
    close(result.leakage_eta_v, 0.95, 1e-12);
    close(result.qth.si, q.q.si / 0.95, 1e-12);
    for bad in [0.0, 1.0, 1.2, f64::NAN] {
        let error = design_with_options(
            &q,
            &DesignOptions {
                eta_v: Some(bad),
                ..DesignOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("eta_v"), "{error}");
    }
}

#[test]
fn inlet_throat_area_shrinks_with_blade_blockage() {
    // zLa·A1q = A1·sen β1B/τ1: el metal de los álabes cierra la garganta.
    let beta = 20f64.to_radians();
    close(inlet_throat_area(0.01, beta, 1.0), 0.003_420_201_4, 1e-9);
    close(
        inlet_throat_area(0.01, beta, 1.28),
        0.003_420_201_4 / 1.28,
        1e-9,
    );
}

#[test]
fn throat_flow_is_below_shockless_flow_with_blockage() {
    // Gülich 6.18 con la garganta con espesor: QW < QSF cuando τ1 > 1. Con el
    // área sin espesor salía QW = τ1·QSF (1,43·Qd frente a 1,12·Qd en B1).
    let result = design(&input("1060 gpm", "83 m", "1750 rpm")).unwrap();
    assert!(
        result.throat_flow.si < result.shockless_flow.si,
        "QW {} frente a QSF {}",
        result.throat_flow.si,
        result.shockless_flow.si
    );
}

#[test]
fn volute_matching_flow_is_the_design_flow_when_c3q_equals_c2() {
    // Gülich 6.22a (con c2m): con la garganta de Srinivasan (c3q = c2) el
    // caudal en que c3q = c2 es el de diseño. Con 6.19 (sin c2m) salía
    // 0,92·Qd en nq ≈ 87.
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("500 m3/h", "60 m", "1450 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
    ] {
        let input = input(q, h, n);
        let result = design_with_options(
            &input,
            &DesignOptions {
                volute_ratio: Some(1.0),
                ..DesignOptions::default()
            },
        )
        .unwrap();
        close(result.volute_matching_flow.si / input.q.si, 1.0, 1e-6);
    }
}

#[test]
fn default_volute_leaves_margin_over_the_design_flow() {
    // Con c3q/c2 = 0,8 la voluta cruza c3q = c2 por encima de Qd, también con
    // nq alto (Gülich pide 1,1..1,3·Qd con nq bajo).
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("500 m3/h", "60 m", "1450 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
        ("150 m3/h", "10 m", "1450 rpm"),
    ] {
        let input = input(q, h, n);
        let result = design(&input).unwrap();
        let ratio = result.volute_matching_flow.si / input.q.si;
        assert!((1.1..1.3).contains(&ratio), "{q}: {ratio}");
    }
}

/// Qc/Qd de Gülich 6.22a en su forma simplificada, `u2·A2·γ/(√(a² − 1) + t)`
/// con `t = τ2/tan β2B`, calculada con los campos del resultado. Es idéntica a
/// la forma del libro, `(√(a²−1) − t)/(a² − 1 − t²)`, y vale para todo a > 1.
fn matching_flow_ratio(input: &DesignInput, result: &hm_design::DesignResult) -> (f64, f64) {
    let a2 = std::f64::consts::PI * result.d2.si * result.b2.si;
    let a =
        a2 * input.q.si / (result.model_volute.as_ref().unwrap().throat_area.si * result.qth.si);
    let beta2 = result.selection.beta2_deg.to_radians();
    let tau2 = (result.u2.si * result.slip - result.cu2.si) * beta2.tan() / result.cm2.si;
    let impeller_flow =
        result.u2.si * a2 * result.slip / ((a * a - 1.0).max(0.0).sqrt() + tau2 / beta2.tan());
    (a, impeller_flow / result.qth.si)
}

#[test]
fn matching_flow_follows_the_simplified_cm_equation() {
    // Gülich 6.22a es finita y positiva siempre que a > 1; el motor informaba
    // 10·Qd (v0.9 antes de la revisión) cuando √(a² − 1) < τ2/tan β2B.
    for (q, h, n) in [
        ("120 m3/h", "45 m", "2900 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
        ("500 m3/h", "40 m", "2900 rpm"),
        ("150 m3/h", "10 m", "1450 rpm"),
        ("1500 m3/h", "15 m", "1450 rpm"),
    ] {
        let input = input(q, h, n);
        let result = design(&input).unwrap();
        let (a, expected) = matching_flow_ratio(&input, &result);
        assert!(a > 1.0, "{q}: a = {a}");
        close(
            result.volute_matching_flow.si / input.q.si,
            expected,
            1e-6 * expected,
        );
        assert!((1.0..3.0).contains(&expected), "{q}: Qc/Qd = {expected}");
    }
}

#[test]
fn matching_flow_is_capped_only_when_the_throat_exceeds_a2_eta_v() {
    // a = A2·Q/(A3q·Qth) ≤ 1: la garganta es mayor que A2·ηv y c3q no alcanza
    // c2 en ningún caudal. Qc se toma como 10·Qd, finito para el informe JSON.
    let input = input("1500 m3/h", "15 m", "1450 rpm");
    let result = design_with_options(
        &input,
        &DesignOptions {
            volute_ratio: Some(0.5),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let (a, _) = matching_flow_ratio(&input, &result);
    assert!(a <= 1.0, "a = {a}: el caso ya no cumple la condición");
    assert!(result.volute_matching_flow.si.is_finite());
    close(
        result.volute_matching_flow.si / input.q.si,
        hm_design::NO_MATCHING_FLOW_RATIO,
        1e-9,
    );
    assert!(result.transition_ratio < hm_design::NO_MATCHING_FLOW_RATIO);
}

#[test]
fn no_spurious_transition_warning_at_high_nq() {
    let result = design(&input("200 m3/h", "8 m", "1750 rpm")).unwrap();
    assert!(
        result.transition_ratio >= 0.95,
        "{}",
        result.transition_ratio
    );
    assert!(!has(&result, "HM-DES-025"));
}

#[test]
fn volute_throat_follows_the_deceleration_ratio() {
    // end_suction_v0.9: A360 = Q/(ratio·c2) con ratio = 0,8 por defecto
    // (Gülich T7.7.11, tabla 10.2 fila 9). Con ratio = 1 sale la de Srinivasan.
    let input = input("1060 gpm", "83 m", "1750 rpm");
    let result = design(&input).unwrap();
    let c2 = result.volute.inlet_velocity.si;
    close(
        result.model_volute.as_ref().unwrap().throat_area.si,
        input.q.si / (0.8 * c2),
        1e-9,
    );
    let equivalent =
        (4.0 * result.model_volute.as_ref().unwrap().throat_area.si / std::f64::consts::PI).sqrt();
    close(
        result.model_volute.as_ref().unwrap().throat_diameter.si,
        1.1 * equivalent,
        1e-9,
    );

    let srinivasan = design_with_options(
        &input,
        &DesignOptions {
            volute_ratio: Some(1.0),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    close(
        srinivasan.model_volute.as_ref().unwrap().throat_area.si,
        input.q.si / srinivasan.volute.inlet_velocity.si,
        1e-9,
    );
    close(
        result.model_volute.as_ref().unwrap().throat_area.si
            / srinivasan.model_volute.as_ref().unwrap().throat_area.si,
        1.0 / 0.8,
        1e-9,
    );
}

#[test]
fn invalid_volute_ratio_is_rejected() {
    let q = input("120 m3/h", "45 m", "2900 rpm");
    for bad in [0.4, 1.2, f64::NAN] {
        let error = design_with_options(
            &q,
            &DesignOptions {
                volute_ratio: Some(bad),
                ..DesignOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("volute_ratio"), "{error}");
    }
}

#[test]
fn momentum_throat_is_a_contrast_that_agrees_at_low_nq() {
    // T7.7.7 (momento cinético) y la razón 0,8 coinciden en B1 (nq 16,5) y
    // se separan con nq alto, donde el momento cinético pide c3q/c2 ≈ 0,4.
    let equivalent = |r: &hm_design::DesignResult| {
        (4.0 * r.model_volute.as_ref().unwrap().throat_area.si / std::f64::consts::PI).sqrt()
    };
    let b1 = design(&input("1060 gpm", "83 m", "1750 rpm")).unwrap();
    let low = b1
        .model_volute
        .as_ref()
        .unwrap()
        .momentum_throat_diameter
        .si
        / equivalent(&b1);
    assert!((low - 1.0).abs() < 0.03, "B1: {low}");
    let high = design(&input("200 m3/h", "8 m", "1750 rpm")).unwrap();
    let ratio = high
        .model_volute
        .as_ref()
        .unwrap()
        .momentum_throat_diameter
        .si
        / equivalent(&high);
    assert!(ratio > 1.25, "nq 87: {ratio}");
}

#[test]
fn seal_rotation_regime_uses_the_transition_reynolds_number() {
    // Gülich 3.7.10: Re* = √(Re² + ¼·Reu²); laminar si Re* < 2000.
    assert!(seal_rotation_is_laminar(100.0, 1000.0));
    assert!(seal_rotation_is_laminar(1900.0, 1000.0)); // Re* = 1964,7
    assert!(!seal_rotation_is_laminar(1950.0, 1000.0)); // Re* = 2013,1
    assert!(!seal_rotation_is_laminar(5000.0, 1000.0));
}

#[test]
fn laminar_seal_leakage_scales_with_inverse_viscosity() {
    // En régimen laminar con poca rotación (Reu ≪ 2000) la fuga del anillo
    // sigue a Hagen–Poiseuille, Qsp ∝ 1/ν: duplicar la viscosidad la divide
    // por dos. Con el factor turbulento aplicado también al caso laminar
    // (v0.8) la escala era otra.
    let water = input("120 m3/h", "45 m", "2900 rpm");
    let sized = design(&water).unwrap();
    let leak = |nu: f64| {
        let mut viscous = water.clone();
        viscous.nu = Some(nu);
        design_with_options(
            &viscous,
            &DesignOptions {
                psi: Some(sized.selection.psi),
                phi: Some(sized.selection.phi),
                beta2_rad: Some(sized.selection.beta2_deg.to_radians()),
                blade_count: Some(sized.selection.blade_count),
                c0: Some(sized.selection.c0),
                cm0: Some(sized.selection.cm0),
                j: Some(sized.selection.j),
                ..DesignOptions::default()
            },
        )
        .unwrap()
        .seal_leakage_ratio
    };
    let ratio = leak(2.0e-3) / leak(1.0e-3);
    assert!(
        (0.45..0.55).contains(&ratio),
        "Qsp(2000 cSt)/Qsp(1000 cSt) = {ratio}"
    );
    // 4000 cSt es el límite de la tabla 13.1: también cierra y sigue la escala.
    let ratio = leak(4.0e-3) / leak(2.0e-3);
    assert!(
        (0.45..0.55).contains(&ratio),
        "Qsp(4000 cSt)/Qsp(2000 cSt) = {ratio}"
    );
}

#[test]
fn rough_impeller_raises_disk_loss() {
    // Gülich 3.10.7: fR = {log(12,5/Re)/log(0,2·ε/r2 + 12,5/Re)}^2,15 con
    // Re = ω·r2²/ν. Se multiplica el kRR del segundo término de 3.10.9.
    let q = input("1060 gpm", "83 m", "1750 rpm");
    let smooth = design(&q).unwrap();
    let eps = 50.0e-6;
    let rough = design_with_options(
        &q,
        &DesignOptions {
            impeller_roughness: Some(eps),
            ..DesignOptions::default()
        },
    )
    .unwrap();

    let r2 = 0.5 * smooth.d2.si;
    let re = 2.0 * std::f64::consts::PI * q.n.si * r2 * r2 / REFERENCE_WATER_NU;
    let factor = ((12.5 / re).log10() / (0.2 * eps / r2 + 12.5 / re).log10()).powf(2.15);
    assert!(factor > 1.5, "fR {factor}");
    close(
        rough.disk_friction_ratio / smooth.disk_friction_ratio,
        factor,
        1e-6 * factor,
    );
    assert!(rough.p_disk_loss.si > smooth.p_disk_loss.si);
    assert!(rough.eta < smooth.eta);
}

#[test]
fn invalid_impeller_roughness_is_rejected() {
    let q = input("120 m3/h", "45 m", "2900 rpm");
    for bad in [-1.0e-6, f64::NAN, f64::INFINITY, 0.05] {
        let error = design_with_options(
            &q,
            &DesignOptions {
                impeller_roughness: Some(bad),
                ..DesignOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("impeller_roughness"), "{bad}: {error}");
    }
}

/// β2 que da Euler con un Z fijado a mano.
fn beta2_with(input: &DesignInput, z: u32) -> f64 {
    design_with_options(
        input,
        &DesignOptions {
            blade_count: Some(z),
            ..DesignOptions::default()
        },
    )
    .unwrap()
    .selection
    .beta2_deg
}

#[test]
fn blade_count_is_the_highest_of_five_to_seven_with_a_usual_outlet_angle() {
    // Gülich p. 408: 5 a 7 álabes en 10 < nq < 120; p. 412: β2B de 15° a 45°.
    // El libro fija Z antes que β2 y no da un criterio entre 5, 6 y 7: el
    // motor prueba de 7 hacia abajo y se queda con el primero cuyo β2 cabe.
    for (q, h, n) in [
        ("120 m3/h", "45 m", "2900 rpm"),
        ("500 m3/h", "60 m", "1450 rpm"),
        ("800 m3/h", "20 m", "1450 rpm"),
        ("1060 gpm", "83 m", "1750 rpm"),
        ("80 m3/h", "160 m", "2900 rpm"),
    ] {
        let input = input(q, h, n);
        let expected = (5..=7u32)
            .rev()
            .find(|&z| (15.0..=45.0).contains(&beta2_with(&input, z)))
            .unwrap();
        let result = design(&input).unwrap();
        assert_eq!(result.selection.blade_count, expected, "{q}");
        assert!(!has(&result, "HM-DES-034"), "{q}");
    }
}

#[test]
fn b1_keeps_five_blades() {
    let result = design(&input("1060 gpm", "83 m", "1750 rpm")).unwrap();
    assert_eq!(result.selection.blade_count, 5);
    close(result.selection.beta2_deg, 15.7, 0.05);
}

#[test]
fn more_than_seven_blades_are_reported() {
    // nq ≈ 123: con 5..7 álabes β2 se sale de 15..45° y el motor amplía a 8 y
    // 9; Gülich no recomienda 8 o más (estabilidad de la curva, p. 408).
    let result = design(&input("1500 m3/h", "15 m", "1450 rpm")).unwrap();
    assert!(result.selection.blade_count > 7);
    assert!(has(&result, "HM-DES-034"));
}

#[test]
fn manual_blade_count_above_seven_does_not_blame_the_engine() {
    // Con Z fijado a mano, la sugerencia «con 5..7 álabes β2 se sale de
    // 15..45°» no es verdad ni útil: el motor no eligió ese Z.
    let result = design_with_options(
        &input("120 m3/h", "45 m", "2900 rpm"),
        &DesignOptions {
            blade_count: Some(9),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let note = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-DES-034")
        .expect("Z = 9 avisa");
    let hint = note.hint.clone().unwrap_or_default();
    assert!(!hint.contains("se sale de 15..45"), "{hint}");
    assert!(hint.contains("fijado"), "{hint}");
}

#[test]
fn double_volute_options_must_agree() {
    // Una voluta doble tiene dos lenguas y un ángulo de envolvente: uno sin
    // el otro es una combinación a medias (HM-DES-038).
    let base = input("120 m3/h", "45 m", "2900 rpm");
    let with = |wrap: Option<f64>, channels: Option<u32>| {
        design_with_options(
            &base,
            &DesignOptions {
                volute_wrap_deg: wrap,
                volute_channels: channels,
                ..DesignOptions::default()
            },
        )
        .unwrap()
    };
    assert!(has(&with(Some(165.0), None), "HM-DES-038"));
    assert!(has(&with(None, Some(2)), "HM-DES-038"));
    assert!(!has(&with(Some(165.0), Some(2)), "HM-DES-038"));
    assert!(!has(&with(None, None), "HM-DES-038"));
}

#[test]
fn even_manual_blade_count_with_two_tongues_is_reported() {
    // Gülich p. 491: con zLe par se evita un Z par. El motor lo respeta al
    // elegir Z; con Z manual solo puede avisar (HM-DES-037).
    let base = input("120 m3/h", "45 m", "2900 rpm");
    let with = |z: u32| {
        design_with_options(
            &base,
            &DesignOptions {
                blade_count: Some(z),
                volute_channels: Some(2),
                ..DesignOptions::default()
            },
        )
        .unwrap()
    };
    assert!(has(&with(6), "HM-DES-037"));
    assert!(!has(&with(7), "HM-DES-037"));
}

#[test]
fn two_volute_channels_get_an_odd_blade_count() {
    // Con zLe = 2 (voluta doble) hay que evitar un Z par (Gülich p. 491).
    for (q, h, n) in [
        ("500 m3/h", "60 m", "1450 rpm"),
        ("120 m3/h", "45 m", "2900 rpm"),
        ("800 m3/h", "20 m", "1450 rpm"),
        // Con zLe = 1 sale Z = 6; con zLe = 2 tiene que ser impar.
        ("80 m3/h", "160 m", "2900 rpm"),
    ] {
        let result = design_with_options(
            &input(q, h, n),
            &DesignOptions {
                volute_channels: Some(2),
                ..DesignOptions::default()
            },
        )
        .unwrap();
        assert_eq!(result.selection.blade_count % 2, 1, "{q}");
    }
}

#[test]
fn zero_volute_channels_are_rejected() {
    let error = design_with_options(
        &input("120 m3/h", "45 m", "2900 rpm"),
        &DesignOptions {
            volute_channels: Some(0),
            ..DesignOptions::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("volute_channels"), "{error}");
}

#[test]
fn inlet_throat_ratio_is_the_throat_over_the_mean_relative_velocity() {
    // w1q = QLa/A1q = 3,5 m/s; w1m = √(3,5² + 30²) = 30,2035 m/s.
    close(inlet_throat_ratio(0.07, 0.02, 3.5, 30.0), 0.115_88, 1e-5);
}

#[test]
fn inlet_deceleration_agrees_with_the_throat_flow() {
    // Gülich 6.18: QW es el caudal con w1 = w1q. A caudal de diseño
    // c1m = cm0 y w1q = QLa/A1q, así que w1q/w1m se deduce de QW:
    // x = Qth/QW,La; W = √((QW,La/A1)² + u1²); w1q/w1m = x·W/√((x·QW,La/A1)² + u1²).
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("120 m3/h", "45 m", "2900 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
    ] {
        let result = design(&input(q, h, n)).unwrap();
        assert!(
            (0.5..1.2).contains(&result.inlet_deceleration),
            "{q}: {}",
            result.inlet_deceleration
        );
        let a1 = std::f64::consts::PI * result.d1.si * result.b1.si;
        let qw_la = result.throat_flow.si / result.eta_v;
        let u1 = result.u1.si;
        let x = result.qth.si / qw_la;
        let w = ((qw_la / a1).powi(2) + u1 * u1).sqrt();
        let expected = x * w / ((x * qw_la / a1).powi(2) + u1 * u1).sqrt();
        close(result.inlet_deceleration, expected, 1e-9);
    }
}

#[test]
fn inlet_deceleration_warning_follows_the_band() {
    // Gülich p. 415: w1q/w1m de 0,75 a 0,85 en el BEP con entrada axial.
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("120 m3/h", "45 m", "2900 rpm"),
        ("500 m3/h", "60 m", "1450 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
    ] {
        let result = design(&input(q, h, n)).unwrap();
        let inside = (0.75..=0.85).contains(&result.inlet_deceleration);
        assert_eq!(has(&result, "HM-DES-035"), !inside, "{q}");
    }
}

#[test]
fn incidence_option_moves_the_inlet_angle_and_the_throat_ratio() {
    // Gülich p. 411: i1 de 0 a 4°. B1 (120 m³/h, 45 m, 2900 rpm): con 4°,
    // w1q/w1m ≈ 0,84 (en la banda de 0,75..0,85) y QSF sube; con 2° (por
    // defecto) queda en 0,92 y HM-DES-035 informa.
    let base = input("120 m3/h", "45 m", "2900 rpm");
    let with = |incidence: Option<f64>| {
        design_with_options(
            &base,
            &DesignOptions {
                incidence_deg: incidence,
                ..DesignOptions::default()
            },
        )
        .unwrap()
    };
    let default = with(None);
    let two = with(Some(2.0));
    let four = with(Some(4.0));
    close(default.inlet_deceleration, two.inlet_deceleration, 1e-12);
    close(two.inlet_deceleration, 0.923, 0.01);
    close(four.inlet_deceleration, 0.84, 0.01);
    assert!(has(&two, "HM-DES-035"));
    // El aviso dice de qué depende y cómo se cambia.
    let note = two
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "HM-DES-035")
        .unwrap();
    let text = format!("{} {}", note.message, note.hint.clone().unwrap_or_default());
    assert!(
        text.contains("incidence_deg") && text.contains("2D"),
        "{text}"
    );
    assert!(!has(&four, "HM-DES-035"));
    assert!(four.shockless_flow.si > two.shockless_flow.si);
}

#[test]
fn incidence_option_rejects_values_outside_the_range() {
    let base = input("120 m3/h", "45 m", "2900 rpm");
    for bad in [-1.0, 12.0, f64::NAN] {
        let error = design_with_options(
            &base,
            &DesignOptions {
                incidence_deg: Some(bad),
                ..DesignOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("incidence_deg"), "{bad}: {error}");
    }
}

#[test]
fn allowable_blade_loading_follows_nq() {
    // Gülich 7.1.11: ξal = (40/nq)^0,77.
    close(blade_loading_allowable(40.0), 1.0, 1e-12);
    close(blade_loading_allowable(16.46), 1.981_23, 1e-5);
}

#[test]
fn effective_blade_loading_follows_the_impeller() {
    // Gülich 7.1.10: ξeff = 2π·ψ/(ηh·zLa·L*·(w1* + w2*)) con L* = L/d2 y
    // w* = w/u2; L es la línea media del álabe. Con ξeff > 1,15·ξal avisa
    // HM-DES-036.
    for (q, h, n) in [
        ("1060 gpm", "83 m", "1750 rpm"),
        ("120 m3/h", "45 m", "2900 rpm"),
        ("500 m3/h", "60 m", "1450 rpm"),
        ("200 m3/h", "8 m", "1750 rpm"),
    ] {
        let result = design(&input(q, h, n)).unwrap();
        let blade = camber_line(
            (0.5 * result.d1.si, 0.5 * result.d2.si),
            (result.beta1b, result.selection.beta2_deg.to_radians()),
            (result.selection.cm0, result.cm2.si),
            result.qth.si,
            BLADE_STEPS,
        );
        let length = blade_length(&blade) / result.d2.si;
        let velocities = (result.w1.si + result.w2.si) / result.u2.si;
        let expected = 2.0 * std::f64::consts::PI * result.selection.psi
            / (result.eta_h * f64::from(result.selection.blade_count) * length * velocities);
        assert!(
            result.blade_loading.is_finite() && result.blade_loading > 0.0,
            "{q}"
        );
        close(result.blade_loading, expected, 1e-9 * expected);
        // Con nq > 40 la línea media 2D es mucho más corta que el álabe 3D
        // (HM-DES-024) y ξeff sale 2..3 veces el admisible sin que el diseño
        // lo esté: el aviso se limita a nq ≤ 40.
        let high = result.blade_loading > 1.15 * blade_loading_allowable(result.nq)
            && result.nq <= CYLINDRICAL_BLADE_MAX_NQ;
        assert_eq!(has(&result, "HM-DES-036"), high, "{q}");
    }
}

#[test]
fn b1_blade_loading_is_inside_the_band() {
    // B1 (nq 16,5, 5 álabes) queda dentro de ξal ± 15 %: sin aviso.
    let result = design(&input("1060 gpm", "83 m", "1750 rpm")).unwrap();
    let allowable = blade_loading_allowable(result.nq);
    let ratio = result.blade_loading / allowable;
    assert!((0.85..=1.15).contains(&ratio), "ξeff/ξal = {ratio}");
    assert!(!has(&result, "HM-DES-036"));
}

/// Fuga del anillo delantero con un fluido de viscosidad `nu` (m²/s) en la duty
/// de 120 m³/h, 45 m y 2900 rpm, con las selecciones del diseño con agua.
fn seal_leakage_with_viscosity(nu: f64) -> f64 {
    let water = input("120 m3/h", "45 m", "2900 rpm");
    let sized = design(&water).unwrap();
    let mut viscous = water;
    viscous.nu = Some(nu);
    design_with_options(
        &viscous,
        &DesignOptions {
            psi: Some(sized.selection.psi),
            phi: Some(sized.selection.phi),
            beta2_rad: Some(sized.selection.beta2_deg.to_radians()),
            blade_count: Some(sized.selection.blade_count),
            c0: Some(sized.selection.c0),
            cm0: Some(sized.selection.cm0),
            j: Some(sized.selection.j),
            ..DesignOptions::default()
        },
    )
    .unwrap()
    .seal_leakage_ratio
}

#[test]
fn seal_leakage_never_rises_with_viscosity_through_the_transition() {
    // Gülich 3.7.11: con Re < 2000, λ0 = 96/Re; la regla máx(λlam; λtur) es solo
    // para la transición. Con ella aplicada en todo el régimen laminar, la
    // correlación turbulenta (polo en Re ≈ 6) mandaba entre ≈ 40 y 75 cSt y la
    // fuga subía al aumentar ν (+35 % de 50 a 75 cSt).
    let mut previous = f64::INFINITY;
    for cst in [
        5.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 75.0, 100.0, 200.0, 500.0,
    ] {
        let leak = seal_leakage_with_viscosity(cst * 1.0e-6);
        assert!(
            leak <= previous * (1.0 + 1e-9),
            "{cst} cSt: la fuga sube de {previous} a {leak}"
        );
        previous = leak;
    }
}

#[track_caller]
fn assert_input_error(input: &DesignInput, name: &str, message: &str) {
    let error = input.validate().unwrap_err();
    assert_eq!(error.name, name);
    assert_eq!(error.message, message);
}

#[track_caller]
fn assert_option_error(options: DesignOptions, name: &str, message: &str) {
    let error = options.validate().unwrap_err();
    assert_eq!(error.name, name);
    assert_eq!(error.message, message);
}

#[track_caller]
fn assert_design_error(options: &DesignOptions, message: &str) {
    let error = design_with_options(&input("120 m3/h", "45 m", "2900 rpm"), options)
        .unwrap_err()
        .to_string();
    assert_eq!(error, format!("entrada inválida: {message}"));
}

#[test]
fn each_invalid_input_names_its_field() {
    assert_input_error(
        &input("120 m", "45 m", "2900 rpm"),
        "Q",
        "Q debe tener dimensiones de m3/s",
    );
    assert_input_error(
        &input("120 m3/h", "45 s", "2900 rpm"),
        "H",
        "H debe tener dimensiones de m",
    );
    assert_input_error(
        &input("120 m3/h", "45 m", "2900 m"),
        "n",
        "n debe tener dimensiones de 1/s",
    );

    let mut rho = input("120 m3/h", "45 m", "2900 rpm");
    rho.rho = parse_quantity("998.2 m").unwrap();
    assert_input_error(&rho, "rho", "rho debe tener dimensiones de kg/m3");

    let mut npsha = input("120 m3/h", "45 m", "2900 rpm");
    npsha.npsha = Some(parse_quantity("10 s").unwrap());
    assert_input_error(&npsha, "NPSHa", "NPSHa debe tener dimensiones de m");

    let mut eta_m = input("120 m3/h", "45 m", "2900 rpm");
    eta_m.eta_m = 0.0;
    assert_input_error(
        &eta_m,
        "eta_m",
        "la eficiencia mecánica debe ser mayor que 0 y menor que 1",
    );

    assert_input_error(
        &input("0 m3/s", "45 m", "2900 rpm"),
        "Q",
        "Q debe ser finito y positivo",
    );
}

#[test]
fn each_invalid_option_names_its_parameter() {
    assert_option_error(
        DesignOptions {
            eta_v: Some(1.2),
            ..DesignOptions::default()
        },
        "eta_v_fixed",
        "`eta_v` (parámetro `eta_v_fixed`, opción `--eta-v`) = 1.2 no es válido; debe estar entre 0 y 1 sin incluirlos",
    );

    assert_option_error(
        DesignOptions {
            volute_wrap_deg: Some(45.0),
            ..DesignOptions::default()
        },
        "volute_wrap_deg",
        "`volute_wrap_deg` = 45 no es válido; debe estar entre 90 y 180 grados",
    );

    assert_option_error(
        DesignOptions {
            incidence_deg: Some(12.0),
            ..DesignOptions::default()
        },
        "incidence_deg",
        "`incidence_deg` = 12 no es válido; debe estar entre 0 y 10 grados",
    );

    assert_option_error(
        DesignOptions {
            volute_channels: Some(0),
            ..DesignOptions::default()
        },
        "volute_channels",
        "`volute_channels` = 0 no es válido; debe ser al menos 1",
    );

    assert_option_error(
        DesignOptions {
            volute_ratio: Some(1.2),
            ..DesignOptions::default()
        },
        "volute_ratio",
        "`volute_ratio` = 1.2 no es válido; debe estar entre 0.5 y 1",
    );
}

#[test]
fn design_with_options_keeps_the_same_error_text() {
    assert_design_error(
        &DesignOptions {
            eta_v: Some(1.2),
            ..DesignOptions::default()
        },
        "`eta_v` (parámetro `eta_v_fixed`, opción `--eta-v`) = 1.2 no es válido; debe estar entre 0 y 1 sin incluirlos",
    );
    assert_design_error(
        &DesignOptions {
            volute_wrap_deg: Some(45.0),
            ..DesignOptions::default()
        },
        "`volute_wrap_deg` = 45 no es válido; debe estar entre 90 y 180 grados",
    );
    assert_design_error(
        &DesignOptions {
            incidence_deg: Some(12.0),
            ..DesignOptions::default()
        },
        "`incidence_deg` = 12 no es válido; debe estar entre 0 y 10 grados",
    );
    assert_design_error(
        &DesignOptions {
            volute_channels: Some(0),
            ..DesignOptions::default()
        },
        "`volute_channels` = 0 no es válido; debe ser al menos 1",
    );
    assert_design_error(
        &DesignOptions {
            volute_ratio: Some(1.2),
            ..DesignOptions::default()
        },
        "`volute_ratio` = 1.2 no es válido; debe estar entre 0.5 y 1",
    );

    let error = design_with_options(
        &input("0 m3/s", "45 m", "2900 rpm"),
        &DesignOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "entrada inválida: Q debe ser finito y positivo");
}

#[test]
fn validate_duty_checks_only_the_duty() {
    let case = input("120 m3/h", "45 m", "2900 rpm");
    assert!(validate_duty(&case.q, &case.h, &case.n).is_ok());

    let mut no_eta = case.clone();
    no_eta.eta_m = 0.0;
    assert!(validate_duty(&no_eta.q, &no_eta.h, &no_eta.n).is_ok());

    let zero = input("0 m3/s", "45 m", "2900 rpm");
    assert_eq!(
        validate_duty(&zero.q, &zero.h, &zero.n).unwrap_err().name,
        "Q"
    );
}

#[test]
fn options_are_checked_in_a_fixed_order() {
    // Con varias opciones inválidas a la vez, manda la primera de esta lista.
    let mut options = DesignOptions {
        eta_v: Some(1.2),
        volute_wrap_deg: Some(45.0),
        incidence_deg: Some(12.0),
        volute_channels: Some(0),
        volute_ratio: Some(1.2),
        ..DesignOptions::default()
    };
    for expected in [
        "eta_v_fixed",
        "volute_wrap_deg",
        "incidence_deg",
        "volute_channels",
        "volute_ratio",
    ] {
        assert_eq!(options.validate().unwrap_err().name, expected);
        match expected {
            "eta_v_fixed" => options.eta_v = None,
            "volute_wrap_deg" => options.volute_wrap_deg = None,
            "incidence_deg" => options.incidence_deg = None,
            "volute_channels" => options.volute_channels = None,
            _ => options.volute_ratio = None,
        }
    }
    assert!(options.validate().is_ok());
}

#[test]
fn an_invalid_input_is_reported_before_an_invalid_option() {
    let options = DesignOptions {
        volute_wrap_deg: Some(45.0),
        ..DesignOptions::default()
    };
    let error = design_with_options(&input("0 m3/s", "45 m", "2900 rpm"), &options)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "entrada inválida: Q debe ser finito y positivo");
}

#[test]
fn an_invalid_option_is_reported_before_an_invalid_selection() {
    // `psi = 0` lo rechaza validate_selection, que corre después de las opciones.
    let options = DesignOptions {
        psi: Some(0.0),
        volute_wrap_deg: Some(45.0),
        ..DesignOptions::default()
    };
    assert_design_error(
        &options,
        "`volute_wrap_deg` = 45 no es válido; debe estar entre 90 y 180 grados",
    );

    let only_selection = DesignOptions {
        psi: Some(0.0),
        ..DesignOptions::default()
    };
    assert_design_error(
        &only_selection,
        "`psi` = 0 no es válido; debe ser mayor que 0",
    );
}

#[test]
fn unreachable_head_does_not_suggest_more_blades() {
    // cu2/u2 = 1,278: ningún Z la da.
    let error = design(&input("10 m3/h", "100 m", "2900 rpm"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("reduce ψ"), "{error}");
    assert!(!error.contains("aumenta Z"), "{error}");
}

#[test]
fn reachable_head_still_suggests_more_blades() {
    // cu2/u2 = 0,817, por debajo de 1: con más álabes puede alcanzarse.
    let error = design(&input("30 m3/h", "100 m", "2900 rpm"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("aumenta Z"), "{error}");
}
