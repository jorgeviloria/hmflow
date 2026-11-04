use hm_design::{DesignInput, design};
use hm_engine::{
    CaseField, Density, DensitySource, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
    case_from_project, cli_message, fluid_diagnostics, fluid_kind, kinematic_viscosity,
    looks_like_water, npsh_fluid_diagnostics, npsha, viscosity_effect, viscosity_from_text,
    water_density_at,
};
use hm_project::{Project, PumpType, Suction};
use hm_units::parse_quantity;

fn codes(diagnostics: &hm_core::Diagnostics) -> Vec<&str> {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

fn temperature(text: &str) -> Temperature {
    hm_engine::parse_temperature(text).unwrap()
}

fn density(text: &str) -> Density {
    Density {
        value: parse_quantity(text).unwrap(),
        source: DensitySource::Explicit {
            text: text.to_string(),
        },
    }
}

fn fluid(
    kind: FluidKind,
    temperature_text: Option<&str>,
    rho: &str,
    viscosity: Option<Viscosity>,
) -> Fluid {
    Fluid {
        kind,
        temperature: temperature_text.map(temperature),
        density: density(rho),
        viscosity,
    }
}

fn water(temperature_text: Option<&str>, rho: &str) -> Fluid {
    fluid(FluidKind::Water, temperature_text, rho, None)
}

fn custom(name: &str, temperature_text: Option<&str>, rho: &str) -> Fluid {
    fluid(
        FluidKind::Custom(name.to_string()),
        temperature_text,
        rho,
        None,
    )
}

fn viscosity(text: &str) -> Viscosity {
    viscosity_from_text(text)
        .unwrap()
        .unwrap_or_else(|| panic!("sin viscosidad: {text}"))
}

fn installation(
    pressure: &str,
    height: Option<&str>,
    losses: Option<&str>,
    vapor: Option<&str>,
) -> Intake {
    let entered = |text: &str| Entered::parse(text).unwrap();
    Intake::Installation {
        pressure: entered(pressure),
        height: height.map(entered),
        losses: losses.map(entered),
        vapor_pressure: vapor.map(entered),
    }
}

#[test]
fn water_with_high_density_warns() {
    let diagnostics = fluid_diagnostics(&water(None, "1200 kg/m3"));
    assert_eq!(codes(&diagnostics), ["HM-DES-009"]);
    let diagnostic = diagnostics.iter().next().unwrap();
    assert_eq!(
        diagnostic.message,
        "rho = 1200.0 kg/m3 no es la densidad del agua a 20.0 °C (998.2 kg/m3)"
    );
}

#[test]
fn hot_water_with_cold_density_warns() {
    let diagnostics = fluid_diagnostics(&water(Some("80 degC"), "998.2 kg/m3"));
    assert_eq!(codes(&diagnostics), ["HM-DES-009"]);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "rho = 998.2 kg/m3 no es la densidad del agua a 80.0 °C (971.8 kg/m3)"
    );
}

#[test]
fn hot_water_with_its_density_is_quiet() {
    let diagnostics = fluid_diagnostics(&water(Some("80 degC"), "971.8 kg/m3"));
    assert!(diagnostics.is_empty());
}

#[test]
fn missing_density_follows_the_water_temperature() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.temperature = "80 degC".to_string();
    let case = case_from_project(&project).unwrap();
    assert!((case.fluid.density.value.value() - 971.8).abs() < 0.01);

    project.operating.fluid = "custom".to_string();
    let case = case_from_project(&project).unwrap();
    assert!((case.fluid.density.value.value() - 998.2).abs() < 1e-9);
}

#[test]
fn water_outside_the_correlation_warns() {
    let diagnostics = fluid_diagnostics(&water(Some("200 degC"), "865 kg/m3"));
    assert_eq!(codes(&diagnostics), ["HM-DES-010"]);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "el agua a 200.0 °C queda fuera de la correlación de densidad (0..150 °C); no se comprobó rho"
    );
}

#[test]
fn water_density_at_is_none_outside_the_correlation() {
    assert!(water_density_at(&temperature("-1 degC")).is_none());
    assert!(water_density_at(&temperature("200 degC")).is_none());
    let density = water_density_at(&temperature("20 degC")).unwrap();
    assert!((density.si - 998.2).abs() < 0.01);
}

#[test]
fn other_fluids_are_not_checked() {
    assert!(fluid_diagnostics(&custom("custom", Some("20 degC"), "1200 kg/m3")).is_empty());
    assert!(fluid_diagnostics(&custom("glycol", Some("80 degC"), "1050 kg/m3")).is_empty());
}

