//! Crear un proyecto y pedir su informe, sin imprimir ni guardar.

use hm_core::PumpType;
use hm_design::blade_camber::{BladeCamber, camber_for_design};
use hm_design::blade_edges::{BladeEdges, LeadingDims, LeadingKind};
use hm_design::blade_setup::{BladeFamily, BladeSetup, BladeShape, CamberBows, STREAMLINE_COUNTS};
use hm_design::blade_solid::{BladeSolid, blade_solid_for_design};
use hm_design::blade_thickness::{BladeThickness, EditedThickness, ThicknessCurve, ThicknessNode};
use hm_design::geometry::DEFAULT_INCIDENCE_DEG;
use hm_design::meridian::{
    CutMode, EdgeEdit, EdgeMode, HubCutEdit, MeridianEdits, MeridionalModel, WallEdit,
    meridional_model_with,
};
use hm_design::plan::{PlanModel, plan_model_with};
use hm_design::scene::{BladeSurfaceSource, PieceId, Scene, SceneNotice, scene_from};
use hm_design::{DesignInput, DesignReport, LEGACY_METHODS, METHOD, design_with_options};
use hm_formula::Registry;
use hm_project::{Casing, Param, Project, Suction};

use crate::analysis::{Analysis, CurvePoints, require_stages};
use crate::audit::{audit_with_analysis, schema_diagnostics};
use crate::case::{Case, DensitySource, Entered, FluidKind, Intake, Temperature, Viscosity};
use crate::convert::{ManualDefaults, case_from_project, casing_fit, design_params};
use crate::dispatch::analyze;
use crate::error::{CaseField, EngineError, Stage, input_error};
use crate::fluid::parse_temperature;
use crate::validate::{design_input, field_code};

/// Crea un proyecto a partir de un caso y un análisis ya calculado.
///
/// Escribe el texto original de cada entrada, `[suction]` si el NPSHa sale de
/// la instalación, el método actual y los parámetros del análisis. `rho` y
/// `eta_m` quedan siempre resueltos. No recalcula: quien llama ya decidió
/// guardar. Exige geometría, mecánica y viscosidad correctas.
pub fn new_project(name: &str, case: &Case, analysis: &Analysis) -> Result<Project, EngineError> {
    if case.design.casing.is_some() {
        return Err(casing_error(
            "este caso lleva carcasa: usa `new_casing_project`, o se perdería",
        ));
    }
    build_project(name, case, analysis, None)
}

/// Como [`new_project`], para un rodete en una carcasa existente. `casing` es
/// el texto escrito: se guarda tal cual, y tiene que dar los valores del caso.
pub fn new_casing_project(
    name: &str,
    case: &Case,
    analysis: &Analysis,
    casing: &Casing,
) -> Result<Project, EngineError> {
    if case.design.casing.as_ref() != Some(&casing_fit(casing)?) {
        return Err(casing_error(
            "el texto de la carcasa no es el del caso analizado",
        ));
    }
    build_project(name, case, analysis, Some(casing))
}

/// Convierte una selección `manual` en decisión: `locked`, con el mismo valor.
/// `recalc --reselect` la conserva. Otro modo no se toca.
pub fn lock_selection(project: &mut Project, name: &str) {
    if let Some(Param::Manual { value }) = project.params.get(name) {
        let value = value.clone();
        project
            .params
            .insert(name.to_string(), Param::Locked { value });
    }
}

fn casing_error(message: &str) -> EngineError {
    input_error(
        CaseField::Param("casing"),
        "HM-PROJ-030",
        format!("[casing] {message}"),
    )
}

