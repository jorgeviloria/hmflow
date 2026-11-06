//! Lectura del proyecto `.hmf` hacia el modelo `Case`, y escritura de los
//! parámetros por defecto del método.

use std::collections::BTreeMap;

use hm_design::geometry::DEFAULT_INCIDENCE_DEG;
use hm_design::{
    CasingFit, CasingLeakage, DEFAULT_K_EFF, DEFAULT_VOLUTE_C, DEFAULT_VOLUTE_RATIO, DesignError,
    DesignOptions, DesignResult, MODEL_VOLUTE_PARAMS, PARAM_SPECS, ParamSpec, designed_si,
    water_density,
};
use hm_project::{Casing, Param, Project};
use hm_units::{Quantity, parse_quantity, parse_unit};

use crate::case::{
    Case, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, Temperature, Viscosity,
};
use crate::error::{CaseField, EngineError, input_error};
use crate::fluid::{self, viscosity_from_text};

/// Mayor conteo entero aceptado (álabes, canales de voluta). Lo usan la
/// lectura del caso y la auditoría.
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

/// Las tres cotas que fijan el ojo: van juntas.
const EYE_SELECTIONS: [&str; 3] = ["c0", "cm0", "J"];

/// Con carcasa, un valor del ojo en `locked` fija el ojo entero.
pub(crate) fn eye_is_fixed(project: &Project) -> bool {
    project.casing.is_some()
        && EYE_SELECTIONS
            .iter()
            .any(|name| matches!(project.params.get(*name), Some(Param::Locked { .. })))
}

pub(crate) fn is_eye_selection(name: &str) -> bool {
    EYE_SELECTIONS.contains(&name)
}

/// Selecciones que, en este proyecto, son resultado y no decisión: ψ sale de
/// D2 y el ojo del cuello. Las que `design` dejó como `manual` quedan viejas
/// al cambiar el punto. `locked` es la decisión: ψ se fija solo, y un valor
/// del ojo en `locked` conserva los otros dos, porque c0 sin J no es un ojo.
pub(crate) fn casing_results(project: &Project) -> Vec<&'static str> {
    if project.casing.is_none() {
        return Vec::new();
    }
    let locked = |name: &str| matches!(project.params.get(name), Some(Param::Locked { .. }));
    let mut names = Vec::new();
    if !locked("psi") {
        names.push("psi");
    }
    if !EYE_SELECTIONS.iter().any(|name| locked(name)) {
        names.extend(EYE_SELECTIONS);
    }
    names
}

/// Parámetros de voluta que una carcasa sustituye.
pub(crate) const CASING_COPIES: [&str; 5] = [
    "k3",
    "volute_c",
    "volute_ratio",
    "volute_channels",
    "volute_wrap_deg",
];

/// Parámetros de voluta que, con carcasa, son copia de `[casing]`. Solo un
/// `locked` se contrasta con ella.
pub(crate) fn casing_volute(project: &Project) -> Vec<&'static str> {
    if project.casing.is_none() {
        return Vec::new();
    }
    CASING_COPIES
        .into_iter()
        .filter(|name| !matches!(project.params.get(*name), Some(Param::Locked { .. })))
        .collect()
}

fn design_options(project: &Project) -> Result<DesignOptions, EngineError> {
    let mut options = DesignOptions::default();

    let from_casing = [casing_results(project), casing_volute(project)].concat();
    for param in MANUAL_PARAMS {
        let value = decode(project, param)?;
        if !from_casing.contains(&param.name) {
            (param.set)(&mut options, value);
        }
    }

    if let Some(casing) = &project.casing {
        options.casing = Some(casing_fit(casing)?);
    }

    Ok(options)
}

