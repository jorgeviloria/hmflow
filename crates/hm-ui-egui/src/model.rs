//! Estado de ingeniería de la interfaz, sin dependencia de egui.
//!
//! Arma un caso de agua desde las entradas y lo pasa al motor. La interfaz
//! muestra el `Analysis`: diseño, curva, etapas y diagnósticos en el orden
//! canónico. Abrir un archivo es `Project::load` y `open_project`: aquí no se
//! interpreta el esquema.

use std::path::Path;

use hm_core::{Diagnostic, Diagnostics};
use hm_design::geometry::Geometry;
use hm_design::mechanics::{BALANCE_HOLE_AREA_RATIO, Mechanics};
use hm_design::{CurvePoint, DesignError, DesignInput, DesignResult, ViscousCorrection};
use hm_engine::redefines_eta_m;
use hm_engine::{
    Analysis, Case, CaseField, ChangeKind, CurvePoints, Density, DensitySource, DesignEdits,
    DesignOptions, Duty, EditorError, EngineError, Entered, Fluid, FluidKind, Intake, Opened,
    ParamChange, PumpType, Temperature, analyze, cli_message, design_edits, edited_options,
    new_casing_project, new_project, open_project, report, reselect_design, reselected_options,
    update_design, update_point, water_density_at,
};
use hm_project::{Casing, Param, Project};
use hm_units::Quantity;

/// Puntos de la curva que se dibujan.
pub const CURVE_POINTS: usize = 61;
/// Puntos de curva del informe: el defecto de `hmflow-cli report`.
const REPORT_CURVE_POINTS: usize = 11;

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
    /// Carcasa, si el rodete es para una existente.
    pub casing: Option<CasingBoxes>,
    /// Decisiones: vacío significa que decide el motor.
    pub phi: String,
    pub blade_count: String,
    /// Medidas: vacío significa el valor por defecto.
    pub roughness: String,
    pub incidence: String,
}

/// Cajas de selecciones y medidas: parámetro, etiqueta y ejemplo.
pub const SELECTION_BOXES: [(&str, &str, &str); 4] = [
    ("phi", "φ (vacío: el motor)", "0.0785"),
    ("blade_count", "Z (vacío: el motor)", "7"),
    ("impeller_roughness", "Rugosidad (vacío: liso)", "0.05 mm"),
    ("incidence_deg", "Incidencia (vacío: 2°)", "4 deg"),
];

impl Inputs {
    /// El texto de una caja de [`SELECTION_BOXES`].
    pub fn selection_mut(&mut self, name: &str) -> Option<&mut String> {
        Some(match name {
            "phi" => &mut self.phi,
            "blade_count" => &mut self.blade_count,
            "impeller_roughness" => &mut self.roughness,
            "incidence_deg" => &mut self.incidence,
            _ => return None,
        })
    }
}

/// Las cajas de `[casing]`. Dos son números en el archivo, pero aquí se escriben:
/// una caja a medias no puede ser un número.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CasingBoxes {
    pub fields: Casing,
    pub channels: String,
    pub hole_count: String,
}

/// Cajas de la carcasa: campo, etiqueta y ejemplo.
pub const CASING_BOXES: [(&str, &str, &str); 10] = [
    ("d2", "D2", "425 mm"),
    ("neck_diameter", "Cuello", "160 mm"),
    ("eye_diameter_max", "Ojo máximo", "140 mm"),
    ("bore_diameter", "Agujero del eje", "41 mm"),
    ("hub_diameter", "Cubo mínimo", "58 mm"),
    ("b3", "b3", "32 mm"),
    ("cutwater_radius", "Radio de lengua", "220 mm"),
    ("channels", "Canales", "1 o 2"),
    ("wrap", "Entre lenguas (2 canales)", "180 deg"),
    ("throat_area", "Garganta (opcional)", "vacío si no se midió"),
];

/// Cajas de los datos de fuga: todos o ninguno.
pub const LEAKAGE_BOXES: [(&str, &str, &str); 8] = [
    ("seal_clearance", "Holgura diametral", "0.3 mm"),
    ("seal_length", "Largo del anillo delantero", "15 mm"),
    ("rear_seal_diameter", "Anillo trasero", "170 mm"),
    ("rear_seal_length", "Largo del anillo trasero", "15 mm"),
    ("balance_hole_count", "Agujeros de equilibrio", "7"),
    ("balance_hole_diameter", "Diámetro de agujero", "11 mm"),
    ("balance_hole_circle", "Círculo de agujeros", "120 mm"),
    (
        "shaft_seal_diameter",
        "Cierre del eje (opcional)",
        "vacío: el cubo",
    ),
];

impl CasingBoxes {
    /// Una carcasa sin escribir: un canal, el caso simple.
    pub fn blank() -> Self {
        Self {
            channels: "1".to_string(),
            ..Self::default()
        }
    }

    fn from_casing(casing: &Casing) -> Self {
        let count = casing.balance_hole_count;
        Self {
            fields: casing.clone(),
            channels: casing.channels.to_string(),
            hole_count: if count == 0 {
                String::new()
            } else {
                count.to_string()
            },
        }
    }