fn build_project(
    name: &str,
    case: &Case,
    analysis: &Analysis,
    casing: Option<&Casing>,
) -> Result<Project, EngineError> {
    require_stages(analysis)?;
    check_case_matches_analysis(case, analysis)?;

    let mut project = Project::new(name, case.pump_type);
    project.casing = casing.cloned();
    project.operating.q = case.duty.q.text.clone();
    project.operating.h = case.duty.h.text.clone();
    project.operating.n = case.duty.n.text.clone();
    project.operating.fluid = fluid_name(&case.fluid.kind);
    project.operating.temperature = temperature_text(case.fluid.temperature.as_ref());
    project.operating.viscosity = viscosity_text(case.fluid.viscosity.as_ref());
    project.operating.npsha = npsha_text(&case.intake);
    project.suction = suction_from(&case.intake);
    project.design.method = METHOD.to_string();

    let defaults = ManualDefaults::with_options(
        rho_text(case, analysis),
        analysis.input.eta_m.to_string(),
        &case.design,
    );
    project.params = design_params(&analysis.result, None, &defaults)?.0;

    let validation = project.validate(&Registry::all());
    if validation.has_errors() {
        return Err(EngineError::Invalid {
            diagnostics: validation,
        });
    }
    // Lo que se escribe tiene que poder leerse: el diseño acepta valores que el
    // archivo no, como Z = 101.
    case_from_project(&project)?;
    Ok(project)
}

/// El caso que se escribe tiene que ser el que se analizó, y cada texto tiene
/// que dar su valor.
///
/// `Entered` guarda el texto y la cantidad por separado, y el análisis es un
/// valor aparte. Si no coinciden, el `.hmf` diría una cosa y el cálculo otra,
/// y `check` lo marcaría como `HM-PROJ-015`. Se detecta aquí, al escribir.
pub(crate) fn check_case_matches_analysis(
    case: &Case,
    analysis: &Analysis,
) -> Result<(), EngineError> {
    check_input_matches(case, &analysis.input)?;
    check_texts_match_values(case, analysis)?;

    // Las opciones no se ven en el análisis; sí en lo que dan.
    let from_options = design_with_options(&analysis.input, &case.design)?;
    if format!("{from_options:?}") != format!("{:?}", analysis.result) {
        return Err(input_error(
            CaseField::Param("opciones"),
            field_code(CaseField::Param("opciones")),
            "las opciones del caso no dan el diseño que se analizó",
        ));
    }
    Ok(())
}

/// La entrada que da el caso es la del análisis, campo por campo.
fn check_input_matches(case: &Case, analyzed: &DesignInput) -> Result<(), EngineError> {
    let rebuilt = design_input(case)?;
    let same_option = |a: &Option<hm_units::Quantity>, b: &Option<hm_units::Quantity>| match (a, b)
    {
        (Some(a), Some(b)) => same_value(a.si, b.si),
        (None, None) => true,
        _ => false,
    };
    let checks = [
        (CaseField::Q, same_value(rebuilt.q.si, analyzed.q.si)),
        (CaseField::H, same_value(rebuilt.h.si, analyzed.h.si)),
        (CaseField::N, same_value(rebuilt.n.si, analyzed.n.si)),
        (
            CaseField::Density,
            same_value(rebuilt.rho.si, analyzed.rho.si),
        ),
        (
            CaseField::MechanicalEfficiency,
            same_value(rebuilt.eta_m, analyzed.eta_m),
        ),
        (
            CaseField::Npsha,
            same_option(&rebuilt.npsha, &analyzed.npsha),
        ),
        (
            CaseField::Viscosity,
            match (rebuilt.nu, analyzed.nu) {
                (Some(a), Some(b)) => same_value(a, b),
                (None, None) => true,
                _ => false,
            },
        ),
    ];
    for (field, agrees) in checks {
        if !agrees {
            return Err(input_error(
                field,
                field_code(field),
                "el caso no es el que se analizó",
            ));
        }
    }
    Ok(())
}

