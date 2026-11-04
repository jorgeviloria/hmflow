//! Lectura del proyecto `.hmf` hacia el modelo `Case`.

use hm_core::Diagnostics;
use hm_design::{DesignInput, DesignOptions, DesignResult, ViscousCorrection, water_density};
use hm_project::{Param, Project, Suction};
use hm_units::{Quantity, parse_quantity, parse_unit};

use crate::case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
use crate::error::{CaseField, EngineError, input_error};
use crate::fluid::{
    self, fluid_diagnostics, kinematic_viscosity, npsh_fluid_diagnostics, npsha, viscosity_effect,
    viscosity_from_text,
};

/// Mayor conteo entero aceptado (álabes, canales de voluta); el mismo límite
/// que aplica la CLI y la auditoría.
pub const MAX_COUNT: f64 = 100.0;

/// Densidad cuando no se puede derivar ni está escrita, kg/m3.
const FALLBACK_DENSITY: f64 = 998.2;

/// Cómo se lee el texto del proyecto. `set` recibe el valor en la unidad
/// de la tabla.
pub enum Decode {
    /// Número en SI. Con `zero_is_none`, el cero es «sin dato».
    Float { zero_is_none: bool },
    /// Ángulo en grados. El texto debe llevar `deg` o `rad`.
    Degrees { zero_is_none: bool },
    /// Entero entre 1 y 100. Con `zero_is_none`, el cero es «sin dato».
    Count { zero_is_none: bool },
    /// `balance_holes`: ausente o 0 es falso, 1 es verdadero. Siempre da un
    /// valor (0 por defecto): la tabla de defaults de `recalc` lo muestra así.
    Flag,
}

/// Descripción de una opción manual del proyecto y su traducción a
/// `DesignOptions`. `get` y `set` usan la unidad de la tabla.
pub struct ManualParam {
    pub name: &'static str,
    pub unit: &'static str,
    pub decode: Decode,
    pub get: fn(&DesignOptions) -> Option<f64>,
    pub set: fn(&mut DesignOptions, Option<f64>),
}

impl ManualParam {
    /// El cero escrito en el proyecto significa «sin dato».
    pub fn zero_is_none(&self) -> bool {
        match self.decode {
            Decode::Float { zero_is_none }
            | Decode::Degrees { zero_is_none }
            | Decode::Count { zero_is_none } => zero_is_none,
            Decode::Flag => false,
        }
    }
}

macro_rules! direct_param {
    ($get:ident, $set:ident, $field:ident) => {
        fn $get(options: &DesignOptions) -> Option<f64> {
            options.$field
        }

        fn $set(options: &mut DesignOptions, value: Option<f64>) {
            options.$field = value;
        }
    };
}

direct_param!(get_psi, set_psi, psi);
direct_param!(get_phi, set_phi, phi);
direct_param!(get_c0, set_c0, c0);
direct_param!(get_cm0, set_cm0, cm0);
direct_param!(get_a, set_a, a);
direct_param!(get_j, set_j, j);
direct_param!(get_margin, set_margin, margin);
direct_param!(get_k3, set_k3, k3);
direct_param!(get_volute_c, set_volute_c, volute_width_factor);
direct_param!(get_k_eff, set_k_eff, k_eff);
direct_param!(get_eta_v_fixed, set_eta_v_fixed, eta_v);
direct_param!(get_volute_ratio, set_volute_ratio, volute_ratio);
direct_param!(
    get_impeller_roughness,
    set_impeller_roughness,
    impeller_roughness
);
direct_param!(get_volute_wrap_deg, set_volute_wrap_deg, volute_wrap_deg);
direct_param!(get_incidence_deg, set_incidence_deg, incidence_deg);

fn get_beta2(options: &DesignOptions) -> Option<f64> {
    options.beta2_rad.map(f64::to_degrees)
}

fn set_beta2(options: &mut DesignOptions, value: Option<f64>) {
    options.beta2_rad = value.map(f64::to_radians);
}

fn get_blade_count(options: &DesignOptions) -> Option<f64> {
    options.blade_count.map(f64::from)
}

fn set_blade_count(options: &mut DesignOptions, value: Option<f64>) {
    options.blade_count = value.map(|value| value as u32);
}

fn get_balance_holes(options: &DesignOptions) -> Option<f64> {
    Some(if options.balance_holes { 1.0 } else { 0.0 })
}

