use hm_engine::{
    CaseField, DensitySource, Entered, FluidKind, Intake, MANUAL_PARAMS, PARAM_SPECS,
    case_from_project,
};
use hm_project::{Param, Project, PumpType, Suction};

fn project() -> Project {
    Project::new("demo", PumpType::EndSuction)
}

fn manual(value: &str) -> Param {
    Param::Manual {
        value: value.to_string(),
    }
}

fn set(project: &mut Project, name: &str, value: &str) {
    project.params.insert(name.to_string(), manual(value));
}

#[test]
fn case_from_project_reads_every_manual_option() {
    let mut project = project();
    for (name, value) in [
        ("psi", "1.1"),
        ("phi", "0.06"),
        ("beta2", "20 deg"),
        ("blade_count", "7"),
        ("c0", "3.5 m/s"),
        ("cm0", "3.2 m/s"),
        ("A", "4.5"),
        ("J", "1.02"),
        ("margin", "1.15"),
        ("k3", "1.08"),
        ("volute_c", "0.06"),
        ("k_eff", "1.12"),
        ("eta_v_fixed", "0.95"),
        ("balance_holes", "1"),
        ("volute_ratio", "0.7"),
        ("impeller_roughness", "0.00005 m"),
        ("volute_channels", "2"),
        ("volute_wrap_deg", "150 deg"),
        ("incidence_deg", "3 deg"),
    ] {
        set(&mut project, name, value);
    }

    let case = case_from_project(&project).unwrap();
    for (name, expected) in [
        ("psi", 1.1),
        ("phi", 0.06),
        ("beta2", 20.0),
        ("blade_count", 7.0),
        ("c0", 3.5),
        ("cm0", 3.2),
        ("A", 4.5),
        ("J", 1.02),
        ("margin", 1.15),
        ("k3", 1.08),
        ("volute_c", 0.06),
        ("k_eff", 1.12),
        ("eta_v_fixed", 0.95),
        ("balance_holes", 1.0),
        ("volute_ratio", 0.7),
        ("impeller_roughness", 0.00005),
        ("volute_channels", 2.0),
        ("volute_wrap_deg", 150.0),
        ("incidence_deg", 3.0),
    ] {
        let param = MANUAL_PARAMS
            .iter()
            .find(|param| param.name == name)
            .unwrap_or_else(|| panic!("falta {name} en MANUAL_PARAMS"));
        let actual =
            (param.get)(&case.design).unwrap_or_else(|| panic!("{name} no se leyó del proyecto"));
        assert!(
            (actual - expected).abs() < 1e-12,
            "{name}: {actual} frente a {expected}"
        );
    }
}

#[test]
fn zero_means_no_data_for_every_option_and_balance_holes_is_strict() {
    let mut project = project();
    for param in MANUAL_PARAMS.iter().filter(|param| param.zero_is_none()) {
        let value = if param.unit == "1" {
            "0".to_string()
        } else {
            format!("0 {}", param.unit)
        };
        set(&mut project, param.name, &value);
    }

    let case = case_from_project(&project).unwrap();
    for param in MANUAL_PARAMS.iter().filter(|param| param.zero_is_none()) {
        assert_eq!(
            (param.get)(&case.design),
            None,
            "{} debería quedar sin dato",
            param.name
        );
    }

    let mut holes = project.clone();
    set(&mut holes, "balance_holes", "2");
    let error = case_from_project(&holes).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Param("balance_holes")));

    let mut blades = project;
    set(&mut blades, "blade_count", "0");
    assert!(case_from_project(&blades).is_err());
}

#[test]
fn override_is_rejected_with_its_code() {
    let mut project = project();
    project.params.insert(
        "psi".to_string(),
        Param::Override {
            expr: "0.5".to_string(),
            author: "test".to_string(),
            date: "2026-11-03".to_string(),
            reason: "golden".to_string(),
        },
    );

    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Param("psi")));
    assert_eq!(error.code(), Some("HM-PROJ-019"));
}

#[test]
fn entered_keeps_the_text_the_user_wrote() {
    let entered = Entered::parse("1060 gpm").unwrap();

    assert_eq!(entered.text, "1060 gpm");
    assert!((entered.quantity.value() - 1060.0).abs() < 1e-12);
}

#[test]
fn entered_quantity_comes_from_its_text() {
    let entered = Entered::parse("1000 mm").unwrap();

    assert!((entered.quantity.si - 1.0).abs() < 1e-12);
}