/// Cada texto da la cantidad que lleva al lado.
fn check_texts_match_values(case: &Case, analysis: &Analysis) -> Result<(), EngineError> {
    let mut entries: Vec<(CaseField, &Entered)> = vec![
        (CaseField::Q, &case.duty.q),
        (CaseField::H, &case.duty.h),
        (CaseField::N, &case.duty.n),
    ];
    match &case.intake {
        Intake::None => {}
        Intake::Available(npsha) => entries.push((CaseField::Npsha, npsha)),
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => {
            entries.push((CaseField::Suction, pressure));
            for entered in [height, losses, vapor_pressure].into_iter().flatten() {
                entries.push((CaseField::Suction, entered));
            }
        }
    }
    if let Some(Viscosity::Kinematic(entered) | Viscosity::Dynamic(entered)) = &case.fluid.viscosity
    {
        entries.push((CaseField::Viscosity, entered));
    }
    for (field, entered) in entries {
        let agrees = Entered::parse(&entered.text)
            .is_ok_and(|parsed| same_value(parsed.quantity.si, entered.quantity.si));
        if !agrees {
            return Err(disagreement(field, &entered.text));
        }
    }

    if let Some(temperature) = &case.fluid.temperature {
        let agrees = parse_temperature(&temperature.text)
            .is_ok_and(|parsed| same_value(parsed.kelvin, temperature.kelvin));
        if !agrees {
            return Err(disagreement(CaseField::Temperature, &temperature.text));
        }
    }

    if let DensitySource::Explicit { text } = &case.fluid.density.source
        && !text.trim().is_empty()
    {
        let agrees = Entered::parse(text)
            .is_ok_and(|parsed| same_value(parsed.quantity.si, analysis.input.rho.si));
        if !agrees {
            return Err(disagreement(CaseField::Density, text));
        }
    }
    Ok(())
}

pub(crate) fn same_value(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs())
}

fn disagreement(field: CaseField, text: &str) -> EngineError {
    input_error(
        field,
        field_code(field),
        format!("el texto «{text}» no da el valor con el que se calculó"),
    )
}

/// Informe de un proyecto end-suction con el método actual.
///
/// El orden es el de hoy: validar, método y tipo, caso y análisis, etapas y
/// solo entonces la auditoría. Los avisos de la auditoría, con los del esquema
/// de parámetros, van al informe y no lo bloquean. No reimplementa el cálculo: es `case_from_project`, `analyze`
/// y `audit_with_analysis`.
pub fn report(project: &Project, curve_points: CurvePoints) -> Result<DesignReport, EngineError> {
    let mut diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        return Err(EngineError::Invalid { diagnostics });
    }
    ensure_reportable(project)?;

    let case = case_from_project(project)?;
    let analysis = analyze(&case, curve_points)?;
    let stages = require_stages(&analysis)?;
    diagnostics.extend(schema_diagnostics(project).into_vec());
    diagnostics.extend(audit_with_analysis(project, &analysis).into_vec());

    let mut report = DesignReport::new(
        &analysis.input,
        &analysis.result,
        curve_points.get(),
        diagnostics.into_vec(),
    )
    .with_geometry(stages.geometry)
    .with_mechanics(stages.mechanics);
    if let Some(correction) = stages.viscous {
        report = report.with_viscous(correction);
    }
    Ok(report)
}

/// Vista en planta de un proyecto, la misma que dibuja el visor.
///
/// Rechaza lo mismo que el informe, pero de las etapas solo exige la
/// geometría: es lo único que la planta necesita. Con una forma 3D en
/// `[blade]` dibuja la superficie media (meridiano del proyecto, líneas de
/// corriente y línea media de [`camber_for_design`], la misma cadena que usa
/// la interfaz); con una forma 2D, la línea media de siempre, también si hay
/// manijas 3D guardadas. Una línea media que falla es un error de la etapa de
/// geometría.
pub fn plan(project: &Project) -> Result<PlanModel, EngineError> {
    let analysis = drawable(project)?;
    let geometry_failed = |source| EngineError::Stage {
        stage: Stage::Geometry,
        source,
    };
    let geometry = analysis
        .geometry
        .as_ref()
        .map_err(|error| geometry_failed(error.clone()))?;
    let setup = blade_setup_of(project.blade.as_ref())
        .map_err(|message| geometry_failed(hm_design::DesignError::Input(message)))?;
    let camber = if setup.shape.family() == BladeFamily::ThreeD {
        // Un recorte del cubo que falle no cambia el perfil hidráulico: la
        // planta se arma con el meridiano sin recorte, como la dibuja el visor.
        let (model, _) = meridian_or_uncut(project, &analysis.result)?;
        let incidence_deg = case_from_project(project)?
            .design
            .incidence_deg
            .unwrap_or(DEFAULT_INCIDENCE_DEG);
        camber_for_design(
            &analysis.result,
            &geometry.impeller,
            &model,
            &setup,
            incidence_deg,
        )
        .map_err(geometry_failed)?
    } else {
        None
    };
    plan_model_with(&analysis.result, geometry, camber.as_ref()).map_err(|error| {
        geometry_failed(hm_design::DesignError::Input(format!(
            "vista en planta: {error}"
        )))
    })
}

