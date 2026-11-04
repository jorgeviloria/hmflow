//! Lectura del proyecto `.hmf` hacia el modelo `Case`.

use hm_core::Diagnostic;
use hm_design::{DesignOptions, water_density};
use hm_project::{Param, Project};
use hm_units::{Dimension, Quantity, parse_quantity, parse_unit};

use crate::case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
use crate::error::{CaseField, EngineError};

/// Temperatura del agua cuando el proyecto no escribe ninguna, K (20 °C).
const DEFAULT_WATER_TEMPERATURE_K: f64 = 293.15;

/// Densidad cuando no se puede derivar ni está escrita, kg/m3.
const FALLBACK_DENSITY: f64 = 998.2;

/// Cómo se lee el texto del proyecto. `set` recibe el valor en la unidad
/// de la tabla.
pub enum Decode {
    /// Número en SI. Con `zero_is_none`, el cero es «sin dato».
    Float { zero_is_none: bool },
    /// Ángulo. El texto se lee en radianes SI y aquí vuelve a grados.
    Degrees { zero_is_none: bool },
    /// Entero entre 1 y 100. Con `zero_is_none`, el cero es «sin dato».
    Count { zero_is_none: bool },
    /// `balance_holes`: ausente o 0 es falso, 1 es verdadero.
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

    let kind = fluid_kind(&project.operating.fluid);
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

fn fluid_kind(fluid: &str) -> FluidKind {
    if fluid.is_empty() || fluid.eq_ignore_ascii_case("water") {
        FluidKind::Water
    } else {
        FluidKind::Custom(fluid.to_string())
    }
}

fn parse_entered(field: CaseField, code: &'static str, text: &str) -> Result<Entered, EngineError> {
    Entered::parse(text).map_err(|error| input_error(field, code, error.to_string()))
}

fn parse_temperature(project: &Project) -> Result<Option<Temperature>, EngineError> {
    let text = &project.operating.temperature;
    if text.trim().is_empty() {
        return Ok(None);
    }

    let quantity = parse_quantity(text)
        .map_err(|error| input_error(CaseField::Temperature, "HM-PROJ-006", error.to_string()))?;
    if quantity.dim() != Dimension::TEMPERATURE {
        return Err(input_error(
            CaseField::Temperature,
            "HM-PROJ-006",
            format!("la temperatura debe ser una temperatura: {text}"),
        ));
    }

    Ok(Some(Temperature {
        text: text.clone(),
        kelvin: quantity.si,
    }))
}

fn parse_viscosity(project: &Project) -> Result<Option<Viscosity>, EngineError> {
    let text = &project.operating.viscosity;
    if text.trim().is_empty() {
        return Ok(None);
    }

    let entered = Entered::parse(text)
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;
    let kinematic = parse_unit("m2/s")
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;
    let dynamic = parse_unit("Pa*s")
        .map_err(|error| input_error(CaseField::Viscosity, "HM-PROJ-026", error.to_string()))?;

    if entered.quantity.dim() == kinematic.dim {
        Ok(Some(Viscosity::Kinematic(entered)))
    } else if entered.quantity.dim() == dynamic.dim {
        Ok(Some(Viscosity::Dynamic(entered)))
    } else {
        Err(input_error(
            CaseField::Viscosity,
            "HM-PROJ-026",
            format!("la viscosidad debe ser m2/s (cSt) o Pa*s (cP): {text}"),
        ))
    }
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

    if let Some(text) = param_text(project, "rho")? {
        let value = param_f64(project, "rho", "kg/m3")?.ok_or_else(|| {
            input_error(
                CaseField::Density,
                "HM-PROJ-012",
                "`rho` no tiene un valor calculado".to_string(),
            )
        })?;
        return Ok(Density {
            value: Quantity::from_si(value, kg_m3),
            source: DensitySource::Explicit {
                text: text.to_string(),
            },
        });
    }

    // Sin temperatura escrita, el agua se toma a 20 °C y la densidad lo dice.
    let (kelvin, water_source) = match temperature {
        Some(temperature) => (temperature.kelvin, DensitySource::WaterAtTemperature),
        None => (
            DEFAULT_WATER_TEMPERATURE_K,
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
            param_angle(project, param.name)?.map(f64::to_degrees),
            zero_is_none,
        )),
        Decode::Count { zero_is_none } => {
            if zero_is_none && param_f64(project, param.name, "1")? == Some(0.0) {
                return Ok(None);
            }
            Ok(param_u32(project, param.name)?.map(f64::from))
        }
        Decode::Flag => match param_f64(project, param.name, "1")? {
            None => Ok(Some(0.0)),
            Some(value) if value == 0.0 || value == 1.0 => Ok(Some(value)),
            Some(_) => Err(input_error(
                CaseField::Param(param.name),
                "HM-PROJ-020",
                format!("`{}` debe ser 0 o 1", param.name),
            )),
        },
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

/// Un ángulo debe escribirse con unidad (`deg` o `rad`): `deg` y `rad` son
/// adimensionales y un número solo se leería como radianes.
fn param_angle(project: &Project, name: &'static str) -> Result<Option<f64>, EngineError> {
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
    if !matches!(quantity.unit.symbol.as_str(), "deg" | "rad") {
        return Err(input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe llevar unidad de ángulo (deg o rad): {text}"),
        ));
    }
    Ok(Some(quantity.si))
}

fn param_f64(
    project: &Project,
    name: &'static str,
    unit_text: &'static str,
) -> Result<Option<f64>, EngineError> {
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

    Ok(Some(quantity.si))
}

fn param_u32(project: &Project, name: &'static str) -> Result<Option<u32>, EngineError> {
    let Some(value) = param_f64(project, name, "1")? else {
        return Ok(None);
    };
    if value.fract() != 0.0 || !(1.0..=100.0).contains(&value) {
        return Err(input_error(
            CaseField::Param(name),
            "HM-PROJ-020",
            format!("`{name}` debe ser un entero entre 1 y 100"),
        ));
    }
    Ok(Some(value as u32))
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

fn input_error(field: CaseField, code: &'static str, message: impl Into<String>) -> EngineError {
    EngineError::Input {
        field,
        diagnostic: Diagnostic::error(code, message),
    }
}