#[test]
fn water_density_follows_temperature_when_rho_is_missing() {
    let mut project = project();
    project.operating.temperature = "80 degC".to_string();

    let case = case_from_project(&project).unwrap();
    assert!(matches!(
        case.fluid.density.source,
        DensitySource::WaterAtTemperature
    ));
    assert!((case.fluid.density.value.value() - 971.8).abs() < 0.1);
}

#[test]
fn explicit_density_is_kept_and_marked_explicit() {
    let mut project = project();
    set(&mut project, "rho", "1000 kg/m3");

    let case = case_from_project(&project).unwrap();
    match &case.fluid.density.source {
        DensitySource::Explicit { text } => assert_eq!(text, "1000 kg/m3"),
        other => panic!("se esperaba densidad explícita, llegó {other:?}"),
    }
    assert!((case.fluid.density.value.value() - 1000.0).abs() < 1e-12);
}

#[test]
fn installation_keeps_empty_fields_distinct_from_zero() {
    let mut project = project();
    project.suction = Some(Suction {
        pressure: "1 atm".to_string(),
        height: String::new(),
        losses: String::new(),
        vapor_pressure: String::new(),
    });

    let case = case_from_project(&project).unwrap();
    match case.intake {
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => {
            assert_eq!(pressure.text, "1 atm");
            assert!(height.is_none());
            assert!(losses.is_none());
            assert!(vapor_pressure.is_none());
        }
        other => panic!("se esperaba instalación, llegó {other:?}"),
    }
}

#[test]
fn legacy_method_is_not_rejected_by_case_from_project() {
    let mut project = project();
    project.design.method = "end_suction_v0.8".to_string();

    assert!(case_from_project(&project).is_ok());
}

#[test]
fn non_water_fluid_without_rho_falls_back_to_the_default_density() {
    let mut project = project();
    project.operating.fluid = "oil".to_string();

    let case = case_from_project(&project).unwrap();
    assert_eq!(case.fluid.kind, FluidKind::Custom("oil".to_string()));
    assert!(matches!(case.fluid.density.source, DensitySource::Fallback));
    assert!((case.fluid.density.value.value() - 998.2).abs() < 1e-12);
}

#[test]
fn manual_params_match_param_specs() {
    for param in MANUAL_PARAMS {
        let spec = PARAM_SPECS
            .iter()
            .find(|spec| spec.name == param.name)
            .unwrap_or_else(|| panic!("{} no está en PARAM_SPECS", param.name));
        assert_eq!(spec.unit, param.unit, "{}", param.name);
    }

    let exceptions = ["rho", "eta_m", "mm"];
    for spec in PARAM_SPECS {
        if spec.formula.is_none() && !exceptions.contains(&spec.name) {
            assert!(
                MANUAL_PARAMS.iter().any(|param| param.name == spec.name),
                "{} es manual y no está en MANUAL_PARAMS",
                spec.name
            );
        }
    }

    let incidence = MANUAL_PARAMS
        .iter()
        .find(|param| param.name == "incidence_deg")
        .unwrap();
    assert!(!incidence.zero_is_none());
}

#[test]
fn available_npsha_keeps_its_text() {
    let mut project = project();
    project.operating.npsha = "10 m".to_string();

    let case = case_from_project(&project).unwrap();
    match case.intake {
        Intake::Available(entered) => {
            assert_eq!(entered.text, "10 m");
            assert!((entered.quantity.si - 10.0).abs() < 1e-12);
        }
        other => panic!("se esperaba NPSHa disponible, llegó {other:?}"),
    }
}

#[test]
fn an_angle_without_a_unit_is_rejected_instead_of_read_as_radians() {
    for name in ["beta2", "volute_wrap_deg", "incidence_deg"] {
        let mut project = project();
        set(&mut project, name, "20");

        let error = case_from_project(&project).unwrap_err();
        assert_eq!(
            error.field(),
            Some(CaseField::Param(match name {
                "beta2" => "beta2",
                "volute_wrap_deg" => "volute_wrap_deg",
                _ => "incidence_deg",
            }))
        );
        assert_eq!(error.code(), Some("HM-PROJ-020"), "{name}");
    }
}

