//! Estado de ingeniería de la interfaz, sin dependencia de egui.
//!
//! Arma un caso de agua desde las entradas y lo pasa al motor. La interfaz
//! muestra el `Analysis`: diseño, curva, etapas y diagnósticos en el orden
//! canónico.

use hm_core::Diagnostic;
use hm_design::geometry::Geometry;
use hm_design::mechanics::Mechanics;
use hm_design::{CurvePoint, DesignError, DesignInput, DesignResult, ViscousCorrection};
use hm_engine::{
    Case, CaseField, CurvePoints, Density, DensitySource, DesignOptions, Duty, EngineError,
    Entered, Fluid, FluidKind, Intake, PumpType, Temperature, analyze, water_density_at,
};
use hm_units::Quantity;

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
///
/// `General` no tiene caja: es un fallo de etapa o un error del motor sin
/// campo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Q,
    H,
    N,
    Temperature,
    EtaM,
    Npsha,
    General,
}

/// Errores de lectura por campo.
pub type FieldErrors = Vec<(Field, String)>;

/// Todo lo calculado para un punto de diseño.
///
/// Geometría, mecánica y viscosidad guardan su fallo. `Ok(None)` en la
/// viscosidad es éxito: no había corrección que aplicar.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub inputs: Inputs,
    pub input: DesignInput,
    pub result: DesignResult,
    pub curve: Vec<CurvePoint>,
    pub geometry: Result<Geometry, DesignError>,
    pub mechanics: Result<Mechanics, DesignError>,
    pub viscous: Result<Option<ViscousCorrection>, DesignError>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Outcome {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }
}

/// Caso de agua que describe `inputs`. Los textos de caudal, altura, giro y
/// NPSHa pasan por `Entered::parse`. La temperatura es un número en °C: fija
/// el fluido y la densidad sale de `water_density_at`.
fn case_from_inputs(inputs: &Inputs) -> Result<Case, FieldErrors> {
    let mut errors = FieldErrors::new();
    let q = entered(Field::Q, &inputs.q, &mut errors);
    let h = entered(Field::H, &inputs.h, &mut errors);
    let n = entered(Field::N, &inputs.n, &mut errors);
    let temperature = read_temperature(inputs, &mut errors);
    let eta_m = read_eta_m(inputs, &mut errors);
    let intake = read_intake(inputs, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    let (temperature, rho) = temperature.expect("sin errores de temperatura");
    Ok(Case {
        pump_type: PumpType::EndSuction,
        duty: Duty {
            q: q.expect("sin errores de caudal"),
            h: h.expect("sin errores de altura"),
            n: n.expect("sin errores de velocidad"),
        },
        fluid: Fluid {
            kind: FluidKind::Water,
            temperature: Some(temperature),
            density: Density {
                value: rho,
                source: DensitySource::WaterAtTemperature,
            },
            viscosity: None,
        },
        intake: intake.expect("sin errores de NPSHa"),
        mechanical_efficiency: eta_m.expect("sin errores de ηm"),
        design: DesignOptions::default(),
    })
}

fn entered(field: Field, text: &str, errors: &mut FieldErrors) -> Option<Entered> {
    match Entered::parse(text) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push((field, error.to_string()));
            None
        }
    }
}

fn read_temperature(inputs: &Inputs, errors: &mut FieldErrors) -> Option<(Temperature, Quantity)> {
    let celsius = match inputs.temperature_c.trim().replace(',', ".").parse::<f64>() {
        Ok(celsius) => celsius,
        Err(_) => {
            errors.push((Field::Temperature, "temperatura en °C, p. ej. 20".into()));
            return None;
        }
    };
    let temperature = Temperature {
        text: inputs.temperature_c.clone(),
        kelvin: celsius + 273.15,
    };
    match water_density_at(&temperature) {
        Some(rho) => Some((temperature, rho)),
        None => {
            errors.push((
                Field::Temperature,
                "fuera del intervalo del agua (0..150 °C)".into(),
            ));
            None
        }
    }
}

/// Vacío: la estimación del motor. Un número tiene que estar entre 0 y 1,
/// sin incluir los extremos.
fn read_eta_m(inputs: &Inputs, errors: &mut FieldErrors) -> Option<Option<f64>> {
    if inputs.eta_m.trim().is_empty() {
        return Some(None);
    }
    match inputs.eta_m.trim().replace(',', ".").parse::<f64>() {
        Ok(value) if value > 0.0 && value < 1.0 => Some(Some(value)),
        _ => {
            errors.push((Field::EtaM, "número entre 0 y 1, o vacío".into()));
            None
        }
    }
}

fn read_intake(inputs: &Inputs, errors: &mut FieldErrors) -> Option<Intake> {
    if inputs.npsha.trim().is_empty() {
        Some(Intake::None)
    } else {
        entered(Field::Npsha, &inputs.npsha, errors).map(Intake::Available)
    }
}