fn set_balance_holes(options: &mut DesignOptions, value: Option<f64>) {
    options.balance_holes = value == Some(1.0);
}

fn get_volute_channels(options: &DesignOptions) -> Option<f64> {
    options.volute_channels.map(f64::from)
}

fn set_volute_channels(options: &mut DesignOptions, value: Option<f64>) {
    options.volute_channels = value.map(|value| value as u32);
}

/// Opciones del método que se leen de `[params]`, en el orden en que las
/// valida y las usa el motor.
pub const MANUAL_PARAMS: &[ManualParam] = &[
    ManualParam {
        name: "psi",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_psi,
        set: set_psi,
    },
    ManualParam {
        name: "phi",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_phi,
        set: set_phi,
    },
    ManualParam {
        name: "beta2",
        unit: "deg",
        decode: Decode::Degrees {
            zero_is_none: false,
        },
        get: get_beta2,
        set: set_beta2,
    },
    ManualParam {
        name: "blade_count",
        unit: "1",
        decode: Decode::Count {
            zero_is_none: false,
        },
        get: get_blade_count,
        set: set_blade_count,
    },
    ManualParam {
        name: "c0",
        unit: "m/s",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_c0,
        set: set_c0,
    },
    ManualParam {
        name: "cm0",
        unit: "m/s",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_cm0,
        set: set_cm0,
    },
    ManualParam {
        name: "A",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_a,
        set: set_a,
    },
    ManualParam {
        name: "J",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_j,
        set: set_j,
    },
    ManualParam {
        name: "margin",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_margin,
        set: set_margin,
    },
    ManualParam {
        name: "k3",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_k3,
        set: set_k3,
    },
    ManualParam {
        name: "volute_c",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_volute_c,
        set: set_volute_c,
    },
    ManualParam {
        name: "k_eff",
        unit: "1",
        decode: Decode::Float {
            zero_is_none: false,
        },
        get: get_k_eff,
        set: set_k_eff,
    },
    ManualParam {
        name: "eta_v_fixed",
        unit: "1",
        decode: Decode::Float { zero_is_none: true },
        get: get_eta_v_fixed,
        set: set_eta_v_fixed,
    },
    ManualParam {
        name: "balance_holes",
        unit: "1",
        decode: Decode::Flag,
        get: get_balance_holes,
        set: set_balance_holes,
    },
    ManualParam {
        name: "volute_ratio",
        unit: "1",
        decode: Decode::Float { zero_is_none: true },
        get: get_volute_ratio,
        set: set_volute_ratio,
    },
    ManualParam {
        name: "impeller_roughness",
        unit: "m",
        decode: Decode::Float { zero_is_none: true },
        get: get_impeller_roughness,
        set: set_impeller_roughness,
    },
    ManualParam {
        name: "volute_channels",
        unit: "1",
        decode: Decode::Count { zero_is_none: true },
        get: get_volute_channels,
        set: set_volute_channels,
    },
    ManualParam {
        name: "volute_wrap_deg",
        unit: "deg",
        decode: Decode::Degrees { zero_is_none: true },
        get: get_volute_wrap_deg,
        set: set_volute_wrap_deg,
    },
    ManualParam {
        name: "incidence_deg",
        unit: "deg",
        decode: Decode::Degrees {
            zero_is_none: false,
        },
        get: get_incidence_deg,
        set: set_incidence_deg,
    },
];

/// Convierte un proyecto en el modelo de entradas del caso.
///
/// No valida el conjunto ni el método: solo traduce texto y rechaza overrides.
pub fn case_from_project(project: &Project) -> Result<Case, EngineError> {
    let q = parse_entered(CaseField::Q, "HM-PROJ-002", &project.operating.q)?;
    let h = parse_entered(CaseField::H, "HM-PROJ-003", &project.operating.h)?;
    let n = parse_entered(CaseField::N, "HM-PROJ-004", &project.operating.n)?;

    let kind = fluid::fluid_kind(&project.operating.fluid);
    let temperature = parse_temperature(project)?;
    let density = default_density(project, &kind, temperature.as_ref())?;
    let mechanical_efficiency = param_f64(project, "eta_m", "1")?;
    let intake = parse_intake(project)?;
    let design = design_options(project)?;
    let viscosity = parse_viscosity(project)?;

    Ok(Case {
        pump_type: project.design.pump_type,
        duty: Duty { q, h, n },
        fluid: Fluid {
            kind,
            temperature,
            density,
            viscosity,
        },
        intake,
        mechanical_efficiency,
        design,
    })
}