    /// El texto de una caja. `None` si el nombre no es de una caja.
    pub fn text_mut(&mut self, name: &str) -> Option<&mut String> {
        let fields = &mut self.fields;
        Some(match name {
            "d2" => &mut fields.d2,
            "neck_diameter" => &mut fields.neck_diameter,
            "eye_diameter_max" => &mut fields.eye_diameter_max,
            "bore_diameter" => &mut fields.bore_diameter,
            "hub_diameter" => &mut fields.hub_diameter,
            "b3" => &mut fields.b3,
            "cutwater_radius" => &mut fields.cutwater_radius,
            "channels" => &mut self.channels,
            "wrap" => &mut fields.wrap,
            "throat_area" => &mut fields.throat_area,
            "seal_clearance" => &mut fields.seal_clearance,
            "seal_length" => &mut fields.seal_length,
            "rear_seal_diameter" => &mut fields.rear_seal_diameter,
            "rear_seal_length" => &mut fields.rear_seal_length,
            "balance_hole_count" => &mut self.hole_count,
            "balance_hole_diameter" => &mut fields.balance_hole_diameter,
            "balance_hole_circle" => &mut fields.balance_hole_circle,
            "shaft_seal_diameter" => &mut fields.shaft_seal_diameter,
            _ => return None,
        })
    }

    /// La carcasa de las cajas. Solo lee los dos números: las cotas las juzga el
    /// motor.
    fn to_casing(&self) -> Result<Casing, (Field, String)> {
        let number = |name: &'static str, text: &str| {
            let text = text.trim();
            if text.is_empty() {
                return Ok(0);
            }
            text.parse::<u32>().map_err(|_| {
                (
                    Field::Casing(name),
                    "un número entero, p. ej. 2".to_string(),
                )
            })
        };
        Ok(Casing {
            channels: number("channels", &self.channels)?,
            balance_hole_count: number("balance_hole_count", &self.hole_count)?,
            ..self.fields.clone()
        })
    }
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
            casing: None,
            phi: String::new(),
            blade_count: String::new(),
            roughness: String::new(),
            incidence: String::new(),
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
    /// Una cota de `[casing]`.
    Casing(&'static str),
    /// Una caja de [`SELECTION_BOXES`].
    Selection(&'static str),
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
    /// Hay errores, y reelegir los quitaría.
    pub reselect_hint: bool,
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
    let eta_m = read_eta_m(inputs, basis.eta_m_is_locked(), &mut errors);
    let intake = read_intake(inputs, &mut errors);
    let design = read_design(inputs, basis, &mut errors);

    // Los `Option` solo son `None` si hay un error en `errors`.
    let (
        Some(q),
        Some(h),
        Some(n),
        Some((temperature, rho)),
        Some(eta_m),
        Some(intake),
        Some(design),
    ) = (q, h, n, temperature, eta_m, intake, design)
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
        design,
    })
}

/// Lo que las cajas piden del diseño, además del punto.
fn edits_from(inputs: &Inputs) -> Result<DesignEdits, (Field, String)> {
    Ok(DesignEdits {
        casing: inputs
            .casing
            .as_ref()
            .map(CasingBoxes::to_casing)
            .transpose()?,
        phi: inputs.phi.clone(),
        blade_count: inputs.blade_count.clone(),
        roughness: inputs.roughness.clone(),
        incidence: inputs.incidence.clone(),
    })
}