/// Diseña y reúne todo lo que se muestra.
///
/// `Err` son errores de lectura o del motor, cada uno en su campo. Un fallo
/// de etapa no descarta el resto: queda en el `Result` de `Outcome`.
pub fn compute(inputs: &Inputs) -> Result<Outcome, FieldErrors> {
    let case = case_from_inputs(inputs)?;
    let points = CurvePoints::new(CURVE_POINTS).expect("61 está entre 2 y 1000");
    let analysis = analyze(&case, points).map_err(|error| vec![engine_error_field(error)])?;
    Ok(Outcome {
        inputs: inputs.clone(),
        input: analysis.input,
        result: analysis.result,
        curve: analysis.curve,
        geometry: analysis.geometry,
        mechanics: analysis.mechanics,
        viscous: analysis.viscous,
        diagnostics: analysis.diagnostics,
    })
}

/// Campo y frase de un error del motor. Los de etapa y los que no tienen
/// campo van a `Field::General`, con la frase de la causa y no la envoltura.
fn engine_error_field(error: EngineError) -> (Field, String) {
    let field = match error.field() {
        Some(CaseField::Q) => Field::Q,
        Some(CaseField::H) => Field::H,
        Some(CaseField::N) => Field::N,
        Some(CaseField::Temperature | CaseField::Density) => Field::Temperature,
        Some(CaseField::MechanicalEfficiency) => Field::EtaM,
        Some(CaseField::Npsha | CaseField::Suction) => Field::Npsha,
        Some(CaseField::Viscosity | CaseField::Param(_)) | None => Field::General,
    };
    let message = match error {
        EngineError::Input { diagnostic, .. } => diagnostic.message,
        EngineError::Stage { source, .. } => source.to_string(),
        other => other.to_string(),
    };
    (field, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    fn warnings(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == hm_core::Severity::Warning)
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    #[test]
    fn default_point_designs_with_all_blocks() {
        let outcome = compute(&Inputs::default()).unwrap();
        assert_eq!(outcome.curve.len(), CURVE_POINTS);
        assert!(outcome.geometry.is_ok());
        assert!(outcome.mechanics.is_ok());
        assert!(matches!(outcome.viscous, Ok(Some(_))));
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

    #[test]
    fn engine_errors_land_on_their_own_field() {
        let errors = compute(&Inputs {
            h: "-1 m".into(),
            ..Inputs::default()
        })
        .unwrap_err();
        assert!(
            errors.iter().any(|(field, message)| {
                *field == Field::H && message.contains("H debe ser finito y positivo")
            }),
            "{errors:?}"
        );
        assert!(
            errors.iter().all(|(field, _)| *field != Field::Q),
            "{errors:?}"
        );
    }

    #[test]
    fn out_of_range_temperature_lands_on_the_temperature_field() {
        let errors = compute(&Inputs {
            temperature_c: "200".into(),
            ..Inputs::default()
        })
        .unwrap_err();
        assert!(
            errors.iter().any(|(field, message)| {
                *field == Field::Temperature
                    && message == "fuera del intervalo del agua (0..150 °C)"
            }),
            "{errors:?}"
        );
        assert!(
            errors.iter().all(|(field, _)| *field != Field::Q),
            "{errors:?}"
        );
    }

    #[test]
    fn stage_errors_are_shown_as_general() {
        // El diseño base no sale: el error no tiene campo.
        let errors = compute(&Inputs {
            q: "5 m3/h".into(),
            h: "60 m".into(),
            ..Inputs::default()
        })
        .unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].0, Field::General);
        assert!(errors[0].1.contains("ηh"), "{}", errors[0].1);

        let (field, message) = engine_error_field(EngineError::Stage {
            stage: hm_engine::Stage::Mechanics,
            source: DesignError::Input("el eje no sale".into()),
        });
        assert_eq!(field, Field::General);
        assert!(message.contains("el eje no sale"), "{message}");
        assert!(!message.contains("falló la etapa"), "{message}");
    }

    #[test]
    fn diagnostics_follow_the_canonical_order() {
        let inputs = Inputs {
            q: "3000 m3/h".into(),
            h: "150 m".into(),
            n: "1450 rpm".into(),
            ..Inputs::default()
        };
        let outcome = compute(&inputs).unwrap();
        let case = case_from_inputs(&inputs).unwrap();
        let analysis = analyze(&case, CurvePoints::new(CURVE_POINTS).unwrap()).unwrap();
        assert_eq!(codes(&outcome.diagnostics), codes(&analysis.diagnostics));
        let mechanics = codes(&outcome.diagnostics)
            .iter()
            .position(|code| *code == "HM-DES-030")
            .expect("el aviso mecánico está");
        let viscous = codes(&outcome.diagnostics)
            .iter()
            .position(|code| matches!(*code, "HM-DES-020" | "HM-DES-021" | "HM-DES-022"));
        if let Some(viscous) = viscous {
            assert!(
                mechanics > viscous,
                "la mecánica queda después de la viscosidad: {:?}",
                codes(&outcome.diagnostics)
            );
        }
    }

    #[test]
    fn compute_agrees_with_the_engine() {
        let inputs = Inputs::default();
        let outcome = compute(&inputs).unwrap();
        let case = case_from_inputs(&inputs).unwrap();
        let analysis = analyze(&case, CurvePoints::new(CURVE_POINTS).unwrap()).unwrap();
        assert_eq!(
            format!("{:?}", outcome.result),
            format!("{:?}", analysis.result)
        );
        assert_eq!(
            format!("{:?}", outcome.curve),
            format!("{:?}", analysis.curve)
        );
        assert_eq!(
            warnings(&outcome.diagnostics),
            warnings(&analysis.diagnostics)
        );
    }
}
