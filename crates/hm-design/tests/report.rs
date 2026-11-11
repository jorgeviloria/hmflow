use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics};
use hm_design::{
    DesignInput, DesignOptions, DesignReport, PARAM_SPECS, REPORT_FORMAT, REPORT_VERSION, design,
    design_with_options,
};
use hm_units::{parse_quantity, parse_unit};

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("15 m").unwrap()),
        nu: None,
    }
}

fn report() -> DesignReport {
    let input = input();
    let result = design(&input).unwrap();
    let diagnostics = result.diagnostics.clone().into_vec();
    let mech = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    DesignReport::new(&input, &result, 11, diagnostics).with_mechanics(&mech)
}

#[test]
fn identifies_format_version_and_method() {
    let report = report();
    assert_eq!(report.format, REPORT_FORMAT);
    assert_eq!(report.version, REPORT_VERSION);
    assert_eq!(report.method, hm_design::METHOD);
}

#[test]
fn every_value_is_coherent_si() {
    let report = report();
    let mut measures: Vec<(&str, &hm_design::Measure)> = Vec::new();
    for map in [&report.input, &report.selection, &report.results] {
        measures.extend(map.iter().map(|(name, measure)| (name.as_str(), measure)));
    }
    for section in &report.volute_sections {
        measures.extend([
            ("angle", &section.angle),
            ("area", &section.area),
            ("diameter", &section.diameter),
            ("outer_radius", &section.outer_radius),
        ]);
    }
    let hydraulics = report
        .hydraulics
        .as_ref()
        .expect("el informe lleva hidráulica");
    measures.extend([
        ("beta1b", &hydraulics.beta1b),
        ("shockless_flow", &hydraulics.shockless_flow),
        ("throat_flow", &hydraulics.throat_flow),
        ("volute_matching_flow", &hydraulics.volute_matching_flow),
        ("transition_ratio", &hydraulics.transition_ratio),
        ("recirculation_onset", &hydraulics.recirculation_onset),
        ("leakage_eta_v", &hydraulics.leakage_eta_v),
        (
            "mechanical_eta_estimate",
            &hydraulics.mechanical_eta_estimate,
        ),
        ("disk_friction_ratio", &hydraulics.disk_friction_ratio),
        ("statistical_eta_h", &hydraulics.statistical_eta_h),
        ("loss_eta", &hydraulics.loss_eta),
        ("zero_head_flow_ratio", &hydraulics.zero_head_flow_ratio),
        ("head_slope", &hydraulics.head_slope),
        ("seal_leakage_ratio", &hydraulics.seal_leakage_ratio),
        ("seal_eta_v", &hydraulics.seal_eta_v),
    ]);
    // v0.9: deceleración de entrada, carga del álabe y garganta por momento
    // cinético. Opcionales: un informe anterior no los lleva.
    for (name, measure) in [
        ("inlet_deceleration", &hydraulics.inlet_deceleration),
        ("blade_loading", &hydraulics.blade_loading),
        (
            "blade_loading_allowable",
            &hydraulics.blade_loading_allowable,
        ),
        (
            "momentum_throat_diameter",
            &hydraulics.momentum_throat_diameter,
        ),
    ] {
        measures.push((name, measure.as_ref().expect(name)));
    }
    let mech = report
        .mechanics
        .as_ref()
        .expect("el informe lleva mecánica");
    measures.extend([
        ("shaft_torque", &mech.shaft_torque),
        ("shaft_shear_stress", &mech.shaft_shear_stress),
        ("shaft_min_diameter", &mech.shaft_min_diameter),
        ("hub_diameter", &mech.hub_diameter),
        (
            "radial_coefficient_shutoff",
            &mech.radial_coefficient_shutoff,
        ),
        ("radial_coefficient_design", &mech.radial_coefficient_design),
        ("radial_width", &mech.radial_width),
        ("radial_thrust_shutoff", &mech.radial_thrust_shutoff),
        ("radial_thrust_design", &mech.radial_thrust_design),
        ("radial_thrust_overload", &mech.radial_thrust_overload),
        ("front_seal_diameter", &mech.front_seal_diameter),
        ("shaft_seal_diameter", &mech.shaft_seal_diameter),
        ("axial_thrust_low", &mech.axial_thrust_low),
        ("axial_thrust_high", &mech.axial_thrust_high),
        ("blade_thickness_ratio", &mech.blade_thickness_ratio),
        ("blade_thickness", &mech.blade_thickness),
        ("trailing_edge_thickness", &mech.trailing_edge_thickness),
        ("shroud_thickness", &mech.shroud_thickness),
        ("seal_clearance", &mech.seal_clearance),
        ("rated_power", &mech.rated_power),
        ("driver_margin", &mech.driver_margin),
        ("driver_power", &mech.driver_power),
        ("curve_max_power", &mech.curve_max_power),
    ]);
    measures.extend(
        mech.motor_rating
            .iter()
            .map(|measure| ("motor_rating", measure)),
    );
    measures.extend(
        mech.volute_wrap
            .iter()
            .map(|measure| ("volute_wrap", measure)),
    );
    for point in &report.curve {
        measures.extend([
            ("q", &point.q),
            ("h", &point.h),
            ("eta", &point.eta),
            ("p_shaft", &point.p_shaft),
            ("npshr", &point.npshr),
        ]);
    }
    for (name, measure) in measures {
        let unit = parse_unit(&measure.unit).unwrap();
        assert!(
            (unit.factor - 1.0).abs() < 1e-12 && unit.offset == 0.0,
            "`{name}` en {} no es SI",
            measure.unit
        );
        assert!(measure.value.is_finite(), "`{name}` no es finito");
    }
}

