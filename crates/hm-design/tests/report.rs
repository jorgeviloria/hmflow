use hm_design::mechanics::{DEFAULT_SHAFT_SHEAR_STRESS, mechanics};
use hm_design::{DesignInput, DesignReport, PARAM_SPECS, REPORT_FORMAT, REPORT_VERSION, design};
use hm_units::{parse_quantity, parse_unit};

fn input() -> DesignInput {
    DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("15 m").unwrap()),
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
    ]);
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
    assert_eq!(report.volute_sections.len(), result.volute.sections.len());
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
    let outlet = &geometry.volute_outlet;
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