/// Corte meridional de un proyecto, el mismo que dibuja la pestaña. Rechaza
/// lo mismo que el informe; solo necesita el diseño 1D.
pub fn meridian(project: &Project) -> Result<MeridionalModel, EngineError> {
    let analysis = drawable(project)?;
    let edits = meridian_edits(project);
    meridional_model_with(&analysis.result, &edits).map_err(|source| EngineError::Stage {
        stage: Stage::Geometry,
        source,
    })
}

/// El meridiano del proyecto; si falla por el recorte del cubo, el mismo sin
/// recorte, con el error del recorte. Un error que no es del recorte se
/// devuelve tal cual. Es lo que hace la interfaz al dibujar
/// (`model_or_uncut`), para que el visor y la exportación no se separen.
fn meridian_or_uncut(
    project: &Project,
    result: &hm_design::DesignResult,
) -> Result<(MeridionalModel, Option<String>), EngineError> {
    let edits = meridian_edits(project);
    let stage = |source| EngineError::Stage {
        stage: Stage::Geometry,
        source,
    };
    match meridional_model_with(result, &edits) {
        Ok(model) => Ok((model, None)),
        Err(cut_error) if edits.hub_cut.is_active() => {
            let plain = MeridianEdits {
                hub_cut: HubCutEdit::default(),
                ..edits
            };
            let model = meridional_model_with(result, &plain).map_err(stage)?;
            Ok((model, Some(cut_error.to_string())))
        }
        Err(source) => Err(stage(source)),
    }
}

/// Escena 3D de un proyecto, en mm, con la tolerancia de cuerda acotada a
/// [`SCENE_TOLERANCE_RANGE_MM`](hm_design::scene::SCENE_TOLERANCE_RANGE_MM).
///
/// Rechaza lo mismo que el meridiano: un proyecto que no pasa del 1D. Un
/// recorte del cubo que falla no es error: el cubo sale sin recorte y la
/// escena lleva el aviso. Con forma 3D, los álabes son la superficie media
/// de [`camber_for_design`], la misma cadena que la planta; si la línea media
/// falla, la escena sale sin álabes y con el motivo. Con forma 2D, la línea
/// media de siempre tendida entre cubo y tapa.
pub fn scene(project: &Project, tolerance: f64) -> Result<Scene, EngineError> {
    let analysis = drawable(project)?;
    let (model, cut_error) = meridian_or_uncut(project, &analysis.result)?;
    let camber = scene_camber(project, &analysis, &model);
    // Con línea media 3D, el álabe con espesor: sale por la misma cadena, o el
    // motivo, que la escena enseña como aviso y con las superficies medias.
    let solid = match &camber {
        Ok(Some(_)) => Some(scene_solid(project, &analysis, &model)),
        _ => None,
    };
    let blade = match (&camber, &analysis.geometry) {
        (Ok(Some(camber)), _) => BladeSurfaceSource::Solid {
            camber,
            solid: solid
                .as_ref()
                .expect("con línea media hay sólido o motivo")
                .as_ref()
                .map_err(Clone::clone),
        },
        (Ok(None), Ok(geometry)) => BladeSurfaceSource::Planar(&geometry.impeller.blade),
        (Ok(None), Err(error)) => {
            BladeSurfaceSource::Unavailable(format!("sin geometría no hay álabes: {error}"))
        }
        (Err(message), _) => BladeSurfaceSource::Unavailable(message.clone()),
    };
    let mut scene = scene_from(
        &model,
        blade,
        analysis.result.selection.blade_count,
        tolerance,
    );
    if let Some(error) = cut_error {
        scene.notices.insert(
            0,
            SceneNotice {
                piece: PieceId::Hub,
                message: format!(
                    "el recorte del cubo no se pudo aplicar ({error}); se dibuja el cubo sin recorte"
                ),
            },
        );
    }
    Ok(scene)
}