fn parse_entered(field: CaseField, code: &'static str, text: &str) -> Result<Entered, EngineError> {
    Entered::parse(text).map_err(|error| input_error(field, code, error.to_string()))
}

fn parse_temperature(project: &Project) -> Result<Option<Temperature>, EngineError> {
    read_temperature(&project.operating.temperature)
}

fn parse_viscosity(project: &Project) -> Result<Option<Viscosity>, EngineError> {
    viscosity_from_text(&project.operating.viscosity)
}

fn parse_intake(project: &Project) -> Result<Intake, EngineError> {
    if let Some(suction) = &project.suction {
        // Un NPSHa en [operating] y otro calculado con [suction] se contradicen.
        if !project.operating.npsha.trim().is_empty() {
            return Err(input_error(
                CaseField::Npsha,
                "HM-PROJ-028",
                "el NPSHa está en [operating] y también se calcula con [suction]; deja solo uno de los dos",
            ));
        }
        let pressure = parse_suction("pressure", &suction.pressure)?;
        let height = parse_optional_suction("height", &suction.height)?;
        let losses = parse_optional_suction("losses", &suction.losses)?;
        let vapor_pressure = parse_optional_suction("vapor_pressure", &suction.vapor_pressure)?;

        return Ok(Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        });
    }

    if project.operating.npsha.trim().is_empty() {
        Ok(Intake::None)
    } else {
        Ok(Intake::Available(parse_entered(
            CaseField::Npsha,
            "HM-PROJ-005",
            &project.operating.npsha,
        )?))
    }
}

/// Lee un dato de `[suction]`. `CaseField::Suction` agrupa los cuatro, así
/// que el mensaje nombra cuál falló.
fn parse_suction(name: &str, text: &str) -> Result<Entered, EngineError> {
    Entered::parse(text).map_err(|error| {
        input_error(
            CaseField::Suction,
            "HM-PROJ-027",
            format!("[suction] `{name}`: {error}"),
        )
    })
}

fn parse_optional_suction(name: &str, text: &str) -> Result<Option<Entered>, EngineError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(parse_suction(name, text)?))
    }
}

fn default_density(
    project: &Project,
    kind: &FluidKind,
    temperature: Option<&Temperature>,
) -> Result<Density, EngineError> {
    let kg_m3 = parse_unit("kg/m3")
        .map_err(|error| input_error(CaseField::Density, "HM-PROJ-012", error.to_string()))?;

    if let Some((text, quantity)) = param_quantity(project, "rho", "kg/m3")? {
        return Ok(Density {
            value: Quantity::from_si(quantity.si, kg_m3),
            source: DensitySource::Explicit {
                text: text.to_string(),
            },
        });
    }

    // Sin temperatura escrita, el agua se toma a 20 °C y la densidad lo dice.
    let (kelvin, water_source) = match temperature {
        Some(temperature) => (temperature.kelvin, DensitySource::WaterAtTemperature),
        None => (
            fluid::DEFAULT_WATER_TEMPERATURE_K,
            DensitySource::WaterAtDefaultTemperature,
        ),
    };
    let derived = if matches!(kind, FluidKind::Water) {
        water_density(kelvin).map(|value| (value, water_source))
    } else {
        None
    };

    // Fuera de 0..150 °C, o con otro fluido sin `rho`, queda la densidad por
    // defecto; el aviso lo da el diagnóstico del fluido.
    let (value, source) = derived.unwrap_or((FALLBACK_DENSITY, DensitySource::Fallback));

    Ok(Density {
        value: Quantity::from_si(value, kg_m3),
        source,
    })
}

fn design_options(project: &Project) -> Result<DesignOptions, EngineError> {
    let mut options = DesignOptions::default();

    for param in MANUAL_PARAMS {
        (param.set)(&mut options, decode(project, param)?);
    }

    Ok(options)
}

fn decode(project: &Project, param: &ManualParam) -> Result<Option<f64>, EngineError> {
    match param.decode {
        Decode::Float { zero_is_none } => Ok(without_zero(
            param_f64(project, param.name, param.unit)?,
            zero_is_none,
        )),
        Decode::Degrees { zero_is_none } => Ok(without_zero(
            param_angle(project, param.name)?,
            zero_is_none,
        )),
        Decode::Count { zero_is_none } => {
            let value = without_zero(param_f64(project, param.name, "1")?, zero_is_none);
            value.map(|value| count(param.name, value)).transpose()
        }
        Decode::Flag => flag(project, param.name),
    }
}