#[test]
fn an_angle_in_radians_or_degrees_is_accepted() {
    let mut project = project();
    set(&mut project, "beta2", "0.3490658503988659 rad");
    set(&mut project, "incidence_deg", "3 deg");

    let case = case_from_project(&project).unwrap();
    assert!((case.design.beta2_rad.unwrap().to_degrees() - 20.0).abs() < 1e-9);
    assert!((case.design.incidence_deg.unwrap() - 3.0).abs() < 1e-9);
}

#[test]
fn water_without_a_temperature_says_it_used_the_default() {
    let mut project = project();
    project.operating.temperature = String::new();

    let case = case_from_project(&project).unwrap();

    assert!(case.fluid.temperature.is_none());
    assert!(matches!(
        case.fluid.density.source,
        DensitySource::WaterAtDefaultTemperature
    ));
    assert!((case.fluid.density.value.value() - 998.2).abs() < 0.1);
}

#[test]
fn water_outside_the_density_correlation_falls_back_and_says_so() {
    let mut project = project();
    project.operating.temperature = "200 degC".to_string();

    let case = case_from_project(&project).unwrap();
    assert!(matches!(case.fluid.density.source, DensitySource::Fallback));
    assert!((case.fluid.density.value.value() - 998.2).abs() < 1e-12);
    assert!(case.fluid.temperature.is_some());
}

#[test]
fn npsha_in_operating_and_in_suction_is_an_error() {
    let mut project = project();
    project.operating.npsha = "6 m".to_string();
    project.suction = Some(Suction {
        pressure: "1 atm".to_string(),
        height: String::new(),
        losses: String::new(),
        vapor_pressure: String::new(),
    });

    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Npsha));
    assert_eq!(error.code(), Some("HM-PROJ-028"));
}

#[test]
fn a_bad_suction_value_names_which_one() {
    for (field, expected) in [
        ("pressure", "pressure"),
        ("height", "height"),
        ("losses", "losses"),
        ("vapor_pressure", "vapor_pressure"),
    ] {
        let mut suction = Suction {
            pressure: "1 atm".to_string(),
            height: String::new(),
            losses: String::new(),
            vapor_pressure: String::new(),
        };
        match field {
            "pressure" => suction.pressure = "abc".to_string(),
            "height" => suction.height = "abc".to_string(),
            "losses" => suction.losses = "abc".to_string(),
            _ => suction.vapor_pressure = "abc".to_string(),
        }
        let mut project = project();
        project.suction = Some(suction);

        let error = case_from_project(&project).unwrap_err();
        assert_eq!(error.field(), Some(CaseField::Suction));
        assert_eq!(error.code(), Some("HM-PROJ-027"));
        assert!(error.to_string().contains(expected), "{field}: {error}");
    }
}

#[test]
fn angles_in_degrees_keep_the_number_that_was_written() {
    let mut project = project();
    set(&mut project, "volute_wrap_deg", "120.975462 deg");
    set(&mut project, "beta2", "20.3 deg");

    let case = case_from_project(&project).unwrap();
    // Sin viaje a radianes y de vuelta: igual bit a bit.
    assert_eq!(case.design.volute_wrap_deg, Some(120.975462));
    assert_eq!(
        case.design.beta2_rad,
        Some(hm_units::parse_quantity("20.3 deg").unwrap().si)
    );
}

#[test]
fn balance_holes_must_be_zero_or_one_even_when_written_as_text() {
    let mut project = project();
    set(&mut project, "balance_holes", "true");

    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Param("balance_holes")));
    assert_eq!(error.code(), Some("HM-PROJ-020"));
    assert!(error.to_string().contains("0 o 1"), "{error}");
}

#[test]
fn counts_outside_one_to_one_hundred_are_rejected() {
    for value in ["101", "2.5", "-1"] {
        let mut project = project();
        set(&mut project, "volute_channels", value);

        let error = case_from_project(&project).unwrap_err();
        assert_eq!(
            error.field(),
            Some(CaseField::Param("volute_channels")),
            "{value}"
        );
    }
}

#[test]
fn a_non_positive_viscosity_is_a_case_error_with_its_code() {
    for text in ["-1 cSt", "0 cSt", "0 cP", "-5 cP"] {
        let mut project = project();
        project.operating.viscosity = text.to_string();

        let error = case_from_project(&project).unwrap_err();
        assert_eq!(error.field(), Some(CaseField::Viscosity), "{text}");
        assert_eq!(error.code(), Some("HM-PROJ-026"), "{text}");
        assert!(
            error.diagnostic().unwrap().message.contains("positiva"),
            "{text}"
        );
    }
}