#[test]
fn results_use_the_hmf_parameter_names() {
    let input = input();
    let result = design(&input).unwrap();
    let report = DesignReport::new(&input, &result, 11, Vec::new());
    for spec in PARAM_SPECS.iter().filter(|spec| spec.formula.is_some()) {
        assert!(
            report.results.contains_key(spec.name),
            "falta `{}`",
            spec.name
        );
    }
    assert_eq!(report.results["d2"].value, result.d2.si);
    assert_eq!(report.results["d2"].unit, "m");
    assert_eq!(
        report.volute_sections.len(),
        result.model_volute.as_ref().unwrap().sections.len()
    );
    assert_eq!(report.curve.len(), 11);
}

#[test]
fn round_trips_through_json() {
    let report = report();
    assert!(!report.diagnostics.is_empty());
    let text = serde_json::to_string_pretty(&report).unwrap();
    let back: DesignReport = serde_json::from_str(&text).unwrap();
    assert_eq!(back, report);
}

#[test]
fn readers_ignore_fields_added_later() {
    let mut value = serde_json::to_value(report()).unwrap();
    value["campo_futuro"] = serde_json::json!({"value": 1.0, "unit": "m"});
    let back: DesignReport = serde_json::from_value(value).unwrap();
    assert_eq!(back.version, REPORT_VERSION);
}

#[test]
fn viscous_block_is_si_and_round_trips() {
    let input = input();
    let result = design(&input).unwrap();
    let correction = hm_design::viscous_correction(&input, &result, 100e-6).unwrap();
    let report = DesignReport::new(&input, &result, 5, Vec::new()).with_viscous(&correction);
    let viscous = report.viscous.as_ref().unwrap();
    for measure in [
        &viscous.nu,
        &viscous.reynolds_mod,
        &viscous.f_q,
        &viscous.f_h,
        &viscous.f_eta,
        &viscous.flow,
        &viscous.head,
        &viscous.efficiency,
        &viscous.shaft_power,
    ] {
        let unit = parse_unit(&measure.unit).unwrap();
        assert!(
            (unit.factor - 1.0).abs() < 1e-12 && unit.offset == 0.0,
            "{}",
            measure.unit
        );
    }
    let text = serde_json::to_string(&report).unwrap();
    let back: DesignReport = serde_json::from_str(&text).unwrap();
    assert_eq!(back, report);

    // Sin corrección, el campo no se escribe.
    let plain = serde_json::to_value(DesignReport::new(&input, &result, 5, Vec::new())).unwrap();
    assert!(plain.get("viscous").is_none());
}