/// Entero entre 1 y `MAX_COUNT`.
fn count(name: &'static str, value: f64) -> Result<f64, EngineError> {
    if value.fract() != 0.0 || !(1.0..=MAX_COUNT).contains(&value) {
        return Err(input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe ser un entero entre 1 y {MAX_COUNT}"),
        ));
    }
    Ok(value)
}

/// `balance_holes`: ausente es falso; si está, 0 o 1.
fn flag(project: &Project, name: &'static str) -> Result<Option<f64>, EngineError> {
    let wrong = || {
        input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe ser 0 o 1"),
        )
    };
    let Some(text) = param_text(project, name)? else {
        return Ok(Some(0.0));
    };
    match parse_quantity(text) {
        Ok(quantity) if quantity.si == 0.0 || quantity.si == 1.0 => Ok(Some(quantity.si)),
        _ => Err(wrong()),
    }
}

fn without_zero(value: Option<f64>, zero_is_none: bool) -> Option<f64> {
    if zero_is_none {
        value.filter(|value| *value != 0.0)
    } else {
        value
    }
}

fn param_text<'a>(
    project: &'a Project,
    name: &'static str,
) -> Result<Option<&'a str>, EngineError> {
    let Some(param) = project.params.get(name) else {
        return Ok(None);
    };
    if matches!(param, Param::Override { .. }) {
        return Err(override_error(name));
    }
    Ok(param_value(param))
}

/// Texto y cantidad de un parámetro, con la dimensión de `unit_text`.
fn param_quantity<'a>(
    project: &'a Project,
    name: &'static str,
    unit_text: &'static str,
) -> Result<Option<(&'a str, Quantity)>, EngineError> {
    let Some(text) = param_text(project, name)? else {
        return Ok(None);
    };

    let quantity = parse_quantity(text).map_err(|error| {
        input_error(
            CaseField::Param(name),
            "HM-PROJ-012",
            format!("`{name}`: {error}"),
        )
    })?;
    let expected = parse_unit(unit_text)
        .map_err(|error| input_error(CaseField::Param(name), "HM-PROJ-012", error.to_string()))?;
    if quantity.dim() != expected.dim {
        return Err(input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe ser {unit_text}"),
        ));
    }

    Ok(Some((text, quantity)))
}

fn param_f64(
    project: &Project,
    name: &'static str,
    unit_text: &'static str,
) -> Result<Option<f64>, EngineError> {
    Ok(param_quantity(project, name, unit_text)?.map(|(_, quantity)| quantity.si))
}

/// Un ángulo en grados. Debe escribirse con unidad (`deg` o `rad`): ambas son
/// adimensionales y un número solo se leería como radianes. En grados se usa
/// el número escrito, sin el viaje a radianes y de vuelta.
fn param_angle(project: &Project, name: &'static str) -> Result<Option<f64>, EngineError> {
    let Some((text, quantity)) = param_quantity(project, name, "1")? else {
        return Ok(None);
    };
    match quantity.unit.symbol.as_str() {
        "deg" => Ok(Some(
            leading_number(text).unwrap_or_else(|| quantity.si.to_degrees()),
        )),
        "rad" => Ok(Some(quantity.si.to_degrees())),
        _ => Err(input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe llevar unidad de ángulo (deg o rad): {text}"),
        )),
    }
}

