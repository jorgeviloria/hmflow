//! Estado de ingeniería de la interfaz, sin dependencia de egui.
//!
//! Arma un caso de agua desde las entradas y lo pasa al motor. La interfaz
//! muestra el `Analysis`: diseño, curva, etapas y diagnósticos en el orden
//! canónico. Abrir un archivo es `Project::load` y `open_project`: aquí no se
//! interpreta el esquema.

use std::path::Path;

use hm_core::{Diagnostic, Diagnostics};
use hm_design::geometry::Geometry;
use hm_design::mechanics::Mechanics;
use hm_design::{CurvePoint, DesignError, DesignInput, DesignResult, ViscousCorrection};
use hm_engine::{
    Analysis, Case, CaseField, CurvePoints, Density, DensitySource, DesignOptions, Duty,
    EditorError, EngineError, Entered, Fluid, FluidKind, Intake, Opened, PumpType, Temperature,
    analyze, cli_message, new_project, open_project, recalc, update_point, water_density_at,
};
use hm_project::Project;
use hm_units::Quantity;

/// Puntos de la curva que se dibujan.
pub const CURVE_POINTS: usize = 61;

/// De dónde sale el NPSHa: un valor escrito o la instalación de aspiración.
/// Son excluyentes, como en el `.hmf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntakeMode {
    #[default]
    Written,
    Installation,
}

/// Campos de entrada, tal como los escribe el usuario.
#[derive(Debug, Clone, PartialEq)]
pub struct Inputs {
    pub q: String,
    pub h: String,
    pub n: String,
    /// Temperatura del agua en °C; fija ρ y ν.
    pub temperature_c: String,
    pub eta_m: String,
    /// NPSH disponible; vacío si no se conoce. Solo cuenta con `Written`.
    pub npsha: String,
    pub intake: IntakeMode,
    /// Instalación de aspiración. Solo cuentan con `Installation`. La presión
    /// es obligatoria; los otros tres pueden quedar vacíos (vacío no es cero).
    pub suction_pressure: String,
    pub suction_height: String,
    pub suction_losses: String,
    pub suction_vapor: String,
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
            intake: IntakeMode::Written,
            suction_pressure: "1 atm".into(),
            suction_height: String::new(),
            suction_losses: String::new(),
            suction_vapor: String::new(),
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
    /// El grupo de las cuatro cajas de la instalación de aspiración.
    Suction,
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
    /// Hay errores si un diagnóstico lo es o si una etapa no salió: la CLI
    /// termina con error en ambos casos y la interfaz no debe decir «válido».
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
            || self.geometry.is_err()
            || self.mechanics.is_err()
            || self.viscous.is_err()
    }
}

/// Caso de agua que describe `inputs`. Los textos de caudal, altura, giro y
/// NPSHa pasan por `Entered::parse`. La temperatura es un número en °C: fija
/// el fluido y la densidad sale de `water_density_at`.
fn case_from_inputs(inputs: &Inputs, basis: &Basis) -> Result<Case, FieldErrors> {
    let mut errors = FieldErrors::new();
    let q = entered(Field::Q, &inputs.q, &mut errors);
    let h = entered(Field::H, &inputs.h, &mut errors);
    let n = entered(Field::N, &inputs.n, &mut errors);
    let temperature = read_temperature(inputs, &mut errors);
    let eta_m = read_eta_m(inputs, &mut errors);
    let intake = read_intake(inputs, &mut errors);

    // Los `Option` solo son `None` si hay un error en `errors`.
    let (Some(q), Some(h), Some(n), Some((temperature, rho)), Some(eta_m), Some(intake)) =
        (q, h, n, temperature, eta_m, intake)
    else {
        return Err(errors);
    };

    Ok(Case {
        pump_type: PumpType::EndSuction,
        duty: Duty { q, h, n },
        fluid: Fluid {
            kind: FluidKind::Water,
            temperature: Some(temperature),
            density: basis.density_for(inputs).unwrap_or(Density {
                value: rho,
                source: DensitySource::WaterAtTemperature,
            }),
            viscosity: None,
        },
        intake,
        mechanical_efficiency: eta_m,
        design: basis.design.clone(),
    })
}

fn entered(field: Field, text: &str, errors: &mut FieldErrors) -> Option<Entered> {
    entered_as(field, "", text, errors)
}