fn read_design(inputs: &Inputs, basis: &Basis, errors: &mut FieldErrors) -> Option<DesignOptions> {
    let read = edits_from(inputs).and_then(|edits| {
        edited_options(basis.project.as_ref(), &edits)
            .map_err(|error| engine_error_field(error, !inputs.eta_m.trim().is_empty()))
    });
    match read {
        Ok(design) => Some(design),
        Err(error) => {
            errors.push(error);
            None
        }
    }
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
fn read_eta_m(inputs: &Inputs, locked: bool, errors: &mut FieldErrors) -> Option<Option<f64>> {
    if inputs.eta_m.trim().is_empty() {
        return Some(None);
    }
    match inputs.eta_m.trim().replace(',', ".").parse::<f64>() {
        Ok(value) if value > 0.0 && value < 1.0 => Some(Some(value)),
        _ => {
            // Con `locked` la caja no se edita: la salida es corregir el archivo.
            errors.push((
                Field::EtaM,
                if locked {
                    "ηm está en locked con un valor que no vale: corrígelo en el archivo".into()
                } else {
                    "número entre 0 y 1, o vacío".into()
                },
            ));
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
    let case = case_from_inputs(&with_locked_eta_m(inputs, basis), basis)?;
    let analysis = analyze(&case, CurvePoints::clamped(CURVE_POINTS)).map_err(|error| {
        let (field, mut message) = engine_error_field(error, !inputs.eta_m.trim().is_empty());
        if reselecting_helps(inputs, basis) {
            message.push(' ');
            message.push_str(RESELECT_HINT);
        }
        vec![(field, message)]
    })?;
    let mut outcome = outcome_from(inputs, analysis);
    outcome.reselect_hint = outcome.has_errors() && reselecting_helps(inputs, basis);
    Ok(outcome)
}

/// Un error de una caja de selecciones: si el bloque está plegado, no se vería.
pub fn selection_has_error(errors: &[(Field, String)]) -> bool {
    errors
        .iter()
        .any(|(field, _)| matches!(field, Field::Selection(_)))
}

/// Lo que se dice si el fallo viene de selecciones guardadas.
pub const RESELECT_HINT: &str = "Pulsa «Reelegir con el motor»: viene de selecciones guardadas.";

/// Reelegir saldría bien. Solo se pregunta tras un fallo, no en cada tecla.
fn reselecting_helps(inputs: &Inputs, basis: &Basis) -> bool {
    basis.project.is_some() && reselect_project(inputs, basis).is_ok()
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
        reselect_hint: false,
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
        Some(CaseField::Casing(name)) => Field::Casing(name),
        Some(CaseField::Param(name))
            if SELECTION_BOXES
                .iter()
                .any(|(box_name, _, _)| *box_name == name) =>
        {
            Field::Selection(name)
        }
        Some(CaseField::MechanicalEfficiency | CaseField::Viscosity | CaseField::Param(_))
        | None => Field::General,
    };
    // La caja ya dice de qué campo se trata: sin el «entrada inválida: » de la CLI.
    let message = cli_message(&error);
    let message = message
        .strip_prefix("entrada inválida: ")
        .map_or_else(|| message.clone(), str::to_string);
    // El diseño rechaza algunos valores sin campo, pero su frase nombra el parámetro.
    let field = SELECTION_BOXES
        .iter()
        .find(|(name, _, _)| message.starts_with(&format!("`{name}`")))
        .filter(|_| field == Field::General)
        .map_or(field, |(name, _, _)| Field::Selection(name));
    let message = if let Field::Casing(name) = field {
        // La caja ya nombra la cota: «[casing] `b3` hace falta» -> «hace falta».
        message
            .strip_prefix(&format!("[casing] `{name}` "))
            .map_or_else(|| message.clone(), str::to_string)
    } else if field == Field::Suction {
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
    /// El proyecto abierto. Sus selecciones y lo que esté fijado no tienen caja.
    pub project: Option<Project>,
    /// Densidad escrita en el archivo y la caja de temperatura con que se
    /// abrió. Solo vale mientras esa caja no cambie, salvo si está bloqueada.
    pub density: Option<(String, Density)>,
    pub rho_locked: bool,
    /// El texto de una ηm bloqueada: al guardar, la caja no la pisa.
    pub locked_eta_m: Option<String>,
}

impl Basis {
    fn from_opened(project: &Project, opened: &Opened, inputs: &Inputs) -> Self {
        let density = &opened.case.fluid.density;
        let written = matches!(
            &density.source,
            DensitySource::Explicit { text } if !text.trim().is_empty()
        );
        Self {
            project: Some(project.clone()),
            density: written.then(|| (inputs.temperature_c.clone(), density.clone())),
            rho_locked: opened.rho_locked,
            locked_eta_m: if opened.eta_m_locked {
                opened.eta_m_text.clone()
            } else {
                None
            },
        }
    }

    /// ηm está en `locked`: su caja no cuenta.
    pub fn eta_m_is_locked(&self) -> bool {
        self.locked_eta_m.is_some()
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
    /// Lo que cambió al migrar en memoria. Vacío si no hizo falta.
    pub migrated: Vec<String>,
    /// Por qué no se pudo migrar. El proyecto queda como estaba.
    pub migration_failure: Option<String>,
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
        casing: None,
        phi: String::new(),
        blade_count: String::new(),
        roughness: String::new(),
        incidence: String::new(),
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
///
/// Un método hasta v0.3 se migra aquí, en memoria: ηm cambió de significado, y
/// calcular con el viejo mostraría algo que luego no se guarda. `migrated` lo dice
/// antes de escribir nada.
pub fn open_loaded(project: Project) -> Result<OpenedFile, String> {
    let mut migration_failure = None;
    let (project, migrated) = if redefines_eta_m(&project.design.method) {
        // Si no se puede migrar, se abre igual: antes abría, y reelegir es la salida.
        match hm_engine::recalc(project.clone(), false) {
            Ok(recalculated) => {
                let migrated = migration_lines(&recalculated.changes);
                (recalculated.project, migrated)
            }
            Err(error) => {
                migration_failure = Some(engine_failure(&error));
                (project, Vec::new())
            }
        }
    } else {
        (project, Vec::new())
    };
    let opened = match open_project(&project) {
        Ok(opened) => opened,
        Err(EditorError::Unsupported { reason }) => return Err(reason),
        Err(EditorError::Invalid { diagnostics }) => return Err(format_diagnostics(&diagnostics)),
    };
    let mut inputs = case_to_inputs(&opened.case);
    if let Some(text) = &opened.eta_m_text {
        inputs.eta_m.clone_from(text);
    }
    inputs.casing = opened.edits.casing.as_ref().map(CasingBoxes::from_casing);
    inputs.phi.clone_from(&opened.edits.phi);
    inputs.blade_count.clone_from(&opened.edits.blade_count);
    inputs.roughness.clone_from(&opened.edits.roughness);
    inputs.incidence.clone_from(&opened.edits.incidence);
    let basis = Basis::from_opened(&project, &opened, &inputs);
    Ok(OpenedFile {
        project,
        inputs,
        basis,
        diagnostics: opened.diagnostics.into_vec(),
        migrated,
        migration_failure,
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
    /// Lo que cambió por migrar un método anterior: el archivo lo guarda distinto
    /// aunque no se haya tocado ninguna caja.
    pub migrated: Vec<String>,
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
    let inputs = with_locked_eta_m(inputs, basis);
    let case = case_from_inputs(&inputs, basis).map_err(|errors| field_failure(&errors))?;
    let analysis = analyze(&case, CurvePoints::clamped(CURVE_POINTS))
        .map_err(|error| engine_failure(&error))?;
    if let Some(message) = stage_failure(&analysis) {
        return Err(message);
    }
    let edits = edits_from(&inputs).map_err(|(_, message)| message)?;
    match previous {
        None => {
            let project = match &edits.casing {
                Some(casing) => new_casing_project(name, &case, &analysis, casing),
                None => new_project(name, &case, &analysis),
            }
            .map_err(|error| engine_failure(&error))?;
            // `design_edits` no ve lo que acaba de elegir el motor, solo lo fijado.
            let project = if edits == design_edits(&project) {
                project
            } else {
                update_design(project, &edits)
                    .map_err(|error| engine_failure(&error))?
                    .project
            };
            Ok(Built {
                project,
                diagnostics: analysis.diagnostics,
                migrated: Vec::new(),
            })
        }
        Some(project) => {
            let project = update_point(project.clone(), &case, &analysis)
                .map_err(|error| engine_failure(&error))?;
            let recalculated =
                update_design(project, &edits).map_err(|error| engine_failure(&error))?;
            let migrated = migration_lines(&recalculated.changes);
            Ok(Built {
                migrated,
                project: recalculated.project,
                diagnostics: recalculated.diagnostics.into_vec(),
            })
        }
    }
}

/// Las cajas, con la ηm bloqueada en vez de lo escrito: es la que se calcula y se
/// guarda.
fn with_locked_eta_m(inputs: &Inputs, basis: &Basis) -> Inputs {
    match &basis.locked_eta_m {
        Some(text) => Inputs {
            eta_m: text.clone(),
            ..inputs.clone()
        },
        None => inputs.clone(),
    }
}

/// Un proyecto reelegido, sin escribir, y lo que cambió.
#[derive(Debug)]
pub struct Reselected {
    pub project: Project,
    pub changes: Vec<String>,
}

/// Número corto: un texto del archivo lleva todas las cifras.
fn short_value(text: &str) -> String {
    let (number, unit) = text.split_once(' ').unwrap_or((text, ""));
    match number.parse::<f64>() {
        Ok(value) => format!("{value:.4} {unit}").trim().to_string(),
        Err(_) => text.to_string(),
    }
}

/// Una línea por cambio que pase el filtro.
fn change_lines(changes: &[ParamChange], keep: impl Fn(&ParamChange) -> bool) -> Vec<String> {
    changes
        .iter()
        .filter(|change| keep(change))
        .map(|change| {
            let before = change.before.as_deref().unwrap_or("nada");
            let (old, new) = (short_value(before), short_value(&change.after));
            // Si redondeados son iguales, se muestran enteros: el cambio es real.
            if old == new {
                format!("{}: {before} → {}", change.name, change.after)
            } else {
                format!("{}: {old} → {new}", change.name)
            }
        })
        .collect()
}

/// Lo que cambió por migrar un método anterior.
fn migration_lines(changes: &[ParamChange]) -> Vec<String> {
    change_lines(changes, |change| {
        matches!(
            change.kind,
            ChangeKind::MethodMigrated | ChangeKind::MechanicalEfficiency
        )
    })
}

/// Rehace lo que esté en `manual` con estas cajas. Es una orden: guardar nunca
/// reelige. No escribe el archivo. Funciona aunque el diseño de las cajas falle,
/// porque es la salida de un φ manual que ya no cabe.
pub fn reselect_project(inputs: &Inputs, basis: &Basis) -> Result<Reselected, String> {
    let Some(project) = &basis.project else {
        return Err("solo se reelige en un proyecto guardado".to_string());
    };
    let edits = edits_from(inputs).map_err(|(_, message)| message)?;
    let inputs = with_locked_eta_m(inputs, basis);
    let mut case = case_from_inputs(&inputs, basis).map_err(|errors| field_failure(&errors))?;
    case.design = reselected_options(project, &edits).map_err(|error| engine_failure(&error))?;
    let analysis = analyze(&case, CurvePoints::clamped(CURVE_POINTS))
        .map_err(|error| engine_failure(&error))?;
    if let Some(message) = stage_failure(&analysis) {
        return Err(message);
    }
    let updated =
        update_point(project.clone(), &case, &analysis).map_err(|error| engine_failure(&error))?;
    let recalculated = reselect_design(updated, &edits).map_err(|error| engine_failure(&error))?;
    let stored = |name: &str| match project.params.get(name) {
        Some(Param::Manual { value } | Param::Locked { value }) => Some(value.clone()),
        Some(Param::Auto {
            value: Some(value), ..
        }) => Some(value.clone()),
        _ => None,
    };
    // Decide la tolerancia del motor, no el texto redondeado: si no, un cambio real
    // en la quinta cifra se daría por nada y se tiraría. Lo rellenado no trae `before`.
    let known: Vec<ParamChange> = recalculated
        .changes
        .iter()
        .cloned()
        .map(|change| ParamChange {
            before: change.before.clone().or_else(|| stored(&change.name)),
            ..change
        })
        .collect();
    let changes = change_lines(&known, |change| {
        !change
            .before
            .as_deref()
            .is_some_and(|before| hm_engine::same_value(before, &change.after))
    });
    Ok(Reselected {
        project: recalculated.project,
        changes,
    })
}

/// JSON del informe, con el mismo proyecto en memoria que al guardar.
///
/// Once puntos, como `hmflow-cli report`. No escribe el `.hmf`: un método
/// anterior se migra solo dentro de este texto.
pub fn export_report(
    inputs: &Inputs,
    basis: &Basis,
    previous: Option<&Project>,
    name: &str,
) -> Result<String, String> {
    let built = build_project(inputs, basis, previous, name)?;
    let report = report(&built.project, CurvePoints::clamped(REPORT_CURVE_POINTS))
        .map_err(|error| engine_failure(&error))?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    Ok(format!("{json}\n"))
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

/// Una fila del bloque de la carcasa.
#[derive(Debug, Clone, PartialEq)]
pub struct CasingRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    pub formula: Option<&'static str>,
}

/// Valor y, si cruza el límite, qué pasa. La fila dice el número; el aviso, qué
/// hacer con él.
fn compared(value: f64, limit: f64, crossed: bool, note: &str) -> String {
    let text = format!("{:.1} / {:.1} mm", value * 1000.0, limit * 1000.0);
    if crossed {
        format!("{text} ({note})")
    } else {
        text
    }
}

/// Lo que importa de un rodete en su carcasa, con sus números. Vacío sin carcasa.
///
/// Los límites son los de `casing_diagnostics`. No calcula nada que no dé el
/// motor: solo pone al lado las dos cifras de cada comparación.
pub fn casing_rows(outcome: &Outcome) -> Vec<CasingRow> {
    let result = &outcome.result;
    let Some(casing) = &result.casing else {
        return Vec::new();
    };
    let row = |key, label, value, formula| CasingRow {
        key,
        label,
        value,
        formula,
    };
    let mut rows = Vec::new();
    if let Ok(mech) = &outcome.mechanics {
        rows.extend([
            row(
                "casing.eye",
                "Ojo y tapas / cuello",
                compared(
                    mech.eye_with_walls,
                    casing.neck_diameter,
                    mech.eye_with_walls > casing.neck_diameter,
                    "no cabe",
                ),
                None,
            ),
            row(
                "casing.shaft",
                "Eje mínimo / agujero",
                compared(
                    mech.shaft_min_diameter,
                    casing.bore_diameter,
                    mech.shaft_min_diameter > casing.bore_diameter,
                    "mayor",
                ),
                None,
            ),
            row(
                "casing.hub",
                "Cubo mínimo / cubo",
                compared(
                    mech.hub_min_diameter,
                    casing.hub_diameter,
                    casing.hub_diameter < mech.hub_min_diameter,
                    "por debajo",
                ),
                None,
            ),
        ]);
    }
    rows.push(row(
        "casing.b3",
        "b3/b2",
        format!("{:.2}", casing.b3 / result.b2.si),
        None,
    ));
    let throat = match result.volute_matching_ratio(outcome.input.q.si) {
        Some(ratio) => format!("{ratio:.2}·Qd"),
        None if casing.throat_area.is_none() => "sin dato".to_string(),
        None => "no limita".to_string(),
    };
    rows.push(row(
        "casing.throat",
        "Qc, límite de la garganta",
        throat,
        Some("design.volute_matching_flow_cm"),
    ));
    if result.leakage_data().is_some() {
        rows.extend([
            row(
                "casing.qsp",
                "Fuga delantera Qsp/Q",
                format!("{:.2} %", 100.0 * result.seal_leakage_ratio),
                Some("design.seal_leakage_flow"),
            ),
            row(
                "casing.qe",
                "Fuga trasera QE/Q",
                format!("{:.2} %", 100.0 * result.rear_seal_leakage_ratio),
                Some("design.seal_leakage_flow"),
            ),
            row(
                "casing.eta_v",
                "ηv / estadístico",
                format!("{:.3} / {:.3}", result.eta_v, result.statistical_eta_v),
                Some("design.volumetric_efficiency_gulich"),
            ),
        ]);
    }
    if let Ok(mech) = &outcome.mechanics
        && let Some(ratio) = mech.balance_hole_area_ratio
    {
        let mut value = format!("{ratio:.1} veces");
        if ratio < BALANCE_HOLE_AREA_RATIO {
            value.push_str(" (pocos)");
        }
        rows.push(row("casing.holes", "Agujeros / paso gastado", value, None));
    }
    if let Some(losses) = &result.impeller_losses {
        let friction = if losses.friction_rough > losses.friction_smooth {
            format!(
                "{:.4} → {:.4}",
                losses.friction_smooth, losses.friction_rough
            )
        } else {
            format!("{:.4}", losses.friction_smooth)
        };
        rows.push(row(
            "casing.friction",
            "Pérdida del rodete ζLa,R (contraste)",
            friction,
            Some("design.impeller_friction_loss"),
        ));
        rows.push(row(
            "casing.shock",
            "Golpe a la entrada ζLa,C (contraste)",
            losses
                .shock
                .map_or("fuera de rango".to_string(), |shock| format!("{shock:.4}")),
            Some("design.impeller_shock_loss"),
        ));
        if let Some(change) = losses.roughness_eta_h {
            rows.push(row(
                "casing.roughness",
                "Rugosidad en ηh (contraste)",
                format!("{:+.2} puntos", 100.0 * change),
                Some("design.impeller_friction_coefficient"),
            ));
        }
    }
    rows
}

/// Con datos de fuga, el empuje axial es el de los agujeros de equilibrio.
pub fn balanced_axial_thrust(result: &DesignResult) -> bool {
    result.leakage_data().is_some()
}

#[cfg(test)]
mod tests {
    use super::{Basis, CasingBoxes, Field, Inputs};

    fn free(edit: impl FnOnce(&mut Inputs)) -> Inputs {
        let mut inputs = Inputs::default();
        edit(&mut inputs);
        inputs
    }

    #[test]
    fn the_selection_boxes_reach_the_design() {
        let outcome = super::compute(
            &free(|inputs| {
                inputs.phi = "0.09".to_string();
                inputs.blade_count = "5".to_string();
                inputs.incidence = "4 deg".to_string();
            }),
            &Basis::default(),
        )
        .unwrap();
        assert_eq!(outcome.result.selection.phi, 0.09);
        assert_eq!(outcome.result.selection.blade_count, 5);
    }

    #[test]
    fn emptying_a_box_gives_the_design_of_the_engine_back() {
        let engine = super::compute(&Inputs::default(), &Basis::default()).unwrap();
        let emptied = free(|inputs| inputs.phi = "   ".to_string());
        let again = super::compute(&emptied, &Basis::default()).unwrap();
        assert_eq!(engine.result.selection.phi, again.result.selection.phi);
    }

    #[test]
    fn a_bad_selection_is_marked_on_its_box() {
        for (name, edit) in [
            (
                "phi",
                (|inputs: &mut Inputs| inputs.phi = "mucho".to_string()) as fn(&mut Inputs),
            ),
            ("blade_count", |inputs| {
                inputs.blade_count = "2.5".to_string()
            }),
            ("impeller_roughness", |inputs| {
                inputs.roughness = "5 kg".to_string()
            }),
            ("incidence_deg", |inputs| {
                inputs.incidence = "5 kg".to_string()
            }),
        ] {
            let errors = errors_of(&free(edit));
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(
                matches!(errors[0].0, Field::Selection(found) if found == name),
                "{name}: {errors:?}"
            );
        }
    }

    #[test]
    fn an_even_blade_count_with_two_channels_keeps_its_warning() {
        let inputs = with_casing(|boxes| {
            *boxes.text_mut("channels").unwrap() = "2".to_string();
            *boxes.text_mut("wrap").unwrap() = "180 deg".to_string();
        });
        let inputs = Inputs {
            blade_count: "6".to_string(),
            ..inputs
        };
        let outcome = super::compute(&inputs, &Basis::default()).unwrap();
        assert!(
            outcome
                .diagnostics
                .iter()
                .any(|item| item.code == "HM-DES-037"),
            "{:?}",
            outcome.diagnostics
        );
    }

    #[test]
    fn a_new_design_without_edits_is_exactly_what_the_engine_writes() {
        let inputs = Inputs::default();
        let basis = Basis::default();
        let case = super::case_from_inputs(&inputs, &basis).unwrap();
        let analysis =
            hm_engine::analyze(&case, hm_engine::CurvePoints::clamped(super::CURVE_POINTS))
                .unwrap();
        let engine = hm_engine::new_project("rodete", &case, &analysis).unwrap();
        let built = super::build_project(&inputs, &basis, None, "rodete").unwrap();
        assert_eq!(built.project.to_toml().unwrap(), engine.to_toml().unwrap());
    }

    fn fit_rows(edit: impl FnOnce(&mut CasingBoxes)) -> Vec<(&'static str, String)> {
        let outcome = super::compute(&with_casing(edit), &Basis::default()).unwrap();
        super::casing_rows(&outcome)
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect()
    }

    fn value<'a>(rows: &'a [(&'static str, String)], key: &str) -> &'a str {
        rows.iter()
            .find(|(found, _)| *found == key)
            .map_or("", |(_, value)| value)
    }

    fn with_leakage(boxes: &mut CasingBoxes) {
        *boxes.text_mut("neck_diameter").unwrap() = "120 mm".to_string();
        *boxes.text_mut("bore_diameter").unwrap() = "29 mm".to_string();
        *boxes.text_mut("hub_diameter").unwrap() = "35 mm".to_string();
        for (name, text) in [
            ("seal_clearance", "0.3 mm"),
            ("seal_length", "15 mm"),
            ("rear_seal_diameter", "80 mm"),
            ("rear_seal_length", "15 mm"),
            ("balance_hole_count", "6"),
            ("balance_hole_diameter", "9 mm"),
            ("balance_hole_circle", "50 mm"),
        ] {
            *boxes.text_mut(name).unwrap() = text.to_string();
        }
    }

    #[test]
    fn without_a_casing_there_is_no_fit_block() {
        let outcome = super::compute(&Inputs::default(), &Basis::default()).unwrap();
        assert!(super::casing_rows(&outcome).is_empty());
    }

    #[test]
    fn the_fit_rows_show_both_numbers() {
        let rows = fit_rows(|_| ());
        assert!(value(&rows, "casing.eye").contains("/ 90.0 mm"), "{rows:?}");
        assert!(
            value(&rows, "casing.shaft").contains("/ 20.0 mm"),
            "{rows:?}"
        );
        assert!(value(&rows, "casing.hub").contains("/ 30.0 mm"), "{rows:?}");
        assert!(!value(&rows, "casing.b3").is_empty(), "{rows:?}");
        assert_eq!(value(&rows, "casing.throat"), "sin dato");
        assert_eq!(value(&rows, "casing.qsp"), "");
    }

    #[test]
    fn a_limit_that_is_crossed_says_so_in_the_row() {
        let rows = fit_rows(|boxes| {
            *boxes.text_mut("neck_diameter").unwrap() = "77 mm".to_string();
        });
        assert!(
            value(&rows, "casing.eye").ends_with("(no cabe)"),
            "{rows:?}"
        );
        let rows = fit_rows(|_| ());
        assert!(!value(&rows, "casing.eye").contains("no cabe"), "{rows:?}");

        let rows = fit_rows(|boxes| {
            *boxes.text_mut("bore_diameter").unwrap() = "5 mm".to_string();
            *boxes.text_mut("hub_diameter").unwrap() = "6 mm".to_string();
        });
        assert!(
            value(&rows, "casing.shaft").ends_with("(mayor)"),
            "{rows:?}"
        );
        assert!(
            value(&rows, "casing.hub").ends_with("(por debajo)"),
            "{rows:?}"
        );
    }

    #[test]
    fn a_measured_throat_shows_qc_only_if_it_limits() {
        let rows = fit_rows(|boxes| {
            *boxes.text_mut("throat_area").unwrap() = "0.0012 m2".to_string();
        });
        assert!(value(&rows, "casing.throat").ends_with("·Qd"), "{rows:?}");
        let rows = fit_rows(|boxes| {
            *boxes.text_mut("throat_area").unwrap() = "0.09 m2".to_string();
        });
        assert_eq!(value(&rows, "casing.throat"), "no limita");
    }

    #[test]
    fn with_leakage_data_the_rows_show_the_leakage() {
        let rows = fit_rows(with_leakage);
        assert!(value(&rows, "casing.qsp").ends_with(" %"), "{rows:?}");
        assert!(value(&rows, "casing.qe").ends_with(" %"), "{rows:?}");
        assert!(value(&rows, "casing.eta_v").contains(" / "), "{rows:?}");
        assert!(value(&rows, "casing.holes").contains("veces"), "{rows:?}");
    }

    #[test]
    fn too_few_holes_say_so_in_the_row() {
        let rows = fit_rows(|boxes| {
            with_leakage(boxes);
            *boxes.text_mut("balance_hole_diameter").unwrap() = "3 mm".to_string();
        });
        assert!(
            value(&rows, "casing.holes").ends_with("(pocos)"),
            "{rows:?}"
        );
        let rows = fit_rows(with_leakage);
        assert!(!value(&rows, "casing.holes").contains("pocos"), "{rows:?}");
    }

    #[test]
    fn the_impeller_losses_are_rows_of_the_casing_block() {
        let rows = fit_rows(|_| ());
        assert!(!value(&rows, "casing.friction").is_empty(), "{rows:?}");
        assert!(!value(&rows, "casing.friction").contains("→"), "{rows:?}");
        assert!(!value(&rows, "casing.shock").is_empty(), "{rows:?}");
        assert_eq!(value(&rows, "casing.roughness"), "");
    }

    #[test]
    fn with_roughness_the_rows_show_what_it_costs() {
        let inputs = Inputs {
            roughness: "0.032 mm".to_string(),
            ..with_casing(|_| ())
        };
        let outcome = super::compute(&inputs, &Basis::default()).unwrap();
        let rows: Vec<_> = super::casing_rows(&outcome)
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect();
        assert!(value(&rows, "casing.friction").contains("→"), "{rows:?}");
        assert!(
            value(&rows, "casing.roughness").ends_with(" puntos"),
            "{rows:?}"
        );
        assert!(
            value(&rows, "casing.roughness").starts_with('-'),
            "{rows:?}"
        );
    }

    #[test]
    fn the_shock_says_when_it_does_not_apply() {
        let mut project = hm_project::Project::new("demo", hm_engine::PumpType::EndSuction);
        for (name, value) in [("cm0", "1.5 m/s"), ("c0", "9 m/s"), ("J", "0.95")] {
            let value = value.to_string();
            project
                .params
                .insert(name.to_string(), hm_project::Param::Locked { value });
        }
        let basis = Basis {
            project: Some(project),
            ..Basis::default()
        };
        let inputs = Inputs {
            incidence: "10 deg".to_string(),
            ..with_casing(|_| ())
        };
        let outcome = super::compute(&inputs, &basis).unwrap();
        let rows: Vec<_> = super::casing_rows(&outcome)
            .into_iter()
            .map(|row| (row.key, row.value))
            .collect();
        assert_eq!(value(&rows, "casing.shock"), "fuera de rango");
    }

    #[test]
    fn a_locked_eta_m_is_what_the_preview_uses() {
        let basis = Basis {
            locked_eta_m: Some("0.95".to_string()),
            ..Basis::default()
        };
        let typed = Inputs {
            eta_m: "0.90".to_string(),
            ..Inputs::default()
        };
        let locked = Inputs {
            eta_m: "0.95".to_string(),
            ..Inputs::default()
        };
        let shown = super::compute(&typed, &basis).unwrap();
        let saved = super::compute(&locked, &Basis::default()).unwrap();
        assert_eq!(shown.result.eta, saved.result.eta);
        // Las cajas no cambian: el resultado sigue siendo de lo que hay escrito.
        assert_eq!(shown.inputs, typed);
    }

    #[test]
    fn a_selection_the_engine_rejects_is_marked_on_its_box() {
        type Edit = fn(&mut Inputs);
        let cases: [(&str, Edit); 3] = [
            ("phi", |inputs| inputs.phi = "0".to_string()),
            ("impeller_roughness", |inputs| {
                inputs.roughness = "-1 mm".to_string();
            }),
            ("impeller_roughness", |inputs| {
                inputs.roughness = "2 m".to_string();
            }),
        ];
        for (name, edit) in cases {
            let errors = errors_of(&free(edit));
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(
                matches!(errors[0].0, Field::Selection(found) if found == name),
                "{name}: {errors:?}"
            );
        }
    }

    #[test]
    fn an_error_about_something_else_stays_general() {
        let inputs = with_casing(|boxes| {
            *boxes.text_mut("b3").unwrap() = "2 mm".to_string();
        });
        let errors = errors_of(&inputs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].0, Field::General, "{errors:?}");
    }

    #[test]
    fn a_locked_eta_m_that_is_not_valid_says_it_is_locked() {
        let basis = Basis {
            locked_eta_m: Some("1".to_string()),
            ..Basis::default()
        };
        let errors = super::compute(&Inputs::default(), &basis).unwrap_err();
        assert_eq!(errors[0].0, Field::EtaM, "{errors:?}");
        assert!(errors[0].1.contains("locked"), "{errors:?}");
    }

    #[test]
    fn an_error_of_a_selection_box_asks_to_show_the_block() {
        assert!(super::selection_has_error(&[(
            Field::Selection("phi"),
            "mal".to_string()
        )]));
        assert!(!super::selection_has_error(&[(
            Field::Q,
            "mal".to_string()
        )]));
    }

    #[test]
    fn every_selection_box_has_a_text() {
        let mut inputs = Inputs::default();
        for (name, _, _) in super::SELECTION_BOXES {
            assert!(inputs.selection_mut(name).is_some(), "{name}");
        }
    }

    fn with_casing(edit: impl FnOnce(&mut CasingBoxes)) -> Inputs {
        let mut boxes = CasingBoxes::blank();
        for (name, text) in [
            ("d2", "210 mm"),
            ("neck_diameter", "90 mm"),
            ("eye_diameter_max", "75 mm"),
            ("bore_diameter", "20 mm"),
            ("hub_diameter", "30 mm"),
            ("b3", "30 mm"),
            ("cutwater_radius", "111 mm"),
        ] {
            *boxes.text_mut(name).unwrap() = text.to_string();
        }
        edit(&mut boxes);
        Inputs {
            casing: Some(boxes),
            ..Inputs::default()
        }
    }

    fn errors_of(inputs: &Inputs) -> Vec<(Field, String)> {
        super::compute(inputs, &Basis::default())
            .err()
            .unwrap_or_default()
    }

    #[test]
    fn the_casing_boxes_reach_the_design() {
        let outcome = super::compute(&with_casing(|_| ()), &Basis::default()).unwrap();
        assert_eq!(outcome.result.d2.si, 0.210);
    }

    #[test]
    fn a_bad_box_is_marked_on_that_box() {
        let inputs = with_casing(|boxes| *boxes.text_mut("b3").unwrap() = "mucho".to_string());
        let errors = errors_of(&inputs);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].0, Field::Casing("b3"));
    }

    #[test]
    fn an_empty_box_only_says_that_it_is_missing() {
        let inputs = with_casing(|boxes| boxes.text_mut("d2").unwrap().clear());
        assert_eq!(
            errors_of(&inputs),
            vec![(Field::Casing("d2"), "hace falta".to_string())]
        );
    }

    #[test]
    fn half_the_leakage_data_marks_what_is_missing() {
        let inputs = with_casing(|boxes| {
            *boxes.text_mut("seal_clearance").unwrap() = "0.3 mm".to_string();
            *boxes.text_mut("balance_hole_count").unwrap() = "6".to_string();
        });
        assert_eq!(
            errors_of(&inputs),
            vec![(Field::Casing("seal_length"), "hace falta".to_string())]
        );
    }

    #[test]
    fn a_count_that_is_not_a_number_is_marked_on_its_box() {
        for name in ["channels", "balance_hole_count"] {
            let inputs = with_casing(|boxes| *boxes.text_mut(name).unwrap() = "dos".to_string());
            let errors = errors_of(&inputs);
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(
                matches!(errors[0].0, Field::Casing(found) if found == name),
                "{errors:?}"
            );
        }
    }

    #[test]
    fn every_box_of_the_panel_has_a_text() {
        let mut boxes = CasingBoxes::blank();
        for (name, _, _) in super::CASING_BOXES.iter().chain(&super::LEAKAGE_BOXES) {
            assert!(boxes.text_mut(name).is_some(), "{name}");
        }
    }

    #[test]
    fn with_a_casing_the_model_volute_is_neither_drawn_nor_listed() {
        let q = |text: &str| hm_units::parse_quantity(text).unwrap();
        let input = hm_design::DesignInput {
            q: q("120 m3/h"),
            h: q("45 m"),
            n: q("2900 rpm"),
            rho: q("998.2 kg/m3"),
            eta_m: 0.95,
            npsha: None,
            nu: None,
        };
        let free = hm_design::design(&input).unwrap();
        assert!(free.model_volute.is_some());
        assert!(hm_design::designed_si(&free, "a360").is_some());

        let options = hm_design::DesignOptions {
            casing: Some(hm_design::CasingFit {
                d2: 0.210,
                neck_diameter: 0.090,
                eye_diameter_max: 0.075,
                bore_diameter: 0.020,
                hub_diameter: 0.030,
                b3: 0.030,
                cutwater_radius: 0.111,
                channels: 1,
                wrap_deg: None,
                throat_area: None,
                leakage: None,
            }),
            ..hm_design::DesignOptions::default()
        };
        let fitted = hm_design::design_with_options(&input, &options).unwrap();
        assert!(fitted.model_volute.is_none());
        for name in ["a360", "d360", "r4"] {
            assert_eq!(hm_design::designed_si(&fitted, name), None, "{name}");
        }
        assert!(hm_design::designed_si(&fitted, "b3").is_some());
    }

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

    #[test]
    fn export_has_the_report_format_and_eleven_points() {
        let json =
            export_report(&Inputs::default(), &Basis::default(), None, "sin título").unwrap();

        assert!(json.ends_with('\n'));
        let report: hm_engine::DesignReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report.format, hm_engine::REPORT_FORMAT);
        assert_eq!(report.curve.len(), 11);
    }

    #[test]
    fn export_equals_the_engine_report() {
        let inputs = Inputs::default();
        let basis = Basis::default();
        let json = export_report(&inputs, &basis, None, "sin título").unwrap();
        let built = build_project(&inputs, &basis, None, "sin título").unwrap();
        let points = CurvePoints::new(11).unwrap();
        let engine = report(&built.project, points).unwrap();

        let exported: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(exported, serde_json::to_value(&engine).unwrap());

        // Lo que la CLI leería del archivo guardado.
        let path = std::env::temp_dir().join(format!("hmflow-export-{}", std::process::id()));
        built.project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let from_file = report(&loaded, points).unwrap();
        assert_eq!(exported, serde_json::to_value(&from_file).unwrap());
    }
}