/// Los brazos de `[meridian]` en mm. Vacío o sin sección es automático. Un
/// texto que no se entienda ya lo rechazó `validate` (HM-PROJ-031).
#[must_use]
pub fn meridian_edits(project: &Project) -> MeridianEdits {
    project
        .meridian
        .as_ref()
        .map(meridian_edits_of)
        .unwrap_or_default()
}

/// La forma del álabe y las líneas de corriente de una sección `[blade]`; sin
/// sección, las de por defecto. Al cargar un archivo la sección ya viene
/// comprobada, pero una hecha a mano puede traer cualquier cosa: es un error
/// con el mismo texto que da la carga, no se corrige en silencio.
pub fn blade_setup_of(section: Option<&hm_project::Blade>) -> Result<BladeSetup, String> {
    let Some(section) = section else {
        return Ok(BladeSetup::default());
    };
    let shape = BladeShape::from_id(&section.shape).ok_or_else(|| {
        format!(
            "forma de álabe desconocida «{}»; las válidas son: {}",
            section.shape,
            BladeShape::ALL.map(BladeShape::id).join(", ")
        )
    })?;
    if !STREAMLINE_COUNTS.contains(&section.streamlines) {
        return Err(format!(
            "«streamlines» = {} no es válido; debe ser 3, 5, 7 o 9",
            section.streamlines
        ));
    }
    let (low, high) = hm_project::BLADE_BOW_RANGE_DEG;
    for (field, pair) in [
        ("bow_hub", section.bow_hub),
        ("bow_shroud", section.bow_shroud),
    ] {
        for value in pair {
            if !(low..=high).contains(&value) {
                return Err(format!(
                    "«{field}» = {value} no es válido; debe ser un número de {low} a {high} grados"
                ));
            }
        }
    }
    Ok(BladeSetup {
        shape,
        streamlines: section.streamlines,
        bows: CamberBows {
            hub: (section.bow_hub[0], section.bow_hub[1]),
            shroud: (section.bow_shroud[0], section.bow_shroud[1]),
        },
        thickness: thickness_of(section)?,
        edges: edges_of(section)?,
    })
}

/// Los bordes de una sección `[blade]`: mm → m. Lo que no cumpla es un error
/// con el texto de la carga; los topes que dependen de e y de L los acota el
/// diseño al construir el álabe. Con «cubo = tapa» la tapa copia el cubo.
fn edges_of(section: &hm_project::Blade) -> Result<BladeEdges, String> {
    if let Some(problem) = section.leading_layout_problem() {
        return Err(problem);
    }
    let kind = LeadingKind::from_id(&section.leading_edge)
        .ok_or_else(|| format!("borde de entrada desconocido «{}»", section.leading_edge))?;
    let to_dims = |file: &hm_project::LeadingDimsFile| LeadingDims {
        lp: file.lp / 1000.0,
        tip: file.tip.unwrap_or(0.0) / 1000.0,
        asymmetry: file.asymmetry.unwrap_or(0.0),
    };
    let Some(hub) = section.leading_hub.as_ref().map(to_dims) else {
        return Ok(BladeEdges::default());
    };
    let shroud = match (&section.leading_shroud, section.thickness_same) {
        (Some(file), false) => to_dims(file),
        _ => hub,
    };
    Ok(BladeEdges {
        leading: kind,
        leading_hub: hub,
        leading_shroud: shroud,
    })
}