/// Como `entered`, pero el error empieza por `prefix` (para las cajas que
/// comparten campo y hay que distinguirlas).
fn entered_as(field: Field, prefix: &str, text: &str, errors: &mut FieldErrors) -> Option<Entered> {
    match Entered::parse(text) {
        Ok(value) => Some(value),
        Err(error) => {
            errors.push((field, format!("{prefix}{error}")));
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
    // El texto lleva su unidad, como el que lee `parse_temperature`.
    let temperature = Temperature {
        text: format!("{} degC", inputs.temperature_c.trim().replace(',', ".")),
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
    match inputs.intake {
        IntakeMode::Written => {
            if inputs.npsha.trim().is_empty() {
                Some(Intake::None)
            } else {
                entered(Field::Npsha, &inputs.npsha, errors).map(Intake::Available)
            }
        }
        IntakeMode::Installation => read_installation(inputs, errors),
    }
}

/// Las cuatro cajas de la instalación: su nombre en el proyecto y la palabra
/// con que empiezan sus errores (`presión: ...`), que es lo que une un error
/// con su caja.
pub const SUCTION_BOXES: [(&str, &str); 4] = [
    ("pressure", "presión"),
    ("height", "altura"),
    ("losses", "pérdidas"),
    ("vapor_pressure", "presión de vapor"),
];

fn suction_prefix(name: &str) -> String {
    let label = SUCTION_BOXES
        .iter()
        .find(|(key, _)| *key == name)
        .map_or(name, |(_, label)| label);
    format!("{label}: ")
}

/// El error de una caja de la instalación (sin su encabezado), si lo hay.
pub fn suction_box_error<'a>(errors: &'a FieldErrors, label: &str) -> Option<&'a str> {
    let prefix = format!("{label}: ");
    errors
        .iter()
        .filter(|(field, _)| *field == Field::Suction)
        .find_map(|(_, message)| message.strip_prefix(&prefix))
}

/// Los errores de la instalación que no son de ninguna caja concreta.
pub fn suction_group_errors(errors: &FieldErrors) -> impl Iterator<Item = &str> {
    errors
        .iter()
        .filter(|(field, _)| *field == Field::Suction)
        .map(|(_, message)| message.as_str())
        .filter(|message| {
            !SUCTION_BOXES
                .iter()
                .any(|(_, label)| message.starts_with(&format!("{label}: ")))
        })
}

/// La presión es obligatoria; altura, pérdidas y presión de vapor vacías
/// quedan en `None` (no en `0 m`). Cada error empieza por el nombre de su caja.
fn read_installation(inputs: &Inputs, errors: &mut FieldErrors) -> Option<Intake> {
    let before = errors.len();
    let pressure = if inputs.suction_pressure.trim().is_empty() {
        errors.push((
            Field::Suction,
            format!(
                "{}es obligatoria en la instalación",
                suction_prefix("pressure")
            ),
        ));
        None
    } else {
        entered_as(
            Field::Suction,
            &suction_prefix("pressure"),
            &inputs.suction_pressure,
            errors,
        )
    };
    let height = optional_suction("height", &inputs.suction_height, errors);
    let losses = optional_suction("losses", &inputs.suction_losses, errors);
    let vapor_pressure = optional_suction("vapor_pressure", &inputs.suction_vapor, errors);
    if errors.len() > before {
        return None;
    }
    Some(Intake::Installation {
        pressure: pressure?,
        height,
        losses,
        vapor_pressure,
    })
}

fn optional_suction(name: &str, text: &str, errors: &mut FieldErrors) -> Option<Entered> {
    if text.trim().is_empty() {
        None
    } else {
        entered_as(Field::Suction, &suction_prefix(name), text, errors)
    }
}

/// Diseña y reúne todo lo que se muestra.
///
/// `Err` son errores de lectura o del motor, cada uno en su campo. Un fallo
/// de etapa no descarta el resto: queda en el `Result` de `Outcome`.
pub fn compute(inputs: &Inputs, basis: &Basis) -> Result<Outcome, FieldErrors> {
    let case = case_from_inputs(inputs, basis)?;
    let analysis = analyze(&case, CurvePoints::clamped(CURVE_POINTS))
        .map_err(|error| vec![engine_error_field(error, !inputs.eta_m.trim().is_empty())])?;
    Ok(outcome_from(inputs, analysis))
}

/// El análisis tal cual: la interfaz no reordena ni descarta nada.
fn outcome_from(inputs: &Inputs, analysis: Analysis) -> Outcome {
    Outcome {
        inputs: inputs.clone(),
        input: analysis.input,
        result: analysis.result,
        curve: analysis.curve,
        geometry: analysis.geometry,
        mechanics: analysis.mechanics,
        viscous: analysis.viscous,
        diagnostics: analysis.diagnostics,
    }
}

/// Campo y frase de un error del motor. Los que no tienen campo van a
/// `Field::General`. Si ηm está vacío, un error de ηm es de la estimación del
/// motor y no de una caja que el usuario no tocó: también va a `General`.
fn engine_error_field(error: EngineError, eta_m_typed: bool) -> (Field, String) {
    let field = match error.field() {
        Some(CaseField::Q) => Field::Q,
        Some(CaseField::H) => Field::H,
        Some(CaseField::N) => Field::N,
        Some(CaseField::Temperature | CaseField::Density) => Field::Temperature,
        Some(CaseField::MechanicalEfficiency) if eta_m_typed => Field::EtaM,
        Some(CaseField::Npsha) => Field::Npsha,
        Some(CaseField::Suction) => Field::Suction,
        Some(CaseField::MechanicalEfficiency | CaseField::Viscosity | CaseField::Param(_))
        | None => Field::General,
    };
    // La caja ya dice de qué campo se trata: sin el «entrada inválida: » de la CLI.
    let message = cli_message(&error);
    let message = message
        .strip_prefix("entrada inválida: ")
        .map_or_else(|| message.clone(), str::to_string);
    let message = if field == Field::Suction {
        // «[suction] `pressure`: ...» del motor -> «presión: ...», como las cajas.
        SUCTION_BOXES.iter().fold(message, |message, (name, _)| {
            message.replace(&format!("[suction] `{name}`: "), &suction_prefix(name))
        })
    } else {
        message
    };
    (field, message)
}

/// Lo que el archivo abierto fija y las cajas no muestran.
///
/// Sin esto, la ventana calcularía otra bomba que la del archivo: las
/// selecciones manuales valen hasta que se cierre el proyecto.
#[derive(Debug, Clone, Default)]
pub struct Basis {
    pub design: DesignOptions,
    /// Densidad escrita en el archivo y la caja de temperatura con que se
    /// abrió. Solo vale mientras esa caja no cambie, salvo si está bloqueada.
    pub density: Option<(String, Density)>,
    pub rho_locked: bool,
    /// El texto de una ηm bloqueada: al guardar, la caja no la pisa.
    pub locked_eta_m: Option<String>,
}

impl Basis {
    fn from_opened(opened: &Opened, inputs: &Inputs) -> Self {
        let density = &opened.case.fluid.density;
        let written = matches!(
            &density.source,
            DensitySource::Explicit { text } if !text.trim().is_empty()
        );
        Self {
            design: opened.case.design.clone(),
            density: written.then(|| (inputs.temperature_c.clone(), density.clone())),
            rho_locked: opened.rho_locked,
            locked_eta_m: if opened.eta_m_locked {
                opened.eta_m_text.clone()
            } else {
                None
            },
        }
    }

    fn density_for(&self, inputs: &Inputs) -> Option<Density> {
        self.density
            .as_ref()
            .filter(|(box_text, _)| {
                self.rho_locked || box_text.trim() == inputs.temperature_c.trim()
            })
            .map(|(_, density)| density.clone())
    }
}

/// Un proyecto que la ventana ya puede mostrar.
#[derive(Debug)]
pub struct OpenedFile {
    pub project: Project,
    pub inputs: Inputs,
    pub basis: Basis,
    pub diagnostics: Vec<Diagnostic>,
}

/// Cajas a partir de un caso.
///
/// Q, H, n, el NPSHa y la aspiración copian el texto guardado. La temperatura
/// queda en °C: si el texto ya es un número que la caja acepta y coincide con
/// los grados, se conserva; `80 degC` pasa a `80`, sin ceros de más. ηm es el
/// número del caso, o vacío si no hay: el texto del parámetro lo pone
/// `open_file`, que recibe ese texto del motor.
pub fn case_to_inputs(case: &Case) -> Inputs {
    // Sin instalación en el archivo, la caja arranca como en un diseño nuevo:
    // pasar a «instalación» no debe abrir con un error.
    let default_pressure = Inputs::default().suction_pressure;
    let (intake, npsha, suction_pressure, suction_height, suction_losses, suction_vapor) =
        match &case.intake {
            Intake::Available(npsha) => (
                IntakeMode::Written,
                npsha.text.clone(),
                default_pressure,
                String::new(),
                String::new(),
                String::new(),
            ),
            Intake::None => (
                IntakeMode::Written,
                String::new(),
                default_pressure,
                String::new(),
                String::new(),
                String::new(),
            ),
            Intake::Installation {
                pressure,
                height,
                losses,
                vapor_pressure,
            } => (
                IntakeMode::Installation,
                String::new(),
                pressure.text.clone(),
                optional_text(height),
                optional_text(losses),
                optional_text(vapor_pressure),
            ),
        };

    Inputs {
        q: case.duty.q.text.clone(),
        h: case.duty.h.text.clone(),
        n: case.duty.n.text.clone(),
        temperature_c: temperature_box(case.fluid.temperature.as_ref()),
        eta_m: case
            .mechanical_efficiency
            .map(|value| value.to_string())
            .unwrap_or_default(),
        npsha,
        intake,
        suction_pressure,
        suction_height,
        suction_losses,
        suction_vapor,
    }
}

/// Abre un `.hmf` para la ventana.
///
/// El rechazo es la frase de `Unsupported` o los diagnósticos, un aviso por
/// línea. No escribe nada.
pub fn open_file(path: &Path) -> Result<OpenedFile, String> {
    open_loaded(Project::load(path).map_err(|error| error.to_string())?)
}

/// Lo mismo con un proyecto en memoria: las cajas de lo recién guardado salen
/// por aquí, igual que al abrirlo.
pub fn open_loaded(project: Project) -> Result<OpenedFile, String> {
    let opened = match open_project(&project) {
        Ok(opened) => opened,
        Err(EditorError::Unsupported { reason }) => return Err(reason),
        Err(EditorError::Invalid { diagnostics }) => return Err(format_diagnostics(&diagnostics)),
    };
    let mut inputs = case_to_inputs(&opened.case);
    if let Some(text) = &opened.eta_m_text {
        inputs.eta_m.clone_from(text);
    }
    let basis = Basis::from_opened(&opened, &inputs);
    Ok(OpenedFile {
        project,
        inputs,
        basis,
        diagnostics: opened.diagnostics.into_vec(),
    })
}

fn optional_text(entered: &Option<Entered>) -> String {
    entered
        .as_ref()
        .map(|entered| entered.text.clone())
        .unwrap_or_default()
}

/// Número en °C, o el texto si la caja ya lo acepta y da los mismos grados.
///
/// Sin temperatura el motor usa 20 °C: la caja no puede quedar vacía.
fn temperature_box(temperature: Option<&Temperature>) -> String {
    let Some(temperature) = temperature else {
        return "20".to_string();
    };
    let celsius = temperature.kelvin - 273.15;
    let trimmed = temperature.text.trim();
    let parsed = trimmed.replace(',', ".");
    if parsed
        .parse::<f64>()
        .is_ok_and(|value| (value - celsius).abs() <= 1e-4)
    {
        return trimmed.to_string();
    }
    format_celsius(celsius)
}

/// Grados sin ceros de más: `80`, no `80.0`.
fn format_celsius(celsius: f64) -> String {
    let rounded = (celsius * 1000.0).round() / 1000.0;
    if (rounded - rounded.round()).abs() < 1e-9 {
        return format!("{}", rounded.round() as i64);
    }
    format!("{rounded:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// Proyecto listo para escribir, con los avisos que debe mostrar el panel.
///
/// En uno nuevo, los de `compute`. En uno que ya existía, los de `recalc`
/// (ahí está `HM-DES-013`).
#[derive(Debug)]
pub struct Built {
    pub project: Project,
    pub diagnostics: Vec<Diagnostic>,
}

/// Arma el proyecto de estas cajas.
///
/// Es el mismo caso que `compute`, y por eso lo guardado es lo que se ve. Sin
/// proyecto previo, `new_project`. Con proyecto, `update_point` y `recalc` sin
/// reelegir. No escribe el archivo. Una `rho` bloqueada ya está en `basis`: si
/// la temperatura no es la de esa densidad, `recalc` deja `HM-DES-009`. Una ηm
/// bloqueada no se deja pisar: la caja vuelve a su texto.
pub fn build_project(
    inputs: &Inputs,
    basis: &Basis,
    previous: Option<&Project>,
    name: &str,
) -> Result<Built, String> {
    let inputs = match &basis.locked_eta_m {
        Some(text) => Inputs {
            eta_m: text.clone(),
            ..inputs.clone()
        },
        None => inputs.clone(),
    };
    let case = case_from_inputs(&inputs, basis).map_err(|errors| field_failure(&errors))?;
    let analysis = analyze(&case, CurvePoints::clamped(CURVE_POINTS))
        .map_err(|error| engine_failure(&error))?;
    if let Some(message) = stage_failure(&analysis) {
        return Err(message);
    }
    match previous {
        None => {
            let project =
                new_project(name, &case, &analysis).map_err(|error| engine_failure(&error))?;
            Ok(Built {
                project,
                diagnostics: analysis.diagnostics,
            })
        }
        Some(project) => {
            let project = update_point(project.clone(), &case, &analysis)
                .map_err(|error| engine_failure(&error))?;
            let recalculated = recalc(project, false).map_err(|error| engine_failure(&error))?;
            Ok(Built {
                project: recalculated.project,
                diagnostics: recalculated.diagnostics.into_vec(),
            })
        }
    }
}

fn field_failure(errors: &FieldErrors) -> String {
    if errors.is_empty() {
        return "hay errores en las cajas".to_string();
    }
    errors
        .iter()
        .map(|(_, message)| message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn stage_failure(analysis: &Analysis) -> Option<String> {
    if let Err(error) = &analysis.geometry {
        return Some(error.to_string());
    }
    if let Err(error) = &analysis.mechanics {
        return Some(error.to_string());
    }
    if let Err(error) = &analysis.viscous {
        return Some(error.to_string());
    }
    None
}

fn engine_failure(error: &EngineError) -> String {
    match error {
        EngineError::Invalid { diagnostics }
        | EngineError::DesignHasErrors { diagnostics }
        | EngineError::RecalculatedInvalid { diagnostics } => format_diagnostics(diagnostics),
        EngineError::AfterDesign { design, source } => {
            format!("{}\n{}", format_diagnostics(design), engine_failure(source))
        }
        _ => cli_message(error),
    }
}

/// Como el panel: código, mensaje y, si la hay, la sugerencia.
fn format_diagnostics(diagnostics: &Diagnostics) -> String {
    let text = diagnostics
        .iter()
        .map(|diagnostic| {
            let mut line = format!("{}  {}", diagnostic.code, diagnostic.message);
            if let Some(hint) = &diagnostic.hint {
                line.push_str(&format!("\n    → {hint}"));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        "el proyecto tiene errores".to_string()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hm_project::Param;

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
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
    fn base_design_errors_are_shown_as_general() {
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
    }

    #[test]
    fn an_estimated_eta_m_error_is_not_blamed_on_an_empty_eta_m_box() {
        // Prueba el reparto de campos con un error del motor construido a mano:
        // no hay una entrada de la interfaz que haga fallar la estimación.
        let error = |typed| {
            engine_error_field(
                EngineError::Input {
                    field: CaseField::MechanicalEfficiency,
                    diagnostic: Diagnostic::error(
                        "HM-PROJ-020",
                        "entrada inválida: la eficiencia mecánica debe ser mayor que 0",
                    ),
                },
                typed,
            )
        };
        // Vacío: la estimó el motor, el usuario no tocó la caja.
        assert_eq!(error(false).0, Field::General);
        // Escrito: es de esa caja.
        let (field, message) = error(true);
        assert_eq!(field, Field::EtaM);
        assert!(message.starts_with("la eficiencia mecánica"), "{message}");
    }

    #[test]
    fn the_temperature_text_keeps_its_unit() {
        let case = case_from_inputs(&Inputs {
            temperature_c: "80,5".into(),
            ..Inputs::default()
        })
        .unwrap();
        let temperature = case.fluid.temperature.unwrap();
        assert_eq!(temperature.text, "80.5 degC");
        // El texto se puede volver a leer y da la misma temperatura.
        let again = hm_engine::parse_temperature(&temperature.text).unwrap();
        assert!((again.kelvin - temperature.kelvin).abs() < 1e-9);
    }

    #[test]
    fn diagnostics_follow_the_canonical_order() {
        // El agua de la interfaz no dispara avisos de viscosidad; con un
        // aceite sí, y la mecánica tiene que quedar después de ella.
        let inputs = Inputs {
            q: "3000 m3/h".into(),
            h: "150 m".into(),
            n: "1450 rpm".into(),
            ..Inputs::default()
        };
        let mut case = case_from_inputs(&inputs).unwrap();
        case.fluid.viscosity = Some(hm_engine::Viscosity::Kinematic(
            Entered::parse("300 cSt").unwrap(),
        ));
        let analysis = analyze(&case, CurvePoints::new(CURVE_POINTS).unwrap()).unwrap();

        let from_engine: Vec<String> = analysis
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.clone())
            .collect();
        let outcome = outcome_from(&inputs, analysis);
        let shown = codes(&outcome.diagnostics);
        // La interfaz no reordena lo que da el motor.
        assert_eq!(shown, from_engine);

        let viscous = shown
            .iter()
            .position(|code| matches!(*code, "HM-DES-020" | "HM-DES-021" | "HM-DES-022"))
            .expect("el aceite dispara un aviso de viscosidad");
        let mechanics = shown
            .iter()
            .position(|code| *code == "HM-DES-030")
            .expect("el aviso mecánico está");
        assert!(
            viscous < mechanics,
            "la mecánica queda después de la viscosidad: {shown:?}"
        );
    }

    #[test]
    fn a_failed_stage_counts_as_an_error() {
        let ok = compute(&Inputs::default()).unwrap();
        assert!(!ok.has_errors());

        for stage in ["geometry", "mechanics", "viscous"] {
            let mut outcome = ok.clone();
            let failure = DesignError::Input("la etapa no salió".into());
            match stage {
                "geometry" => outcome.geometry = Err(failure),
                "mechanics" => outcome.mechanics = Err(failure),
                _ => outcome.viscous = Err(failure),
            }
            assert!(outcome.has_errors(), "{stage}");
        }
    }

    #[test]
    fn compute_agrees_with_the_loose_design_calls() {
        // Contra las llamadas sueltas de `hm-design`, sin pasar por `analyze`.
        let outcome = compute(&Inputs::default()).unwrap();
        let rho = hm_units::parse_quantity(&format!(
            "{} kg/m3",
            hm_design::water_density(293.15).unwrap()
        ))
        .unwrap();
        let q = hm_units::parse_quantity("120 m3/h").unwrap();
        let n = hm_units::parse_quantity("2900 rpm").unwrap();
        let input = DesignInput {
            eta_m: hm_design::default_mechanical_efficiency(&q, &n).unwrap(),
            q,
            n,
            h: hm_units::parse_quantity("45 m").unwrap(),
            rho,
            npsha: Some(hm_units::parse_quantity("15 m").unwrap()),
            nu: hm_design::fluid::water_kinematic_viscosity(293.15),
        };
        let result = hm_design::design(&input).unwrap();
        let curve = hm_design::performance_curve(&input, &result, CURVE_POINTS);

        assert_eq!(format!("{:?}", outcome.result), format!("{result:?}"));
        assert_eq!(format!("{:?}", outcome.curve), format!("{curve:?}"));
    }

    fn installation(pressure: &str, height: &str, losses: &str, vapor: &str) -> Inputs {
        Inputs {
            intake: IntakeMode::Installation,
            suction_pressure: pressure.into(),
            suction_height: height.into(),
            suction_losses: losses.into(),
            suction_vapor: vapor.into(),
            ..Inputs::default()
        }
    }

    #[test]
    fn installation_intake_computes_the_npsha() {
        let outcome = compute(&installation("1 atm", "", "", "")).unwrap();
        let npsha = outcome
            .input
            .npsha
            .as_ref()
            .expect("la instalación da NPSHa");
        // Agua a 20 °C a 1 atm: unos 10 m, no los 15 m de la caja escrita.
        assert!((9.0..11.0).contains(&npsha.si), "{}", npsha.si);
        assert!(!outcome.has_errors());
    }

    #[test]
    fn switching_to_installation_with_the_default_pressure_computes() {
        // Al pulsar «instalación» sin tocar nada no debe aparecer un error.
        let inputs = Inputs {
            intake: IntakeMode::Installation,
            ..Inputs::default()
        };
        let outcome = compute(&inputs).expect("la presión por defecto basta");
        assert!(outcome.input.npsha.is_some());
    }

    #[test]
    fn empty_height_is_not_zero() {
        let empty = case_from_inputs(&installation("1 atm", "", "", "")).unwrap();
        let zero = case_from_inputs(&installation("1 atm", "0 m", "0 m", "")).unwrap();
        match (&empty.intake, &zero.intake) {
            (
                Intake::Installation {
                    height: a,
                    losses: b,
                    ..
                },
                Intake::Installation {
                    height: c,
                    losses: d,
                    ..
                },
            ) => {
                assert!(a.is_none() && b.is_none(), "vacío no se escribe como 0");
                assert!(c.is_some() && d.is_some());
            }
            other => panic!("se esperaba la instalación: {other:?}"),
        }
    }

    #[test]
    fn empty_vapor_pressure_uses_water() {
        let case = case_from_inputs(&installation("1 atm", "", "", "")).unwrap();
        match case.intake {
            Intake::Installation { vapor_pressure, .. } => assert!(vapor_pressure.is_none()),
            other => panic!("{other:?}"),
        }
        let with_vapor = compute(&installation("1 atm", "", "", "50 kPa")).unwrap();
        let without = compute(&installation("1 atm", "", "", "")).unwrap();
        // Con una presión de vapor mayor queda menos NPSHa.
        assert!(
            with_vapor.input.npsha.as_ref().unwrap().si < without.input.npsha.as_ref().unwrap().si
        );
    }

    #[test]
    fn a_missing_pressure_is_an_error_on_the_group() {
        let errors = compute(&installation("", "2 m", "", "")).unwrap_err();
        assert!(
            errors.iter().any(|(field, _)| *field == Field::Suction),
            "{errors:?}"
        );
    }

    #[test]
    fn bad_suction_value_lands_under_the_suction_group() {
        let errors = compute(&installation("mucho", "", "", "")).unwrap_err();
        let (field, message) = &errors[0];
        assert_eq!(*field, Field::Suction);
        assert!(message.starts_with("presión: "), "{message}");

        let errors = compute(&installation("1 atm", "abc", "", "")).unwrap_err();
        assert!(errors[0].1.starts_with("altura: "), "{errors:?}");
    }

    #[test]
    fn every_bad_suction_box_has_its_own_error() {
        let errors = compute(&installation("mucho", "abc", "", "")).unwrap_err();
        assert!(
            suction_box_error(&errors, "presión").is_some(),
            "{errors:?}"
        );
        assert!(suction_box_error(&errors, "altura").is_some(), "{errors:?}");
        assert!(
            suction_box_error(&errors, "pérdidas").is_none(),
            "{errors:?}"
        );
        assert_eq!(suction_group_errors(&errors).count(), 0, "{errors:?}");
    }

    #[test]
    fn an_engine_error_without_a_box_shows_under_the_group() {
        // El motor no dice de qué caja es un error de unidades: va bajo el grupo.
        let errors = compute(&installation("1 m", "", "", "")).unwrap_err();
        assert!(
            suction_box_error(&errors, "presión").is_none(),
            "{errors:?}"
        );
        assert_eq!(suction_group_errors(&errors).count(), 1, "{errors:?}");
        assert!(
            errors
                .iter()
                .all(|(_, message)| !message.contains("[suction]")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_suction_value_with_the_wrong_unit_lands_on_the_group() {
        let errors = compute(&installation("1 m", "", "", "")).unwrap_err();
        assert!(
            errors.iter().all(|(field, _)| *field == Field::Suction),
            "{errors:?}"
        );
    }

    #[test]
    fn written_and_installation_ignore_each_other() {
        // En «instalación» la caja escrita no cuenta, aunque tenga basura.
        let mut inputs = installation("1 atm", "", "", "");
        inputs.npsha = "basura".into();
        assert!(compute(&inputs).is_ok());

        // En «escrito» las cajas de la instalación no cuentan.
        let inputs = Inputs {
            suction_pressure: "basura".into(),
            ..Inputs::default()
        };
        assert!(compute(&inputs).is_ok());
    }

    /// Sin archivo abierto: las pruebas de las cajas no necesitan más.
    fn compute(inputs: &Inputs) -> Result<Outcome, FieldErrors> {
        super::compute(inputs, &Basis::default())
    }

    fn case_from_inputs(inputs: &Inputs) -> Result<Case, FieldErrors> {
        super::case_from_inputs(inputs, &Basis::default())
    }

    /// Fixture v0.9 con el rodete real de la nota: seis álabes y su φ.
    fn opened_with_selections() -> (Project, OpenedFile) {
        let mut project = load("end_suction_v0.9.hmf");
        for (name, value) in [("blade_count", "6"), ("phi", "0.0785")] {
            project.params.insert(
                name.into(),
                Param::Manual {
                    value: value.into(),
                },
            );
        }
        let path = std::env::temp_dir().join(format!(
            "hmflow-selecciones-{:?}.hmf",
            std::thread::current().id()
        ));
        project.save(&path).unwrap();
        let opened = open_file(&path);
        let _ = std::fs::remove_file(&path);
        (project, opened.unwrap())
    }

    #[test]
    fn the_window_shows_the_design_of_the_file() {
        let (project, opened) = opened_with_selections();
        let case = hm_engine::case_from_project(&project).unwrap();
        let file = analyze(&case, CurvePoints::clamped(CURVE_POINTS)).unwrap();

        let window = super::compute(&opened.inputs, &opened.basis).unwrap();

        assert_eq!(window.result.selection.blade_count, 6);
        assert_eq!(format!("{:?}", window.result), format!("{:?}", file.result));
        assert_eq!(format!("{:?}", window.curve), format!("{:?}", file.curve));
    }

    #[test]
    fn an_edit_keeps_the_selections_of_the_file() {
        let (_, opened) = opened_with_selections();
        let inputs = Inputs {
            q: "100 m3/h".into(),
            ..opened.inputs
        };

        let window = super::compute(&inputs, &opened.basis).unwrap();

        assert_eq!(window.result.selection.blade_count, 6);
    }

    #[test]
    fn the_written_density_stays_until_the_temperature_changes() {
        let (_, opened) = opened_with_selections();
        let written = super::compute(&opened.inputs, &opened.basis).unwrap();
        assert!((written.input.rho.si - 998.204).abs() < 1e-12);

        let inputs = Inputs {
            temperature_c: "60".into(),
            ..opened.inputs
        };
        let warmer = super::compute(&inputs, &opened.basis).unwrap();
        assert!(
            (warmer.input.rho.si - 983.2).abs() < 0.1,
            "{}",
            warmer.input.rho.si
        );
    }

    #[test]
    fn a_locked_density_is_on_screen_whatever_the_temperature() {
        let mut project = load("end_suction_v0.9.hmf");
        project.params.insert(
            "rho".into(),
            Param::Locked {
                value: "998.204 kg/m3".into(),
            },
        );
        let path = std::env::temp_dir().join(format!(
            "hmflow-rho-bloqueada-{:?}.hmf",
            std::thread::current().id()
        ));
        project.save(&path).unwrap();
        let opened = open_file(&path);
        let _ = std::fs::remove_file(&path);
        let opened = opened.unwrap();

        let inputs = Inputs {
            temperature_c: "60".into(),
            ..opened.inputs
        };
        let window = super::compute(&inputs, &opened.basis).unwrap();

        assert!((window.input.rho.si - 998.204).abs() < 1e-12);
    }

    #[test]
    fn a_file_without_written_density_uses_the_water_of_the_box() {
        let opened = open_file(&fixture("end_suction_v0.9.hmf")).unwrap();
        let basis = Basis {
            density: None,
            ..opened.basis
        };
        let window = super::compute(&opened.inputs, &basis).unwrap();
        assert!(
            (window.input.rho.si - 998.207).abs() < 0.01,
            "{}",
            window.input.rho.si
        );
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../hm-project/tests/fixtures")
            .join(name)
    }

    fn load(name: &str) -> Project {
        Project::load(&fixture(name)).unwrap()
    }

    #[test]
    fn case_to_inputs_copies_the_texts() {
        let project = load("end_suction_v0.9.hmf");
        let case = hm_engine::case_from_project(&project).unwrap();
        let inputs = case_to_inputs(&case);

        assert_eq!(inputs.q, "120 m3/h");
        assert_eq!(inputs.h, "45 m");
        assert_eq!(inputs.n, "2900 rpm");
        assert_eq!(inputs.npsha, "15 m");
        assert_eq!(inputs.intake, IntakeMode::Written);

        let mut case = case;
        case.intake = Intake::Installation {
            pressure: Entered::parse("1 atm").unwrap(),
            height: Some(Entered::parse("2 m").unwrap()),
            losses: None,
            vapor_pressure: Some(Entered::parse("2.3 kPa").unwrap()),
        };
        let inputs = case_to_inputs(&case);
        assert_eq!(inputs.intake, IntakeMode::Installation);
        assert!(inputs.npsha.is_empty());
        assert_eq!(inputs.suction_pressure, "1 atm");
        assert_eq!(inputs.suction_height, "2 m");
        assert!(inputs.suction_losses.is_empty());
        assert_eq!(inputs.suction_vapor, "2.3 kPa");
    }

    #[test]
    fn temperature_shows_degrees_celsius() {
        let mut project = load("end_suction_v0.9.hmf");
        project.operating.temperature = "80 degC".into();
        let mut case = hm_engine::case_from_project(&project).unwrap();
        assert_eq!(case_to_inputs(&case).temperature_c, "80");

        // Un número que la caja ya acepta se conserva, ceros incluidos.
        case.fluid.temperature = Some(Temperature {
            text: "20.0".into(),
            kelvin: 20.0 + 273.15,
        });
        assert_eq!(case_to_inputs(&case).temperature_c, "20.0");

        case.fluid.temperature = Some(Temperature {
            text: "20,5".into(),
            kelvin: 20.5 + 273.15,
        });
        assert_eq!(case_to_inputs(&case).temperature_c, "20,5");
    }

    #[test]
    fn open_fixture_fills_the_boxes_and_computes() {
        let path = fixture("end_suction_v0.9.hmf");
        let opened = open_file(&path).unwrap();

        assert_eq!(opened.inputs.q, "120 m3/h");
        assert_eq!(opened.inputs.h, "45 m");
        assert_eq!(opened.inputs.n, "2900 rpm");
        assert_eq!(opened.inputs.temperature_c, "20");
        assert_eq!(opened.inputs.npsha, "15 m");
        assert_eq!(opened.inputs.eta_m, "0.9856064144305404");
        assert!(compute(&opened.inputs).is_ok());

        let project = Project::load(&path).unwrap();
        let from_engine = open_project(&project).unwrap();
        assert_eq!(opened.diagnostics, from_engine.diagnostics.into_vec());
    }

    #[test]
    fn a_file_without_installation_can_switch_to_it() {
        let opened = open_file(&fixture("end_suction_v0.9.hmf")).unwrap();
        assert_eq!(opened.inputs.suction_pressure, "1 atm");
        let inputs = Inputs {
            intake: IntakeMode::Installation,
            ..opened.inputs
        };

        assert!(super::compute(&inputs, &opened.basis).is_ok());
    }

    #[test]
    fn a_rejected_file_returns_its_reason() {
        let mut project = load("end_suction_v0.9.hmf");
        project.operating.fluid = "aceite".into();
        let path = std::env::temp_dir().join("hmflow-abrir-aceite.hmf");
        project.save(&path).unwrap();
        let error = open_file(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert_eq!(error, "esta interfaz solo edita proyectos de agua");

        let mut project = load("end_suction_v0.9.hmf");
        project.design.method = "end_suction_v9.9".into();
        let path = std::env::temp_dir().join("hmflow-abrir-metodo.hmf");
        project.save(&path).unwrap();
        let error = open_file(&path).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(error.contains("HM-PROJ-016"), "{error}");
    }

    #[test]
    fn opening_a_legacy_project_keeps_the_notice_first() {
        let opened = open_file(&fixture("end_suction_v0.8.hmf")).unwrap();
        assert_eq!(opened.diagnostics[0].code, "HM-PROJ-025");
        assert_eq!(opened.inputs.q, "120 m3/h");
    }

    #[test]
    fn an_unreadable_file_returns_an_error() {
        let error = open_file(std::path::Path::new("/no/existe/hmflow.hmf")).unwrap_err();
        assert!(error.contains("E/S"), "{error}");
    }
}