#[test]
fn npsh_criterion_warns_for_fluids_other_than_water() {
    let custom = custom("custom", None, "900 kg/m3");
    let diagnostics = npsh_fluid_diagnostics(&custom, true);
    assert_eq!(codes(&diagnostics), ["HM-DES-018"]);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "el NPSHA necesario supone agua (FF = 1) y el fluido es `custom`; la tabla 6.2 de Gülich pide FF ≥ 1,15 con agua de mar o fluidos corrosivos y otros criterios con hidrocarburos"
    );
    assert!(npsh_fluid_diagnostics(&water(None, "998.2 kg/m3"), true).is_empty());
    assert!(npsh_fluid_diagnostics(&custom, false).is_empty());
}

#[test]
fn suction_npsha_uses_iapws_vapor_pressure_for_water() {
    let suction = installation("1 atm", Some("-3 m"), Some("0.5 m"), None);
    let cold = water(Some("20 degC"), "998.2 kg/m3");
    // (101325 − 2339,2)/(998,2·9,80665) − 3 − 0,5
    let npsha_cold = npsha(&suction, &cold).unwrap().unwrap();
    assert!((npsha_cold.si - 6.612).abs() < 1e-3, "{}", npsha_cold.si);

    let same = npsha(&suction, &water(None, "998.2 kg/m3"))
        .unwrap()
        .unwrap();
    assert!((same.si - npsha_cold.si).abs() < 1e-9);

    // Más caliente, más presión de vapor y menos NPSHa.
    let hot = npsha(&suction, &water(Some("80 degC"), "971.8 kg/m3"))
        .unwrap()
        .unwrap();
    assert!(hot.si < npsha_cold.si - 4.0, "{}", hot.si);

    let error = npsha(&suction, &custom("glycol", Some("20 degC"), "1050 kg/m3")).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Suction));
    assert_eq!(error.code(), Some("HM-PROJ-029"));
    assert_eq!(
        error.diagnostic().unwrap().message,
        "el fluido es `glycol`: indica la presión de vapor de la aspiración"
    );

    let given = installation("1 atm", Some("-3 m"), Some("0.5 m"), Some("0 Pa"));
    let no_vapor = npsha(&given, &custom("glycol", Some("20 degC"), "1000 kg/m3"))
        .unwrap()
        .unwrap();
    assert!((no_vapor.si - (101_325.0 / (1000.0 * 9.80665) - 3.5)).abs() < 1e-9);

    let absent = installation("1 atm", None, None, Some("0 Pa"));
    let written = installation("1 atm", Some("0 m"), Some("0 m"), Some("0 Pa"));
    let glycol = custom("glycol", None, "1000 kg/m3");
    let from_absent = npsha(&absent, &glycol).unwrap().unwrap();
    let from_zero = npsha(&written, &glycol).unwrap().unwrap();
    assert!((from_absent.si - from_zero.si).abs() < 1e-12);
    assert!((from_absent.si - 101_325.0 / (1000.0 * 9.80665)).abs() < 1e-9);
}

#[test]
fn vapor_pressure_outside_the_correlation_keeps_the_cli_sentence() {
    let suction = installation("1 atm", Some("-3 m"), Some("0.5 m"), None);
    let error = npsha(&suction, &water(Some("400 degC"), "865 kg/m3")).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Suction));
    assert_eq!(error.code(), Some("HM-PROJ-027"));
    assert_eq!(
        cli_message(&error),
        "la temperatura está fuera del intervalo de la presión de vapor del agua"
    );
}

#[test]
fn viscosity_can_be_kinematic_or_dynamic() {
    let mut oil = fluid(
        FluidKind::Custom("oil".to_string()),
        None,
        "900 kg/m3",
        Some(viscosity("30 cSt")),
    );
    assert!((kinematic_viscosity(&oil).unwrap().unwrap() - 30e-6).abs() < 1e-15);
    oil.viscosity = Some(viscosity("27 cP"));
    assert!((kinematic_viscosity(&oil).unwrap().unwrap() - 30e-6).abs() < 1e-15);

    let water = water(Some("20 degC"), "998.2 kg/m3");
    assert!((kinematic_viscosity(&water).unwrap().unwrap() - 1.0034e-6).abs() < 1e-9);
    assert_eq!(
        kinematic_viscosity(&custom("oil", None, "900 kg/m3")).unwrap(),
        None
    );

    let error = viscosity_from_text("3 m").unwrap_err();
    assert_eq!(error.code(), Some("HM-PROJ-026"));
    assert_eq!(
        error.diagnostic().unwrap().message,
        "la viscosidad debe ser m2/s (cSt) o Pa*s (cP): 3 m"
    );
}

#[test]
fn viscosity_effect_reports_missing_and_large_effects() {
    let result = design(&DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: Some(parse_quantity("10 m").unwrap()),
        nu: None,
    })
    .unwrap();
    let input = DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity("900 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
        nu: None,
    };
    let oil = custom("oil", Some("20 degC"), "900 kg/m3");
    let (diagnostics, correction) = viscosity_effect(&oil, &input, &result).unwrap();
    assert_eq!(codes(&diagnostics), ["HM-DES-022"]);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "el fluido es `oil` y no tiene viscosidad: no se evaluó su efecto"
    );
    assert!(correction.is_none());

    let viscous = fluid(
        FluidKind::Custom("oil".to_string()),
        Some("20 degC"),
        "900 kg/m3",
        Some(viscosity("100 cSt")),
    );
    let (diagnostics, correction) = viscosity_effect(&viscous, &input, &result).unwrap();
    assert_eq!(codes(&diagnostics), ["HM-DES-020"]);
    assert!(correction.unwrap().f_eta < 0.85);
}