/// El número con el que empieza un texto como `"20 deg"`.
fn leading_number(text: &str) -> Option<f64> {
    let text = text.trim_start();
    let end = text
        .find(|c: char| !matches!(c, '0'..='9' | '.' | '+' | '-' | 'e' | 'E'))
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

fn param_value(param: &Param) -> Option<&str> {
    match param {
        Param::Manual { value } | Param::Locked { value } => Some(value),
        Param::Auto { value, .. } => value.as_deref(),
        Param::Override { .. } => None,
    }
}

fn override_error(name: &'static str) -> EngineError {
    input_error(
        CaseField::Param(name),
        "HM-PROJ-019",
        format!("`{name}` está en override; la auditoría no evalúa overrides"),
    )
}

/// NPSHa de una instalación de aspiración con la fórmula `npsh.npsha`.
///
/// La presión de vapor es la de `[suction]` o, si falta y el fluido es agua,
/// la de IAPWS-IF97 a la temperatura del proyecto (20 °C si no se indica).
pub fn suction_npsha(
    suction: &Suction,
    fluid: &str,
    temperature: &str,
    rho: f64,
) -> Result<Quantity, EngineError> {
    let vapor_pressure = parse_optional_suction("vapor_pressure", &suction.vapor_pressure)?;
    let kind = fluid::fluid_kind(fluid);
    // La temperatura solo se lee cuando hace falta para la presión de vapor.
    let temperature = if vapor_pressure.is_none() && matches!(kind, FluidKind::Water) {
        read_temperature(temperature)?
    } else {
        None
    };
    let intake = Intake::Installation {
        pressure: parse_suction("pressure", &suction.pressure)?,
        height: parse_optional_suction("height", &suction.height)?,
        losses: parse_optional_suction("losses", &suction.losses)?,
        vapor_pressure,
    };
    let fluid = Fluid {
        kind,
        temperature,
        density: density_from_si(rho)?,
        viscosity: None,
    };
    match npsha(&intake, &fluid)? {
        Some(quantity) => Ok(quantity),
        None => Err(EngineError::Design(hm_design::DesignError::Input(
            "la instalación no produjo NPSHa".into(),
        ))),
    }
}

/// Viscosidad cinemática a partir del texto de operación.
///
/// Un texto de viscosidad se interpreta antes que la temperatura: si la
/// viscosidad está escrita, una temperatura ilegible no impide el cálculo.
pub fn kinematic_viscosity_of(
    viscosity: &str,
    fluid: &str,
    temperature: &str,
    rho: f64,
) -> Result<Option<f64>, EngineError> {
    kinematic_viscosity(&fluid_from_text(viscosity, fluid, temperature, rho)?)
}

/// Avisos de densidad y de criterio NPSH del fluido, en ese orden.
///
/// Una temperatura ilegible no añade el aviso de densidad: la validación del
/// proyecto ya la señala.
pub fn add_fluid_temperature_diagnostics(
    project: &Project,
    input: &DesignInput,
    diagnostics: &mut Diagnostics,
) {
    let kind = fluid::fluid_kind(&project.operating.fluid);
    let temperature = match read_temperature(&project.operating.temperature) {
        Ok(temperature) => temperature,
        Err(_) => {
            diagnostics.extend(
                npsh_fluid_diagnostics(&bare_fluid(kind, input.rho.clone()), input.npsha.is_some())
                    .into_vec(),
            );
            return;
        }
    };
    let fluid = Fluid {
        kind,
        temperature,
        density: density_from(input.rho.clone()),
        viscosity: None,
    };
    diagnostics.extend(fluid_diagnostics(&fluid).into_vec());
    diagnostics.extend(npsh_fluid_diagnostics(&fluid, input.npsha.is_some()).into_vec());
}

pub fn project_viscosity_effect(
    project: &Project,
    input: &DesignInput,
    result: &DesignResult,
) -> Result<(Diagnostics, Option<ViscousCorrection>), EngineError> {
    let fluid = fluid_from_text(
        &project.operating.viscosity,
        &project.operating.fluid,
        &project.operating.temperature,
        input.rho.si,
    )?;
    viscosity_effect(&fluid, input, result)
}

fn fluid_from_text(
    viscosity: &str,
    fluid: &str,
    temperature: &str,
    rho: f64,
) -> Result<Fluid, EngineError> {
    let viscosity = viscosity_from_text(viscosity)?;
    let kind = fluid::fluid_kind(fluid);
    let temperature = if viscosity.is_none() && matches!(kind, FluidKind::Water) {
        read_temperature(temperature)?
    } else {
        None
    };
    Ok(Fluid {
        kind,
        temperature,
        density: density_from_si(rho)?,
        viscosity,
    })
}

fn read_temperature(text: &str) -> Result<Option<Temperature>, EngineError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(fluid::parse_temperature(text)?))
    }
}

fn bare_fluid(kind: FluidKind, rho: Quantity) -> Fluid {
    Fluid {
        kind,
        temperature: None,
        density: density_from(rho),
        viscosity: None,
    }
}

fn density_from(value: Quantity) -> Density {
    Density {
        value,
        source: DensitySource::Explicit {
            text: String::new(),
        },
    }
}

fn density_from_si(si: f64) -> Result<Density, EngineError> {
    let unit = parse_unit("kg/m3")
        .map_err(|error| EngineError::Design(hm_design::DesignError::from(error)))?;
    Ok(density_from(Quantity::from_si(si, unit)))
}