/// La curva de espesor de una sección `[blade]`: mm → m, y cada lista pasa por
/// `ThicknessCurve::new`, cuyo texto es el error. Una sección hecha a mano que
/// no cumpla no se arregla: es un error.
fn thickness_of(section: &hm_project::Blade) -> Result<BladeThickness, String> {
    if let Some(problem) = section.thickness_layout_problem() {
        return Err(problem);
    }
    let to_nodes = |pairs: &[[f64; 2]]| -> Result<Vec<ThicknessNode>, String> {
        let nodes: Vec<ThicknessNode> = pairs
            .iter()
            .map(|&[x, e_mm]| ThicknessNode {
                x,
                e: e_mm / 1000.0,
            })
            .collect();
        ThicknessCurve::new(nodes.clone()).map_err(|error| error.to_string())?;
        Ok(nodes)
    };
    let edited = if section.thickness_hub.is_empty() {
        None
    } else {
        let hub = to_nodes(&section.thickness_hub)?;
        let shroud = if section.thickness_same {
            hub.clone()
        } else {
            to_nodes(&section.thickness_shroud)?
        };
        Some(EditedThickness { hub, shroud })
    };
    Ok(BladeThickness {
        same: section.thickness_same,
        edited,
    })
}

/// La sección `[blade]` de esa elección, o ninguna si es la de por defecto.
#[must_use]
pub fn blade_section_of(setup: &BladeSetup) -> Option<hm_project::Blade> {
    (*setup != BladeSetup::default()).then(|| hm_project::Blade {
        shape: setup.shape.id().to_string(),
        streamlines: setup.streamlines,
        bow_hub: [setup.bows.hub.0, setup.bows.hub.1],
        bow_shroud: [setup.bows.shroud.0, setup.bows.shroud.1],
        thickness_same: setup.thickness.same,
        thickness_hub: setup
            .thickness
            .edited
            .as_ref()
            .map(|edited| nodes_mm(&edited.hub))
            .unwrap_or_default(),
        // Con «cubo = tapa» solo se escribe el cubo.
        thickness_shroud: setup
            .thickness
            .edited
            .as_ref()
            .filter(|_| !setup.thickness.same)
            .map(|edited| nodes_mm(&edited.shroud))
            .unwrap_or_default(),
        leading_edge: setup.edges.leading.id().to_string(),
        // Romo no lleva cotas; con «cubo = tapa» solo se escribe el cubo.
        leading_hub: (setup.edges.leading != LeadingKind::Blunt)
            .then(|| leading_dims_mm(&setup.edges.leading, setup.edges.leading_hub)),
        leading_shroud: (setup.edges.leading != LeadingKind::Blunt && !setup.thickness.same)
            .then(|| leading_dims_mm(&setup.edges.leading, setup.edges.leading_shroud)),
    })
}

/// Las cotas de una pared m → mm, redondeadas a 6 decimales, con solo los
/// campos del tipo.
fn leading_dims_mm(kind: &LeadingKind, dims: LeadingDims) -> hm_project::LeadingDimsFile {
    let wedge = *kind == LeadingKind::Wedge;
    hm_project::LeadingDimsFile {
        lp: round6(dims.lp * 1000.0),
        tip: wedge.then(|| round6(dims.tip * 1000.0)),
        asymmetry: wedge.then(|| round6(dims.asymmetry)),
    }
}

fn round6(value: f64) -> f64 {
    (value * 1.0e6).round() / 1.0e6
}

/// Nodos de espesor m → mm, con x y e redondeados a 6 decimales.
fn nodes_mm(nodes: &[ThicknessNode]) -> Vec<[f64; 2]> {
    nodes
        .iter()
        .map(|node| [round6(node.x), round6(node.e * 1000.0)])
        .collect()
}

/// Lo editado de una sección `[meridian]`, en mm. Un texto vacío o que no
/// se entiende queda en automático (ya lo marcó `validate`, HM-PROJ-031).
#[must_use]
pub fn meridian_edits_of(section: &hm_project::Meridian) -> MeridianEdits {
    MeridianEdits {
        hub_diameter: length_mm(&section.hub_diameter).filter(|value| *value >= 0.0),
        edge_shroud: fraction(&section.edge_shroud),
        edge_hub: fraction(&section.edge_hub),
        edge_mode: if section.edge_shape.trim() == "line" {
            EdgeMode::Line
        } else {
            EdgeMode::Channel
        },
        edge: edge_edit_of(&section.edge),
        shroud: wall_edit_of(&section.shroud),
        hub: wall_edit_of(&section.hub),
        hub_cut: hub_cut_of(&section.hub_cut),
    }
}