/// `[casing]` en SI. `validate` ya habla de cada cota, pero este camino
/// también se recorre sin validar.
pub fn casing_fit(casing: &Casing) -> Result<CasingFit, EngineError> {
    let si = |name: &'static str, text: &str, unit: &str| {
        let wrong = |problem: String| {
            input_error(
                CaseField::Casing(name),
                "HM-PROJ-030",
                format!("[casing] `{name}` {problem}: {text}"),
            )
        };
        let quantity = parse_quantity(text).map_err(|error| wrong(error.to_string()))?;
        let expected = parse_unit(unit).map_err(|error| wrong(error.to_string()))?;
        if quantity.dim() != expected.dim {
            return Err(wrong(format!("debe ser {unit}")));
        }
        Ok(quantity.si)
    };
    let throat_area = match casing.throat_area.trim() {
        "" => None,
        text => Some(si("throat_area", text, "m2")?),
    };
    let leakage = casing
        .has_leakage()
        .then(|| {
            if casing.balance_hole_count == 0 {
                return Err(input_error(
                    CaseField::Casing("balance_hole_count"),
                    "HM-PROJ-030",
                    "[casing] `balance_hole_count` hace falta con los demás datos de fuga",
                ));
            }
            Ok(CasingLeakage {
                seal_clearance: si("seal_clearance", &casing.seal_clearance, "m")?,
                seal_length: si("seal_length", &casing.seal_length, "m")?,
                rear_seal_diameter: si("rear_seal_diameter", &casing.rear_seal_diameter, "m")?,
                rear_seal_length: si("rear_seal_length", &casing.rear_seal_length, "m")?,
                balance_hole_count: casing.balance_hole_count,
                balance_hole_diameter: si(
                    "balance_hole_diameter",
                    &casing.balance_hole_diameter,
                    "m",
                )?,
                balance_hole_circle: si("balance_hole_circle", &casing.balance_hole_circle, "m")?,
                shaft_seal_diameter: match casing.shaft_seal_diameter.trim() {
                    "" => None,
                    text => Some(si("shaft_seal_diameter", text, "m")?),
                },
            })
        })
        .transpose()?;
    let wrap_deg = match casing.wrap.trim() {
        "" => None,
        text => Some(si("wrap", text, "rad")?.to_degrees()),
    };
    let fit = CasingFit {
        d2: si("d2", &casing.d2, "m")?,
        neck_diameter: si("neck_diameter", &casing.neck_diameter, "m")?,
        eye_diameter_max: si("eye_diameter_max", &casing.eye_diameter_max, "m")?,
        bore_diameter: si("bore_diameter", &casing.bore_diameter, "m")?,
        hub_diameter: si("hub_diameter", &casing.hub_diameter, "m")?,
        b3: si("b3", &casing.b3, "m")?,
        cutwater_radius: si("cutwater_radius", &casing.cutwater_radius, "m")?,
        channels: casing.channels,
        wrap_deg,
        throat_area,
        leakage,
    };
    // Que encajen entre sí también es un problema de la carcasa, no del diseño.
    fit.validate().map_err(|error| {
        input_error(CaseField::Casing(error.name), "HM-PROJ-030", error.message)
    })?;
    Ok(fit)
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
    // Una unidad (`1 m`) no es una bandera: se rechaza igual que un texto.
    match param_quantity(project, name, "1") {
        Ok(None) => Ok(Some(0.0)),
        Ok(Some((_, quantity))) if quantity.si == 0.0 || quantity.si == 1.0 => {
            Ok(Some(quantity.si))
        }
        Ok(Some(_)) => Err(wrong()),
        Err(EngineError::Input { diagnostic, .. }) if diagnostic.code == "HM-PROJ-019" => {
            Err(override_error(name))
        }
        Err(_) => Err(wrong()),
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
    param_quantity_shown(project, name, unit_text, unit_text)
}

/// Como `param_quantity`, con otra unidad en el mensaje de dimensión
/// incorrecta (`deg` en vez de `1` para un ángulo).
fn param_quantity_shown<'a>(
    project: &'a Project,
    name: &'static str,
    unit_text: &'static str,
    shown: &'static str,
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
            format!("`{name}` debe ser {shown}"),
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
    let Some((text, quantity)) = param_quantity_shown(project, name, "1", "deg")? else {
        return Ok(None);
    };
    match quantity.unit.symbol.as_str() {
        // Un cero no necesita unidad: es «sin dato» o 0°, sea cual sea.
        "1" if quantity.si == 0.0 => Ok(Some(0.0)),
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

/// Texto guardado de un parámetro, si el modo lo tiene.
pub(crate) fn param_value(param: &Param) -> Option<&str> {
    match param {
        Param::Manual { value } | Param::Locked { value } => Some(value),
        Param::Auto { value, .. } => value.as_deref(),
        Param::Override { .. } => None,
    }
}

pub(crate) fn override_error(name: &'static str) -> EngineError {
    input_error(
        CaseField::Param(name),
        "HM-PROJ-019",
        format!("`{name}` está en override; la auditoría no evalúa overrides"),
    )
}

fn read_temperature(text: &str) -> Result<Option<Temperature>, EngineError> {
    if text.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(fluid::parse_temperature(text)?))
    }
}

/// Texto de los parámetros manuales que no salen de la selección del motor.
pub struct ManualDefaults {
    rho: String,
    eta_m: String,
    /// Opciones del método v0.9, como texto de su parámetro.
    eta_v_fixed: String,
    balance_holes: String,
    volute_ratio: String,
    impeller_roughness: String,
    volute_channels: String,
    volute_wrap_deg: String,
    incidence_deg: String,
}

impl ManualDefaults {
    /// Con las opciones por defecto del motor.
    pub fn engine(rho: String, eta_m: String) -> Self {
        Self::with_options(rho, eta_m, &DesignOptions::default())
    }

