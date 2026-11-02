//! Estado de ingeniería de la interfaz, sin dependencia de egui.
//!
//! Reúne lo que la CLI muestra en `design`: el diseño, la curva, la
//! geometría, la mecánica, la corrección por viscosidad del agua y todos
//! los diagnósticos, para que la interfaz no diga "válido" donde la CLI
//! avisa.

use hm_core::Diagnostic;
use hm_design::fluid::water_kinematic_viscosity;
use hm_design::geometry::{Geometry, geometry};
use hm_design::mechanics::{
    DEFAULT_SHAFT_SHEAR_STRESS, Mechanics, mechanics, mechanics_diagnostics,
};
use hm_design::{
    CurvePoint, DesignInput, DesignResult, ViscousCorrection, default_mechanical_efficiency,
    design, performance_curve, viscous_correction, viscous_diagnostics, water_density,
};
use hm_units::{Quantity, parse_quantity};

/// Puntos de la curva que se dibujan.
pub const CURVE_POINTS: usize = 61;

/// Campos de entrada, tal como los escribe el usuario.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub q: String,
    pub h: String,
    pub n: String,
    /// Temperatura del agua en °C; fija ρ y ν.
    pub temperature_c: String,
    pub eta_m: String,
    /// NPSH disponible; vacío si no se conoce.
    pub npsha: String,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            q: "120 m3/h".into(),
            h: "45 m".into(),
            n: "2900 rpm".into(),
            temperature_c: "20".into(),
            eta_m: String::new(),
            npsha: "15 m".into(),
        }
    }
}

/// Campo de entrada, para señalar a cuál pertenece un error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Q,
    H,
    N,
    Temperature,
    EtaM,
    Npsha,
}

/// Errores de lectura por campo.
pub type FieldErrors = Vec<(Field, String)>;

/// Todo lo calculado para un punto de diseño.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub inputs: Inputs,
    pub input: DesignInput,
    pub result: DesignResult,
    pub curve: Vec<CurvePoint>,
    pub geometry: Option<Geometry>,
    /// Por qué no hay geometría, si no la hay.
    pub geometry_error: Option<String>,
    pub mechanics: Option<Mechanics>,
    pub viscous: Option<ViscousCorrection>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Outcome {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

fn quantity(field: Field, text: &str, errors: &mut FieldErrors) -> Option<Quantity> {
    match parse_quantity(text) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push((field, error.to_string()));
            None
        }
    }
}

/// Lee los campos. Devuelve la entrada del diseño y la temperatura en K, o
/// los errores de cada campo.
pub fn parse(inputs: &Inputs) -> Result<(DesignInput, f64), FieldErrors> {
    let mut errors = FieldErrors::new();
    let q = quantity(Field::Q, &inputs.q, &mut errors);
    let h = quantity(Field::H, &inputs.h, &mut errors);
    let n = quantity(Field::N, &inputs.n, &mut errors);

    let temperature_k = match inputs.temperature_c.trim().replace(',', ".").parse::<f64>() {
        Ok(celsius) => Some(celsius + 273.15),
        Err(_) => {
            errors.push((Field::Temperature, "temperatura en °C, p. ej. 20".into()));
            None
        }
    };
    let rho = temperature_k.and_then(|kelvin| match water_density(kelvin) {
        Some(rho) => Some(rho),
        None => {
            errors.push((
                Field::Temperature,
                "fuera del intervalo del agua (0..150 °C)".into(),
            ));
            None
        }
    });

    // Vacío: la estimación de Gülich para cojinetes y cierre (ec. 3.10.14).
    let eta_m = if inputs.eta_m.trim().is_empty() {
        match (&q, &n) {
            (Some(q), Some(n)) => default_mechanical_efficiency(q, n).ok(),
            _ => None,
        }
    } else {
        match inputs.eta_m.trim().replace(',', ".").parse::<f64>() {
            Ok(value) if value > 0.0 && value <= 1.0 => Some(value),
            _ => {
                errors.push((Field::EtaM, "número entre 0 y 1, o vacío".into()));
                None
            }
        }
    };

    let npsha = if inputs.npsha.trim().is_empty() {
        Some(None)
    } else {
        quantity(Field::Npsha, &inputs.npsha, &mut errors).map(Some)
    };

    match (q, h, n, rho, eta_m, npsha, temperature_k) {
        (Some(q), Some(h), Some(n), Some(rho), Some(eta_m), Some(npsha), Some(kelvin))
            if errors.is_empty() =>
        {
            let rho = parse_quantity(&format!("{rho} kg/m3")).expect("unidad fija");
            Ok((
                DesignInput {
                    q,
                    h,
                    n,
                    rho,
                    eta_m,
                    npsha,
                },
                kelvin,
            ))
        }
        _ => Err(errors),
    }
}