/// Una longitud con unidad, en mm; `None` si está vacía o no se entiende.
fn length_mm(text: &str) -> Option<f64> {
    (!text.trim().is_empty())
        .then(|| hm_units::parse_quantity(text).ok())
        .flatten()
        .filter(|quantity| quantity.dim() == hm_units::Dimension::LENGTH && quantity.si.is_finite())
        .map(|quantity| quantity.si * 1000.0)
}

/// Una fracción de 0 a 0,9; `None` si está vacía o fuera de rango.
fn fraction(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| (0.0..=0.9).contains(value))
}

fn edge_edit_of(edge: &hm_project::MeridianEdge) -> EdgeEdit {
    let angle = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| (-90.0..=90.0).contains(value))
    };
    let arm = |text: &str| length_mm(text).filter(|value| *value > 0.0);
    EdgeEdit {
        p2_along: edge
            .p2_along
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| (0.0..=1.0).contains(value)),
        p2_offset: length_mm(&edge.p2_offset),
        start_angle: angle(&edge.start_angle_deg),
        start_length: arm(&edge.start_length),
        end_angle: angle(&edge.end_angle_deg),
        end_length: arm(&edge.end_length),
    }
}

/// El recorte del cubo en mm; una cota que no se entiende queda en `None`.
fn hub_cut_of(cut: &hm_project::MeridianHubCut) -> HubCutEdit {
    let size = |text: &str| length_mm(text).filter(|value| *value > 0.0);
    HubCutEdit {
        z: length_mm(&cut.z).filter(|value| *value >= 0.0),
        mode: if cut.mode.trim() == "through" {
            CutMode::Through
        } else {
            CutMode::Closed
        },
        shaft_diameter: size(&cut.shaft_diameter),
        counterbore_diameter: size(&cut.counterbore_diameter),
        counterbore_depth: size(&cut.counterbore_depth),
        chamfer: size(&cut.chamfer),
    }
}

fn wall_edit_of(wall: &hm_project::MeridianWall) -> WallEdit {
    let mm = length_mm;
    let angle = |text: &str| {
        text.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| (-90.0..=90.0).contains(value))
    };
    let arm = |text: &str| mm(text).filter(|value| *value > 0.0);
    let p2 = match (mm(&wall.p2_z), mm(&wall.p2_r)) {
        (Some(z), Some(r)) if r > 0.0 => Some([z, r]),
        _ => None,
    };
    WallEdit {
        p2,
        start_angle: angle(&wall.start_angle_deg),
        start_length: arm(&wall.start_length),
        end_angle: angle(&wall.end_angle_deg),
        end_length: arm(&wall.end_length),
    }
}

/// Las comprobaciones del informe y el análisis, para dibujar.
fn drawable(project: &Project) -> Result<Analysis, EngineError> {
    let diagnostics = project.validate(&Registry::all());
    if diagnostics.has_errors() {
        return Err(EngineError::Invalid { diagnostics });
    }
    ensure_reportable(project)?;
    let case = case_from_project(project)?;
    analyze(&case, CurvePoints::MINIMUM)
}

/// Tipo, método anterior y método desconocido. Un método vacío se admite.
fn ensure_reportable(project: &Project) -> Result<(), EngineError> {
    if project.design.pump_type != PumpType::EndSuction {
        return Err(EngineError::Unsupported(project.design.pump_type));
    }
    let method = project.design.method.as_str();
    if LEGACY_METHODS.contains(&method) {
        return Err(EngineError::LegacyMethod {
            method: method.to_string(),
        });
    }
    if !method.is_empty() && method != METHOD {
        return Err(EngineError::UnknownMethod {
            method: method.to_string(),
        });
    }
    Ok(())
}

