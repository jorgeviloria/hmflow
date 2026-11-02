use hm_design::{
    DesignInput, DesignOptions, PARAM_SPECS, default_mechanical_efficiency, design,
    design_with_options, designed_si, gulich_b2_star, param_spec, required_npsha,
    stored_si_matches, water_density,
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
    }
}

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
    // Desde end_suction_v0.4 η sale del análisis de pérdidas de Gülich; el
    // libro da 0,813 con su cadena ηh·ηv·ηm. La diferencia (−0,4 % en
    // potencia y par) es la del modelo, no la de la cadena de fórmulas.
    close(result.eta, 0.812, 0.005);
    close(result.p_hyd.value(), 2161.4, 5.0);
    close(result.p_shaft.value(), 2661.8, 15.0);
    close(result.p_max.value(), 2928.0, 20.0);
    close(result.torque.value(), 14.12, 0.08);
    close(result.torque_max.value(), 15.53, 0.1);
    // v0.6: Gülich ec. 6.10 con c0 y u1 del ojo; σ·H daba 1,67 m. El libro
    // no publica el NPSH de D1.A1.
    close(result.npshr_design.value(), 1.42, 0.05);
    // Gülich (3.28) da 18,0 m; la fig. D1.A2 de Srinivasan, ≈19 m (−5 %).
    close(result.shutoff_head.value(), 18.03, 0.05);
    // Con β2 = 23,5° y c2u de Gülich (3.2.7), Euler da +4,5 % de altura.
    assert_eq!(codes(&result), ["HM-DES-019", "HM-DES-012"]);
}

fn d1a1_selections() -> DesignOptions {
    DesignOptions {
        psi: Some(0.87),
        phi: Some(0.13),
        beta2_rad: Some(23.5f64.to_radians()),
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
    // v0.7: con el ηh de Gülich (0,890 frente a 0,870) Euler pide menos cu2.
    close(result.selection.beta2_deg, 18.85, 0.1);
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
    // v0.7: ηh de Gülich 0,904 (Srinivasan daba 0,875): β2 baja de 19,08°.
    close(result.selection.beta2_deg, 17.82, 0.02);
    close(result.d2.value(), 0.2000, 0.001);
    close(result.b2.value(), 0.01794, 0.0003);
    close(result.d1.value(), 0.1135, 0.001);
    close(result.b1.value(), 0.0428, 0.001);
    // end_suction_v0.4 (análisis de pérdidas): 0,843; v0.3 daba 0,816.
    // Gülich ec. 3.9.1 da 0,812 ± 0,038 para este Q y nq.
    close(result.eta, 0.843, 0.003);
    close(result.p_hyd.value(), 14681.5, 5.0);
    close(result.p_shaft.value(), 14681.5 / 0.8431616, 20.0);
    close(
        result.torque.value(),
        14681.5 / 0.8431616 / (2.0 * std::f64::consts::PI * 2900.0 / 60.0),
        0.1,
    );
    // v0.6: Gülich ec. 6.10 con el ojo; σ·H daba 5,14 m.
    close(result.npshr_design.value(), 3.89, 0.05);
    close(result.shutoff_head.value(), 53.65, 0.1);
    // H0/Hd = 1,19: por debajo del 1,2 de Gülich, informativo. Desde la v0.5
    // la pendiente de la tangente (§4.3.5) es mayor de lo que admite ese H0 y
    // la curva tiene un máximo a caudal parcial (HM-DES-031).
    assert_eq!(codes(&result), ["HM-DES-017", "HM-DES-031", "HM-DES-012"]);
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
    assert!(!stored_si_matches(result.volute.throat_area.si, 0.0018));
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
    // Con el NPSH3 de la v0.6 la región 0,7..1,2 Qd pide ≈ 8,6 m (tabla 6.2
    // de Gülich en 1,2 Qd); 8 m no llega.
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("8 m").unwrap());
    let result = design(&input).unwrap();
    assert!(has(&result, "HM-DES-007"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn npsh_shortfall_outside_the_preferred_region_only_warns() {
    // 10 m cubre la región 0,7..1,2 Qd (≈ 8,6 m) pero no 1,3 Qd (≈ 11 m).
    let mut input = input("120 m3/h", "45 m", "2900 rpm");
    input.npsha = Some(parse_quantity("10 m").unwrap());
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
    // u1 < 10 m/s: max(1,25·NPSH3; NPSH3 + 0,6 m).
    close(required_npsha(2.0, 7.4, 1.0), 2.6, 1e-12);
    close(required_npsha(4.0, 7.4, 1.0), 5.0, 1e-12);

    // 10..50 m/s: NPSH3 + FR·{0,05 + 2(u1/100)³}·u1²/2g, con el mínimo.
    let u1: f64 = 30.0;
    let term = (0.05 + 2.0 * 0.3f64.powi(3)) * u1.powi(2) / (2.0 * 9.80665);
    close(required_npsha(2.0, u1, 1.0), 2.0 + term, 1e-12);
    close(required_npsha(2.0, u1, 1.2), 2.0 + 1.2 * term, 1e-12);
    // Entre 1,0 y 1,1 Qd también 1,2: se supone QSF = Qd.
    close(required_npsha(2.0, u1, 1.05), 2.0 + 1.2 * term, 1e-12);
    close(required_npsha(2.0, u1, 0.9), 2.0 + term, 1e-12);
    // Con NPSH3 grande domina el mínimo de 1,25·NPSH3.
    close(required_npsha(20.0, u1, 1.0), 25.0, 1e-12);

    // Por encima de 50 m/s solo el mínimo.
    close(required_npsha(10.0, 60.0, 1.0), 12.5, 1e-12);
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

    // Un ojo muy grande (c0 = 1 m/s) sube u1 y con él w1.
    let slow = design_with_options(
        &input,
        &DesignOptions {
            c0: Some(1.0),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let ratio = slow.w2.si / slow.w1.si;
    assert!(ratio < 0.6, "w2/w1 = {ratio}");
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
    let mut input = input("2000 m3/h", "200 m", "3000 rpm");
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
    // la voluta manda (Qc < QSF) y la subida empieza muy cerca de Qd.
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let qd = input.q.si;
    close(result.shockless_flow.si / qd, 1.274, 0.005);
    close(result.volute_matching_flow.si / qd, 0.989, 0.005);
    // Sin espesor de álabe, QW = QSF.
    close(result.throat_flow.si, result.shockless_flow.si, 1e-12);
    close(
        result.transition_ratio,
        result.volute_matching_flow.si / qd,
        1e-12,
    );
    assert!(!has(&result, "HM-DES-025"));
    close(result.recirculation_onset, 0.513, 0.002);
    assert!(!has(&result, "HM-DES-026"));
}

#[test]
fn high_nq_transition_and_recirculation_are_reported() {
    // nq ≈ 98: Qc = 0,93·Qd y recirculación desde 0,63·Qd (±0,1).
    let input = input("954 m3/h", "15 m", "1450 rpm");
    let result = design(&input).unwrap();
    assert!(
        result.transition_ratio < 0.95,
        "{}",
        result.transition_ratio
    );
    assert!(has(&result, "HM-DES-025"));
    assert!(has(&result, "HM-DES-026"));
}

#[test]
fn loss_estimates_follow_gulich() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    // 1/(1 + 4,1·nq^−1,6) y 1 − 0,0045·(1/Q)^0,4·(1500/n)^0,3
    close(
        result.leakage_eta_v,
        1.0 / (1.0 + 4.1 * result.nq.powf(-1.6)),
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
