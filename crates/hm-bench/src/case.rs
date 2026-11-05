//! Lectura y validación del archivo del caso.

use hm_units::{Dimension, parse_quantity, parse_unit};
use serde::Deserialize;
use thiserror::Error;

/// Versión del formato que entiende este crate.
const SCHEMA: u32 = 1;

/// Fracción de Qd dentro de la cual una serie de NPSH cuenta como «en Qd».
const QD_SERIES_TOLERANCE: f64 = 0.02;

/// Series de NPSH que exige el nivel `bench`.
const MIN_NPSH_SERIES: usize = 3;

#[derive(Debug, Error, PartialEq)]
pub enum BenchError {
    #[error("el caso no se puede leer: {0}")]
    Toml(String),
    #[error("schema {0} no soportado; este programa lee el {SCHEMA}")]
    Schema(u32),
    #[error("{field}: {message}")]
    Field { field: String, message: String },
    #[error("el nivel `bench` exige: {}", .0.join("; "))]
    Incomplete(Vec<String>),
    #[error("cálculo: {0}")]
    Formula(String),
    #[error(
        "el modelo está a {model:.0} rpm y el caso a {case:.0} rpm: la comparación no tendría sentido"
    )]
    Speed { model: f64, case: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Trial,
    Bench,
}

/// Incertidumbre expandida declarada para un instrumento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Uncertainty {
    /// Fracción de lo leído.
    Relative(f64),
    /// En SI.
    Absolute(f64),
}

impl Uncertainty {
    /// Valor absoluto en SI para una lectura.
    pub fn absolute(self, reading: f64) -> f64 {
        match self {
            Self::Relative(fraction) => fraction * reading.abs(),
            Self::Absolute(value) => value,
        }
    }

    /// Fracción de una lectura; `None` si la lectura es cero y la incertidumbre
    /// es absoluta.
    pub fn relative(self, reading: f64) -> Option<f64> {
        match self {
            Self::Relative(fraction) => Some(fraction),
            Self::Absolute(value) => (reading != 0.0).then(|| value / reading.abs()),
        }
    }
}

/// Punto especificado, en SI.
#[derive(Debug, Clone, PartialEq)]
pub struct Specified {
    pub q: f64,
    pub h: f64,
    pub n: f64,
}

/// Montaje, en SI. Lo que falta solo se admite en un `trial`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rig {
    pub p_atm: Option<f64>,
    pub d_suction: Option<f64>,
    pub d_discharge: Option<f64>,
    pub z_suction: Option<f64>,
    pub z_discharge: Option<f64>,
    /// La succión se leyó en presión absoluta.
    pub suction_absolute: bool,
}

/// Incertidumbres que entran en la propagación.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Uncertainties {
    pub flow: Option<Uncertainty>,
    pub speed: Option<Uncertainty>,
    pub torque: Option<Uncertainty>,
    pub p_discharge: Option<Uncertainty>,
    pub p_suction: Option<Uncertainty>,
}

/// Una lectura de la curva, en SI y tal como se leyó.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub point: u32,
    pub n: Option<f64>,
    pub q: f64,
    pub p_discharge: f64,
    pub p_suction: f64,
    pub torque: Option<f64>,
    pub t_water: f64,
}

/// Un paso de una serie de NPSH, en SI y tal como se leyó.
#[derive(Debug, Clone, PartialEq)]
pub struct NpshReading {
    pub q_target: f64,
    pub step: u32,
    pub n: Option<f64>,
    pub q: f64,
    pub p_discharge: f64,
    pub p_suction: f64,
    pub t_water: f64,
}