    /// Con los valores de `options`; «0» donde no hay dato.
    pub fn with_options(rho: String, eta_m: String, options: &DesignOptions) -> Self {
        Self {
            rho,
            eta_m,
            eta_v_fixed: options.eta_v.unwrap_or(0.0).to_string(),
            balance_holes: u8::from(options.balance_holes).to_string(),
            volute_ratio: options
                .volute_ratio
                .unwrap_or(DEFAULT_VOLUTE_RATIO)
                .to_string(),
            impeller_roughness: format!("{} m", options.impeller_roughness.unwrap_or(0.0)),
            volute_channels: options.volute_channels.unwrap_or(1).to_string(),
            volute_wrap_deg: format!("{} deg", options.volute_wrap_deg.unwrap_or(0.0)),
            incidence_deg: format!(
                "{} deg",
                options.incidence_deg.unwrap_or(DEFAULT_INCIDENCE_DEG)
            ),
        }
    }
}

/// Valor manual que usó el motor para `name`.
fn default_manual_text(
    name: &str,
    result: &DesignResult,
    defaults: &ManualDefaults,
) -> Option<String> {
    let selection = &result.selection;
    Some(match name {
        "rho" => defaults.rho.clone(),
        "eta_m" => defaults.eta_m.clone(),
        "mm" => "0.001 m".to_string(),
        "psi" => selection.psi.to_string(),
        "phi" => selection.phi.to_string(),
        "beta2" => format!("{} deg", selection.beta2_deg),
        "blade_count" => selection.blade_count.to_string(),
        "c0" => format!("{} m/s", selection.c0),
        "cm0" => format!("{} m/s", selection.cm0),
        "A" => selection.a.to_string(),
        "J" => selection.j.to_string(),
        "margin" => selection.margin.to_string(),
        "k3" => (result.volute.base_radius.si / (result.d2.si / 2.0)).to_string(),
        "volute_c" => DEFAULT_VOLUTE_C.to_string(),
        "k_eff" => DEFAULT_K_EFF.to_string(),
        "eta_v_fixed" => defaults.eta_v_fixed.clone(),
        "balance_holes" => defaults.balance_holes.clone(),
        "volute_ratio" => defaults.volute_ratio.clone(),
        "impeller_roughness" => defaults.impeller_roughness.clone(),
        "volute_channels" => match &result.casing {
            Some(casing) => casing.channels.to_string(),
            None => defaults.volute_channels.clone(),
        },
        "volute_wrap_deg" => match (&result.casing, result.volute.wrap_deg) {
            (Some(_), Some(wrap)) => format!("{wrap} deg"),
            _ => defaults.volute_wrap_deg.clone(),
        },
        "incidence_deg" => defaults.incidence_deg.clone(),
        _ => return None,
    })
}

/// Valor calculado de un parámetro `auto`, en la unidad de su especificación.
fn auto_text(result: &DesignResult, spec: &ParamSpec) -> Result<String, EngineError> {
    let si = designed_si(result, spec.name).ok_or_else(|| {
        EngineError::Design(DesignError::Input(format!(
            "el diseño no calcula `{}`",
            spec.name
        )))
    })?;
    if spec.unit == "1" {
        return Ok(si.to_string());
    }
    let unit = parse_unit(spec.unit).map_err(DesignError::from)?;
    let value = Quantity::from_si(si, unit).value();
    Ok(format!("{value} {}", spec.unit))
}

fn manual(value: String) -> Param {
    Param::Manual { value }
}

fn auto(formula: &str, value: String) -> Param {
    Param::Auto {
        formula: formula.to_string(),
        value: Some(value),
    }
}

/// Parámetros del método actual para `result`.
///
/// Los `auto` se escriben siempre con el valor recalculado. Los manuales y
/// bloqueados de `existing` se conservan; los que faltan se rellenan con el
/// valor que usó el motor. Con `existing`, la segunda lista son esos
/// rellenos; sin él (un proyecto nuevo) no se informan.
pub fn design_params(
    result: &DesignResult,
    existing: Option<&BTreeMap<String, Param>>,
    defaults: &ManualDefaults,
) -> Result<(BTreeMap<String, Param>, Vec<String>), EngineError> {
    let mut params = BTreeMap::new();
    let mut filled = Vec::new();

    for spec in PARAM_SPECS {
        let param = if let Some(formula) = spec.formula {
            if result.model_volute.is_none() && MODEL_VOLUTE_PARAMS.contains(&spec.name) {
                continue;
            }
            auto(formula, auto_text(result, spec)?)
        } else {
            match existing.and_then(|params| params.get(spec.name)) {
                Some(param @ (Param::Manual { .. } | Param::Locked { .. })) => param.clone(),
                Some(_) => {
                    return Err(input_error(
                        CaseField::Param(spec.name),
                        "HM-PROJ-018",
                        format!("`{}` debe ser manual o locked para recalcular", spec.name),
                    ));
                }
                None => {
                    let value =
                        default_manual_text(spec.name, result, defaults).ok_or_else(|| {
                            EngineError::Design(DesignError::Input(format!(
                                "sin valor por defecto para `{}`",
                                spec.name
                            )))
                        })?;
                    if existing.is_some() {
                        filled.push(spec.name.to_string());
                    }
                    manual(value)
                }
            }
        };
        params.insert(spec.name.to_string(), param);
    }

    Ok((params, filled))
}