/// El agua se escribe con su nombre canónico, `water`, sea cual sea la grafía
/// con que se pidió (`Water`, vacío): el proyecto la lee sin distinguir
/// mayúsculas. Otro fluido conserva su nombre.
pub(crate) fn fluid_name(kind: &FluidKind) -> String {
    match kind {
        FluidKind::Water => "water".to_string(),
        FluidKind::Custom(name) => name.clone(),
    }
}

pub(crate) fn temperature_text(temperature: Option<&Temperature>) -> String {
    temperature
        .map(|temperature| temperature.text.clone())
        .unwrap_or_default()
}

pub(crate) fn viscosity_text(viscosity: Option<&Viscosity>) -> String {
    match viscosity {
        Some(Viscosity::Kinematic(entered) | Viscosity::Dynamic(entered)) => entered.text.clone(),
        None => String::new(),
    }
}

/// El NPSHa de la instalación no se copia a `[operating]`: vive en `[suction]`.
pub(crate) fn npsha_text(intake: &Intake) -> String {
    match intake {
        Intake::Available(entered) => entered.text.clone(),
        Intake::None | Intake::Installation { .. } => String::new(),
    }
}

pub(crate) fn suction_from(intake: &Intake) -> Option<Suction> {
    match intake {
        Intake::Installation {
            pressure,
            height,
            losses,
            vapor_pressure,
        } => Some(Suction {
            pressure: pressure.text.clone(),
            height: entered_text(height),
            losses: entered_text(losses),
            vapor_pressure: entered_text(vapor_pressure),
        }),
        Intake::None | Intake::Available(_) => None,
    }
}

fn entered_text(entered: &Option<Entered>) -> String {
    entered
        .as_ref()
        .map(|entered| entered.text.clone())
        .unwrap_or_default()
}

/// El texto explícito se conserva. Si la densidad se derivó, queda con tres
/// decimales, como la CLI cuando no le pasan `--rho`.
pub(crate) fn rho_text(case: &Case, analysis: &Analysis) -> String {
    match &case.fluid.density.source {
        // Un texto vacío no es un `rho` escrito: se trata como derivado.
        DensitySource::Explicit { text } if !text.trim().is_empty() => text.clone(),
        DensitySource::Explicit { .. }
        | DensitySource::WaterAtTemperature
        | DensitySource::WaterAtDefaultTemperature
        | DensitySource::Fallback => format!("{:.3} kg/m3", analysis.input.rho.si),
    }
}

/// El álabe con espesor de la escena, por la misma cadena que la línea media
/// ([`blade_solid_for_design`]); el error pasa a texto.
fn scene_solid(
    project: &Project,
    analysis: &Analysis,
    model: &MeridionalModel,
) -> Result<BladeSolid, String> {
    let geometry = analysis
        .geometry
        .as_ref()
        .map_err(|error| format!("sin geometría no hay álabes: {error}"))?;
    let setup = blade_setup_of(project.blade.as_ref())?;
    let incidence_deg = case_from_project(project)
        .map_err(|error| error.to_string())?
        .design
        .incidence_deg
        .unwrap_or(DEFAULT_INCIDENCE_DEG);
    blade_solid_for_design(
        &analysis.result,
        &geometry.impeller,
        model,
        &setup,
        incidence_deg,
    )
    .map_err(|error| error.to_string())
}

/// La línea media 3D de la escena; `None` con forma 2D, o el motivo de que
/// no haya álabes.
fn scene_camber(
    project: &Project,
    analysis: &Analysis,
    model: &MeridionalModel,
) -> Result<Option<BladeCamber>, String> {
    let geometry = analysis
        .geometry
        .as_ref()
        .map_err(|error| format!("sin geometría no hay álabes: {error}"))?;
    let setup = blade_setup_of(project.blade.as_ref())?;
    if setup.shape.family() != BladeFamily::ThreeD {
        return Ok(None);
    }
    let incidence_deg = case_from_project(project)
        .map_err(|error| error.to_string())?
        .design
        .incidence_deg
        .unwrap_or(DEFAULT_INCIDENCE_DEG);
    camber_for_design(
        &analysis.result,
        &geometry.impeller,
        model,
        &setup,
        incidence_deg,
    )
    .map_err(|error| format!("la línea media del álabe no sale: {error}"))
}