/// Caso de banco ya validado.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchCase {
    pub id: String,
    pub level: Level,
    pub hmf: Option<String>,
    pub specified: Specified,
    pub rig: Rig,
    pub uncertainties: Uncertainties,
    pub curve: Vec<Reading>,
    pub npsh: Vec<NpshReading>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    schema: u32,
    id: String,
    pump: String,
    unit: String,
    month: String,
    level: Level,
    authorization: bool,
    hmf: Option<String>,
    specified: RawSpecified,
    rig: RawRig,
    #[serde(default)]
    seal: Option<RawSeal>,
    #[serde(default)]
    instruments: RawInstruments,
    #[serde(default)]
    curve: Vec<RawReading>,
    #[serde(default)]
    npsh: Vec<RawNpsh>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSpecified {
    #[serde(rename = "Q")]
    q: String,
    #[serde(rename = "H")]
    h: String,
    n: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRig {
    p_atm: Option<String>,
    #[serde(rename = "D_suction")]
    d_suction: Option<String>,
    #[serde(rename = "D_discharge")]
    d_discharge: Option<String>,
    z_suction: Option<String>,
    z_discharge: Option<String>,
    suction_reference: SuctionReference,
}

#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SuctionReference {
    Gauge,
    Absolute,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSeal {
    #[allow(dead_code)]
    kind: SealKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum SealKind {
    Packing,
    Mechanical,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawInstruments {
    flow: Option<RawInstrument>,
    speed: Option<RawInstrument>,
    torque: Option<RawInstrument>,
    p_discharge: Option<RawInstrument>,
    p_suction: Option<RawInstrument>,
    temperature: Option<RawInstrument>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInstrument {
    #[allow(dead_code)]
    kind: String,
    #[allow(dead_code)]
    range: Option<String>,
    u: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReading {
    point: u32,
    n: Option<String>,
    #[serde(rename = "Q")]
    q: String,
    p_discharge: String,
    p_suction: String,
    #[serde(rename = "T")]
    torque: Option<String>,
    t_water: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNpsh {
    #[serde(rename = "Q_target")]
    q_target: String,
    step: u32,
    n: Option<String>,
    #[serde(rename = "Q")]
    q: String,
    p_discharge: String,
    p_suction: String,
    t_water: String,
}

fn field_error(field: &str, message: impl Into<String>) -> BenchError {
    BenchError::Field {
        field: field.to_string(),
        message: message.into(),
    }
}

/// Valor en SI, con la dimensión de `unit`.
fn si(field: &str, text: &str, unit: &str) -> Result<f64, BenchError> {
    let quantity = parse_quantity(text).map_err(|error| field_error(field, error.to_string()))?;
    let expected = parse_unit(unit)
        .map_err(|error| field_error(field, error.to_string()))?
        .dim;
    if quantity.dim() != expected {
        return Err(field_error(field, format!("«{text}» no es de {unit}")));
    }
    if !quantity.si.is_finite() {
        return Err(field_error(field, format!("«{text}» no es finito")));
    }
    Ok(quantity.si)
}

fn optional_si(field: &str, text: Option<&String>, unit: &str) -> Result<Option<f64>, BenchError> {
    text.map(|text| si(field, text, unit)).transpose()
}

fn positive(field: &str, value: f64) -> Result<f64, BenchError> {
    if value > 0.0 {
        Ok(value)
    } else {
        Err(field_error(field, "tiene que ser mayor que cero"))
    }
}

fn not_negative(field: &str, value: f64) -> Result<f64, BenchError> {
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(field_error(field, "no puede ser negativo"))
    }
}

fn temperature(field: &str, text: &str) -> Result<f64, BenchError> {
    let quantity = parse_quantity(text).map_err(|error| field_error(field, error.to_string()))?;
    if quantity.dim() != Dimension::TEMPERATURE {
        return Err(field_error(
            field,
            format!("«{text}» no es una temperatura"),
        ));
    }
    Ok(quantity.si)
}

/// `"0.5 %"` o una cantidad con la unidad de `unit`.
fn uncertainty(
    field: &str,
    instrument: Option<&RawInstrument>,
    unit: &str,
) -> Result<Option<Uncertainty>, BenchError> {
    let Some(text) = instrument.and_then(|instrument| instrument.u.as_deref()) else {
        return Ok(None);
    };
    if let Some(percent) = text.trim().strip_suffix('%') {
        let value: f64 = percent
            .trim()
            .parse()
            .map_err(|_| field_error(field, format!("«{text}» no es un porcentaje")))?;
        return Ok(Some(Uncertainty::Relative(
            not_negative(field, value)? / 100.0,
        )));
    }
    let value = not_negative(field, si(field, text, unit)?)?;
    Ok(Some(Uncertainty::Absolute(value)))
}

/// Solo año y mes: la fecha exacta ayudaría a identificar la unidad.
fn check_month(month: &str) -> Result<(), BenchError> {
    let bytes = month.as_bytes();
    let valid = bytes.len() == 7
        && bytes[4] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || byte.is_ascii_digit());
    if valid {
        Ok(())
    } else {
        Err(field_error("month", "tiene que ser AAAA-MM"))
    }
}

impl BenchCase {
    /// Lee y valida un caso. Los campos desconocidos se rechazan: así un
    /// serial o un cliente no pueden colarse en un archivo que va a git.
    pub fn parse(text: &str) -> Result<Self, BenchError> {
        let raw: RawCase = toml::from_str(text).map_err(|e| BenchError::Toml(e.to_string()))?;
        if raw.schema != SCHEMA {
            return Err(BenchError::Schema(raw.schema));
        }
        check_month(&raw.month)?;
        if raw.pump.trim().is_empty() || raw.unit.trim().is_empty() {
            return Err(field_error("pump", "pump y unit no pueden estar vacíos"));
        }

        let specified = Specified {
            q: positive("specified.Q", si("specified.Q", &raw.specified.q, "m3/s")?)?,
            h: positive("specified.H", si("specified.H", &raw.specified.h, "m")?)?,
            n: positive("specified.n", si("specified.n", &raw.specified.n, "1/s")?)?,
        };
        let rig = Rig {
            p_atm: optional_si("rig.p_atm", raw.rig.p_atm.as_ref(), "Pa")?,
            d_suction: optional_si("rig.D_suction", raw.rig.d_suction.as_ref(), "m")?,
            d_discharge: optional_si("rig.D_discharge", raw.rig.d_discharge.as_ref(), "m")?,
            z_suction: optional_si("rig.z_suction", raw.rig.z_suction.as_ref(), "m")?,
            z_discharge: optional_si("rig.z_discharge", raw.rig.z_discharge.as_ref(), "m")?,
            suction_absolute: raw.rig.suction_reference == SuctionReference::Absolute,
        };
        for (field, diameter) in [
            ("rig.D_suction", rig.d_suction),
            ("rig.D_discharge", rig.d_discharge),
        ] {
            if let Some(diameter) = diameter {
                positive(field, diameter)?;
            }
        }

        let instruments = &raw.instruments;
        let uncertainties = Uncertainties {
            flow: uncertainty("instruments.flow", instruments.flow.as_ref(), "m3/s")?,
            speed: uncertainty("instruments.speed", instruments.speed.as_ref(), "1/s")?,
            torque: uncertainty("instruments.torque", instruments.torque.as_ref(), "N*m")?,
            p_discharge: uncertainty(
                "instruments.p_discharge",
                instruments.p_discharge.as_ref(),
                "Pa",
            )?,
            p_suction: uncertainty(
                "instruments.p_suction",
                instruments.p_suction.as_ref(),
                "Pa",
            )?,
        };

        let curve = raw
            .curve
            .iter()
            .map(parse_reading)
            .collect::<Result<Vec<_>, _>>()?;
        let npsh = raw
            .npsh
            .iter()
            .map(parse_npsh)
            .collect::<Result<Vec<_>, _>>()?;
        if curve.is_empty() {
            return Err(field_error("curve", "el caso no tiene lecturas"));
        }
        if rig.p_atm.is_none() && (rig.suction_absolute || !npsh.is_empty()) {
            return Err(field_error(
                "rig.p_atm",
                "hace falta con succión absoluta o con NPSH",
            ));
        }

        let case = Self {
            id: raw.id.clone(),
            level: raw.level,
            hmf: raw.hmf.clone(),
            specified,
            rig,
            uncertainties,
            curve,
            npsh,
        };
        if case.level == Level::Bench {
            let missing = case.missing_for_bench(&raw);
            if !missing.is_empty() {
                return Err(BenchError::Incomplete(missing));
            }
        }
        Ok(case)
    }

    fn missing_for_bench(&self, raw: &RawCase) -> Vec<String> {
        let mut missing = Vec::new();
        let mut need = |ok: bool, what: &str| {
            if !ok {
                missing.push(what.to_string());
            }
        };

        need(raw.authorization, "authorization = true");
        need(self.hmf.is_some(), "hmf con la geometría medida");
        need(raw.seal.is_some(), "seal");
        need(
            self.rig.d_suction.is_some() && self.rig.d_discharge.is_some(),
            "diámetros en las tomas",
        );
        need(
            self.rig.z_suction.is_some() && self.rig.z_discharge.is_some(),
            "alturas de las tomas",
        );
        need(
            self.curve
                .iter()
                .all(|reading| reading.n.is_some() && reading.torque.is_some()),
            "velocidad y par en cada lectura",
        );
        need(
            self.npsh.iter().all(|reading| reading.n.is_some()),
            "velocidad en cada paso de NPSH",
        );
        let u = &self.uncertainties;
        let temperature_declared = raw
            .instruments
            .temperature
            .as_ref()
            .is_some_and(|instrument| instrument.u.is_some());
        need(
            u.flow.is_some()
                && u.speed.is_some()
                && u.torque.is_some()
                && u.p_discharge.is_some()
                && u.p_suction.is_some()
                && temperature_declared,
            "incertidumbre de cada instrumento",
        );

        let mut targets: Vec<f64> = self.npsh.iter().map(|reading| reading.q_target).collect();
        targets.sort_by(f64::total_cmp);
        targets.dedup();
        need(
            targets.len() >= MIN_NPSH_SERIES,
            "tres series de NPSH como mínimo",
        );
        need(
            targets
                .iter()
                .any(|q| (q / self.specified.q - 1.0).abs() <= QD_SERIES_TOLERANCE),
            "una serie de NPSH en Qd",
        );
        missing
    }
}

fn parse_reading(raw: &RawReading) -> Result<Reading, BenchError> {
    let field = |name: &str| format!("curve[{}].{name}", raw.point);
    Ok(Reading {
        point: raw.point,
        n: optional_si(&field("n"), raw.n.as_ref(), "1/s")?
            .map(|n| positive(&field("n"), n))
            .transpose()?,
        q: not_negative(&field("Q"), si(&field("Q"), &raw.q, "m3/s")?)?,
        p_discharge: si(&field("p_discharge"), &raw.p_discharge, "Pa")?,
        p_suction: si(&field("p_suction"), &raw.p_suction, "Pa")?,
        torque: optional_si(&field("T"), raw.torque.as_ref(), "N*m")?
            .map(|torque| positive(&field("T"), torque))
            .transpose()?,
        t_water: temperature(&field("t_water"), &raw.t_water)?,
    })
}

fn parse_npsh(raw: &RawNpsh) -> Result<NpshReading, BenchError> {
    let field = |name: &str| format!("npsh[{}].{name}", raw.step);
    Ok(NpshReading {
        q_target: positive(
            &field("Q_target"),
            si(&field("Q_target"), &raw.q_target, "m3/s")?,
        )?,
        step: raw.step,
        n: optional_si(&field("n"), raw.n.as_ref(), "1/s")?
            .map(|n| positive(&field("n"), n))
            .transpose()?,
        q: positive(&field("Q"), si(&field("Q"), &raw.q, "m3/s")?)?,
        p_discharge: si(&field("p_discharge"), &raw.p_discharge, "Pa")?,
        p_suction: si(&field("p_suction"), &raw.p_suction, "Pa")?,
        t_water: temperature(&field("t_water"), &raw.t_water)?,
    })
}