#[test]
fn npsha_and_suction_exclude_each_other() {
    let mut project = Project::new("demo", PumpType::EndSuction);
    project.operating.npsha = "10 m".to_string();
    project.suction = Some(Suction {
        pressure: "1 atm".to_string(),
        height: String::new(),
        losses: String::new(),
        vapor_pressure: String::new(),
    });

    let error = case_from_project(&project).unwrap_err();
    assert_eq!(error.field(), Some(CaseField::Npsha));
    assert_eq!(error.code(), Some("HM-PROJ-028"));

    let available = Intake::Available(Entered::parse("10 m").unwrap());
    let quantity = npsha(&available, &water(None, "998.2 kg/m3"))
        .unwrap()
        .unwrap();
    assert!((quantity.si - 10.0).abs() < 1e-12);
    assert!(
        npsha(&Intake::None, &water(None, "998.2 kg/m3"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn densities_of_liquid_water_are_recognised() {
    assert!(looks_like_water(998.2));
    assert!(looks_like_water(971.8));
    assert!(looks_like_water(917.0));
    assert!(!looks_like_water(1200.0));
    assert!(!looks_like_water(850.0));
}

#[test]
fn empty_fluid_is_water() {
    assert_eq!(fluid_kind(""), FluidKind::Water);
    assert_eq!(fluid_kind("Water"), FluidKind::Water);
    assert_eq!(
        fluid_kind("glycol"),
        FluidKind::Custom("glycol".to_string())
    );
}

#[test]
fn an_empty_fluid_name_is_water_for_every_diagnostic() {
    // El proyecto y la densidad por defecto ya tratan «» como agua; los
    // diagnósticos también: avisan de una densidad que no es de agua y no
    // dicen que el fluido «no es agua».
    let empty = fluid_kind("");
    assert_eq!(empty, FluidKind::Water);

    let heavy = fluid(empty, Some("20 degC"), "1200 kg/m3", None);
    assert!(codes(&fluid_diagnostics(&heavy)).contains(&"HM-DES-009"));
    assert!(npsh_fluid_diagnostics(&heavy, true).is_empty());
}

fn design_input_for(rho: &str) -> DesignInput {
    DesignInput {
        q: parse_quantity("120 m3/h").unwrap(),
        h: parse_quantity("45 m").unwrap(),
        n: parse_quantity("2900 rpm").unwrap(),
        rho: parse_quantity(rho).unwrap(),
        eta_m: 0.96,
        npsha: None,
        nu: None,
    }
}

#[test]
fn dynamic_viscosity_is_divided_by_the_density_in_the_effect() {
    // 50 cP con ρ = 800 kg/m3 son 62.5 cSt: el mismo efecto que 62.5 cSt.
    let input = design_input_for("800 kg/m3");
    let result = design(&input).unwrap();
    let dynamic = fluid(
        FluidKind::Custom("aceite".to_string()),
        None,
        "800 kg/m3",
        Some(viscosity("50 cP")),
    );
    let kinematic = fluid(
        FluidKind::Custom("aceite".to_string()),
        None,
        "800 kg/m3",
        Some(viscosity("62.5 cSt")),
    );

    assert!((kinematic_viscosity(&dynamic).unwrap().unwrap() - 62.5e-6).abs() < 1e-12);
    let (_, by_dynamic) = viscosity_effect(&dynamic, &input, &result).unwrap();
    let (_, by_kinematic) = viscosity_effect(&kinematic, &input, &result).unwrap();
    let (by_dynamic, by_kinematic) = (by_dynamic.unwrap(), by_kinematic.unwrap());
    assert!(by_dynamic.f_h < 1.0, "la corrección tiene que notarse");
    assert!((by_dynamic.f_h - by_kinematic.f_h).abs() < 1e-12);
    assert!((by_dynamic.f_eta - by_kinematic.f_eta).abs() < 1e-12);
}

#[test]
fn water_viscosity_is_unknown_outside_the_correlation() {
    assert!(
        kinematic_viscosity(&water(Some("400 degC"), "998.2 kg/m3"))
            .unwrap()
            .is_none()
    );
    // Sin temperatura, 20 °C: hay valor.
    assert!(
        kinematic_viscosity(&water(None, "998.2 kg/m3"))
            .unwrap()
            .is_some()
    );
    // Otro fluido sin viscosidad escrita: no se conoce.
    assert!(
        kinematic_viscosity(&custom("aceite", None, "850 kg/m3"))
            .unwrap()
            .is_none()
    );
}