#[test]
fn geometry_block_is_si_and_round_trips() {
    let input = input();
    let result = design(&input).unwrap();
    let shape = hm_design::geometry::geometry(&input, &result).unwrap();
    let report = DesignReport::new(&input, &result, 5, Vec::new()).with_geometry(&shape);
    let geometry = report.geometry.as_ref().unwrap();
    let outlet = geometry.volute_outlet.as_ref().unwrap();
    let mut measures = vec![
        &geometry.impeller.beta1b,
        &geometry.impeller.wrap_angle,
        &outlet.cutwater_radius,
        &outlet.cutwater_min_radius,
        &outlet.nominal_diameter,
        &outlet.area_ratio,
        &outlet.min_length,
        &outlet.half_angle,
    ];
    for point in &geometry.impeller.blade {
        measures.extend([&point.r, &point.beta, &point.theta, &point.width]);
    }
    for measure in measures {
        let unit = parse_unit(&measure.unit).unwrap();
        assert!(
            (unit.factor - 1.0).abs() < 1e-12 && unit.offset == 0.0,
            "{}",
            measure.unit
        );
        assert!(measure.value.is_finite());
    }
    assert_eq!(geometry.impeller.blade.len(), shape.impeller.blade.len());
    let text = serde_json::to_string(&report).unwrap();
    let back: DesignReport = serde_json::from_str(&text).unwrap();
    assert_eq!(back, report);
}

#[test]
fn report_says_which_volute_the_radial_thrust_assumes() {
    // Sin ángulo de envolvente el empuje radial es el de una voluta simple
    // (kR,opt = 0,08); con él, el de voluta doble (tabla 9.7 (1)). El informe
    // lo dice para que no haya que adivinarlo a partir de los coeficientes.
    let mech = report().mechanics.expect("el informe lleva mecánica");
    assert_eq!(mech.radial_volute.as_deref(), Some("simple"));
    assert!(mech.volute_wrap.is_none());

    let input = input();
    let result = design_with_options(
        &input,
        &DesignOptions {
            volute_wrap_deg: Some(165.0),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    let measured = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let report = DesignReport::new(&input, &result, 11, result.diagnostics.clone().into_vec())
        .with_mechanics(&measured);
    let mech = report.mechanics.expect("el informe lleva mecánica");
    assert_eq!(mech.radial_volute.as_deref(), Some("doble"));
    let wrap = mech
        .volute_wrap
        .expect("la voluta doble lleva su envolvente");
    assert_eq!(wrap.unit, "rad");
    assert!((wrap.value - 165.0_f64.to_radians()).abs() < 1e-9);
}

#[test]
fn blade_loading_is_left_out_above_the_cylindrical_blade_range() {
    // Con nq > 40 la línea media 2D sobrestima la carga (2–3×): el informe
    // no publica ξeff ni ξal ahí, en vez de publicar un número engañoso.
    let input = DesignInput {
        q: parse_quantity("954 m3/h").unwrap(),
        h: parse_quantity("15 m").unwrap(),
        n: parse_quantity("1450 rpm").unwrap(),
        ..input()
    };
    let result = design(&input).unwrap();
    assert!(result.nq > hm_design::CYLINDRICAL_BLADE_MAX_NQ);
    let report = DesignReport::new(&input, &result, 11, Vec::new());
    let hydraulics = report.hydraulics.expect("hidráulica");
    assert!(hydraulics.blade_loading.is_none());
    assert!(hydraulics.blade_loading_allowable.is_none());
    assert!(hydraulics.inlet_deceleration.is_some());
}

#[test]
fn the_report_adds_the_edited_edges_only_when_there_are() {
    let input = input();
    let plain = design(&input).unwrap();
    let mech = mechanics(&input, &plain, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let report = DesignReport::new(&input, &plain, 11, plain.diagnostics.clone().into_vec())
        .with_mechanics(&mech);
    let section = report.mechanics.as_ref().unwrap();
    assert!(section.edited_inlet_thickness.is_none());
    assert!(section.edited_exit_thickness.is_none());
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains("edited_inlet_thickness"), "{text}");
    assert!(!text.contains("edited_exit_thickness"), "{text}");

    let options = DesignOptions {
        blade_edge_thickness: Some((0.006, 0.003)),
        ..DesignOptions::default()
    };
    let edited = design_with_options(&input, &options).unwrap();
    let mech = mechanics(&input, &edited, DEFAULT_SHAFT_SHEAR_STRESS).unwrap();
    let report = DesignReport::new(&input, &edited, 11, edited.diagnostics.clone().into_vec())
        .with_mechanics(&mech);
    let section = report.mechanics.as_ref().unwrap();
    let inlet = section.edited_inlet_thickness.as_ref().unwrap();
    let exit = section.edited_exit_thickness.as_ref().unwrap();
    assert_eq!((inlet.value, inlet.unit.as_str()), (0.006, "m"));
    assert_eq!((exit.value, exit.unit.as_str()), (0.003, "m"));
    let text = serde_json::to_string(&report).unwrap();
    assert!(text.contains("edited_inlet_thickness"), "{text}");
}