/// Diseña y reúne todo lo que se muestra. `Err` lleva el error de lectura
/// por campo o, en `Field::Q`, el error del motor.
pub fn compute(inputs: &Inputs) -> Result<Outcome, FieldErrors> {
    let (input, kelvin) = parse(inputs)?;
    let result = design(&input).map_err(|error| vec![(Field::Q, error.to_string())])?;
    let curve = performance_curve(&input, &result, CURVE_POINTS);

    let mut diagnostics = result.diagnostics.clone().into_vec();
    let (geometry, geometry_error) = match geometry(&input, &result) {
        Ok(shape) => (Some(shape), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let mechanics = mechanics(&input, &result, DEFAULT_SHAFT_SHEAR_STRESS).ok();
    if let Some(mech) = &mechanics {
        diagnostics.extend(mechanics_diagnostics(mech, &result).into_vec());
    }
    let viscous = water_kinematic_viscosity(kelvin)
        .and_then(|nu| viscous_correction(&input, &result, nu).ok());
    if let Some(correction) = &viscous {
        diagnostics.extend(viscous_diagnostics(correction, result.nq).into_vec());
    }

    Ok(Outcome {
        inputs: inputs.clone(),
        input,
        result,
        curve,
        geometry,
        geometry_error,
        mechanics,
        viscous,
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_point_designs_with_all_blocks() {
        let outcome = compute(&Inputs::default()).unwrap();
        assert_eq!(outcome.curve.len(), CURVE_POINTS);
        assert!(outcome.geometry.is_some());
        assert!(outcome.mechanics.is_some());
        assert!(outcome.viscous.is_some());
        assert!(!outcome.has_errors());
    }

    #[test]
    fn npsha_is_optional() {
        let inputs = Inputs {
            npsha: String::new(),
            ..Inputs::default()
        };
        let outcome = compute(&inputs).unwrap();
        assert!(outcome.input.npsha.is_none());
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "HM-DES-012")
        );
    }

    #[test]
    fn each_bad_field_is_reported() {
        let inputs = Inputs {
            q: "mucho".into(),
            eta_m: "2".into(),
            temperature_c: "400".into(),
            ..Inputs::default()
        };
        let errors = compute(&inputs).unwrap_err();
        let fields: Vec<Field> = errors.iter().map(|(field, _)| *field).collect();
        assert!(fields.contains(&Field::Q));
        assert!(fields.contains(&Field::EtaM));
        assert!(fields.contains(&Field::Temperature));
    }

    #[test]
    fn mechanical_warnings_reach_the_interface() {
        // Un motor por encima de la serie IEC (HM-DES-030). Hasta la v0.6 la
        // prueba usaba el cubo grueso de 10 m³/h y 120 m; con el ηh de Gülich
        // (v0.7) ese punto de nq ≈ 4 ya no tiene solución por defecto.
        let inputs = Inputs {
            q: "3000 m3/h".into(),
            h: "150 m".into(),
            n: "1450 rpm".into(),
            ..Inputs::default()
        };
        let outcome = compute(&inputs).unwrap();
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "HM-DES-030")
        );
    }

    #[test]
    fn density_follows_temperature() {
        let hot = compute(&Inputs {
            temperature_c: "80".into(),
            ..Inputs::default()
        })
        .unwrap();
        let cold = compute(&Inputs::default()).unwrap();
        assert!(hot.input.rho.si < cold.input.rho.si);
    }
}
