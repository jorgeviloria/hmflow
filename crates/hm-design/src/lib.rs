//! Diseño preliminar de bombas centrífugas end-suction OH1/OH2.
//!
//! El motor reproduce el ejemplo resuelto D1.A1 de Srinivasan 2017. Las
//! selecciones empíricas (psi, phi, beta2, número de álabes, velocidades de
//! entrada) están documentadas y se aplican por rango de `nq`. El tramo
//! 25–40 se ancla en D1.A1; los demás tramos son selecciones preliminares
//! pendientes de contraste experimental.

use std::collections::BTreeMap;

pub mod fluid;
pub mod geometry;
pub mod mechanics;
mod report;
mod viscous;

pub use fluid::{WATER_DENSITY_RANGE_C, water_density};
pub use report::{
    DesignReport, Measure, REPORT_FORMAT, REPORT_VERSION, ReportBladePoint, ReportGeometry,
    ReportHydraulics, ReportImpeller, ReportMechanics, ReportPoint, ReportSection, ReportViscous,
    ReportVoluteOutlet,
};
pub use viscous::{
    REFERENCE_WATER_NU, VISCOUS_EFFICIENCY_LOSS, VISCOUS_HEAD_LOSS, ViscousCorrection,
    viscous_correction, viscous_diagnostics,
};

use hm_core::{Diagnostic, Diagnostics};
use hm_formula::{FormulaError, Registry};
use hm_units::{Dimension, Quantity, Unit, UnitError, parse_unit};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DesignError {
    #[error(transparent)]
    Formula(#[from] FormulaError),

    #[error(transparent)]
    Unit(#[from] UnitError),

    #[error("entrada inválida: {0}")]
    Input(String),
}

/// Datos de entrada del diseño preliminar.
#[derive(Debug, Clone)]
pub struct DesignInput {
    pub q: Quantity,
    pub h: Quantity,
    pub n: Quantity,
    pub rho: Quantity,
    pub eta_m: f64,
    pub npsha: Option<Quantity>,
}

/// Selecciones empíricas aplicadas por el motor.
#[derive(Debug, Clone)]
pub struct DesignSelection {
    pub psi: f64,
    pub phi: f64,
    pub beta2_deg: f64,
    pub blade_count: u32,
    pub c0: f64,
    pub cm0: f64,
    pub a: f64,
    pub j: f64,
    pub margin: f64,
}

/// Resultado del diseño preliminar.
#[derive(Debug, Clone)]
pub struct DesignResult {
    pub nq: f64,
    pub specific_diameter_mm: f64,
    pub sigma: f64,
    pub selection: DesignSelection,
    pub d0: Quantity,
    pub d1: Quantity,
    pub b1: Quantity,
    pub u2: Quantity,
    pub d2: Quantity,
    pub qth: Quantity,
    pub cm2: Quantity,
    pub b2: Quantity,
    pub eta_h: f64,
    pub eta_v: f64,
    pub eta: f64,
    pub p_hyd: Quantity,
    pub p_shaft: Quantity,
    pub p_max: Quantity,
    pub p_hyd_loss: Quantity,
    pub p_vol_loss: Quantity,
    pub p_mech_loss: Quantity,
    pub torque: Quantity,
    pub torque_max: Quantity,
    pub npshr_design: Quantity,
    pub shutoff_head: Quantity,
    /// dz/d2 mínimo de la lengua según Gülich 2020, tabla 10.2.
    pub cutwater_min_ratio: f64,
    /// Velocidad periférica en el diámetro de entrada D1.
    pub u1: Quantity,
    /// Velocidad relativa a la entrada, en D1: `w1 = √(cm0² + u1²)`.
    pub w1: Quantity,
    /// Velocidad relativa a la salida: `w2 = √(cm2² + (u2 − cu2)²)`.
    pub w2: Quantity,
    /// Ángulo del álabe a la entrada, rad (`design.blade_inlet_angle`).
    pub beta1b: f64,
    /// Caudal de entrada sin choque QSF (Gülich ec. 6.17).
    pub shockless_flow: Quantity,
    /// Caudal con w1 = w1q, QW (Gülich ec. 6.18).
    pub throat_flow: Quantity,
    /// Caudal en que la voluta casa con el rodete, Qc (Gülich ec. 6.19).
    pub volute_matching_flow: Quantity,
    /// Qsa = min(Qc, (QSF + QW)/2), comienzo de la subida brusca del NPSH3
    /// (Gülich p. 342), como fracción del caudal de diseño.
    pub transition_ratio: f64,
    /// Inicio estimado de la recirculación a la entrada, fracción de Qd.
    pub recirculation_onset: f64,
    /// ηv por la fuga estimada de Gülich (ec. 3.10.3), para contraste.
    pub leakage_eta_v: f64,
    /// ηm estimado de Gülich (ec. 3.10.14), para contraste.
    pub mechanical_eta_estimate: f64,
    pub slip: f64,
    pub cu2: Quantity,
    pub volute: VoluteResult,
    pub diagnostics: Diagnostics,
}

/// Sección circular de la voluta en un ángulo dado.
#[derive(Debug, Clone)]
pub struct VoluteSection {
    pub angle_deg: f64,
    pub area: Quantity,
    pub diameter: Quantity,
    pub outer_radius: Quantity,
}

/// Geometría preliminar de la voluta.
#[derive(Debug, Clone)]
pub struct VoluteResult {
    pub base_radius: Quantity,
    pub width: Quantity,
    pub inlet_velocity: Quantity,
    pub throat_area: Quantity,
    pub throat_diameter: Quantity,
    pub outer_radius: Quantity,
    pub sections: Vec<VoluteSection>,
}

/// Punto de una curva preliminar de prestaciones.
#[derive(Debug, Clone)]
pub struct CurvePoint {
    pub q: Quantity,
    pub h: Quantity,
    pub eta: f64,
    pub p_shaft: Quantity,
    pub npshr: Quantity,
}

/// Especificación de un parámetro del diseño end-suction.
#[derive(Debug, Clone, Copy)]
pub struct ParamSpec {
    pub name: &'static str,
    pub unit: &'static str,
    pub formula: Option<&'static str>,
}

/// Método de diseño que implementa este motor, tal como se guarda en el
/// `.hmf`. Cambia cada vez que el mismo proyecto daría otros resultados.
///
/// - v0.1: selecciones por tablas, c2u con el deslizamiento sobre todo el
///   término, NPSHr con k = 1,5 y margen fijo de 0,5 m.
/// - v0.2: ψ y φ de Gülich, β2 por Euler, c2u de Gülich (tabla 3.2), NPSHr y
///   margen de Gülich (§6.3.3 y tabla 6.2), H0 de Gülich (3.28).
/// - v0.3: la transición del NPSHr a la subida brusca sale de la geometría
///   (Gülich ec. 6.17–6.20) en vez de suponerla en el caudal de diseño.
pub const METHOD: &str = "end_suction_v0.3";

/// Métodos anteriores que `recalc` sabe migrar a [`METHOD`].
pub const LEGACY_METHODS: &[&str] = &["end_suction_v0.1", "end_suction_v0.2"];

/// Parámetros que genera o consume el método [`METHOD`].
pub const PARAM_SPECS: &[ParamSpec] = &[
    ParamSpec {
        name: "rho",
        unit: "kg/m3",
        formula: None,
    },
    ParamSpec {
        name: "eta_m",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "mm",
        unit: "m",
        formula: None,
    },
    ParamSpec {
        name: "psi",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "phi",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "beta2",
        unit: "deg",
        formula: None,
    },
    ParamSpec {
        name: "blade_count",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "c0",
        unit: "m/s",
        formula: None,
    },
    ParamSpec {
        name: "cm0",
        unit: "m/s",
        formula: None,
    },
    ParamSpec {
        name: "A",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "J",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "margin",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "k3",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "volute_c",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "k_eff",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "slip",
        unit: "1",
        formula: Some("design.slip_factor"),
    },
    ParamSpec {
        name: "cu2",
        unit: "m/s",
        formula: Some("design.cu2"),
    },
    ParamSpec {
        name: "nq",
        unit: "1",
        formula: Some("similarity.nq"),
    },
    ParamSpec {
        name: "dn",
        unit: "1",
        formula: Some("design.specific_diameter"),
    },
    ParamSpec {
        name: "sigma",
        unit: "1",
        formula: Some("design.thoma_sigma"),
    },
    ParamSpec {
        name: "d0",
        unit: "m",
        formula: Some("design.d0"),
    },
    ParamSpec {
        name: "d1",
        unit: "m",
        formula: Some("design.d1"),
    },
    ParamSpec {
        name: "b1",
        unit: "m",
        formula: Some("design.b1"),
    },
    ParamSpec {
        name: "u2",
        unit: "m/s",
        formula: Some("design.u2"),
    },
    ParamSpec {
        name: "d2",
        unit: "m",
        formula: Some("design.d2"),
    },
    ParamSpec {
        name: "qth",
        unit: "m3/s",
        formula: Some("design.qth"),
    },
    ParamSpec {
        name: "cm2",
        unit: "m/s",
        formula: Some("design.cm2"),
    },
    ParamSpec {
        name: "b2",
        unit: "m",
        formula: Some("design.b2"),
    },
    ParamSpec {
        name: "eta_h",
        unit: "1",
        formula: Some("design.hydraulic_efficiency"),
    },
    ParamSpec {
        name: "eta_v",
        unit: "1",
        formula: Some("design.volumetric_efficiency"),
    },
    ParamSpec {
        name: "eta",
        unit: "1",
        formula: Some("design.overall_efficiency"),
    },
    ParamSpec {
        name: "p_hyd",
        unit: "W",
        formula: Some("perf.hydraulic_power"),
    },
    ParamSpec {
        name: "p_shaft",
        unit: "W",
        formula: Some("design.shaft_power"),
    },
    ParamSpec {
        name: "torque",
        unit: "N*m",
        formula: Some("mech.shaft_torque"),
    },
    ParamSpec {
        name: "p_max",
        unit: "W",
        formula: Some("design.max_power"),
    },
    ParamSpec {
        name: "p_hyd_loss",
        unit: "W",
        formula: Some("design.hydraulic_loss"),
    },
    ParamSpec {
        name: "p_vol_loss",
        unit: "W",
        formula: Some("design.volumetric_loss"),
    },
    ParamSpec {
        name: "p_mech_loss",
        unit: "W",
        formula: Some("design.mechanical_loss"),
    },
    ParamSpec {
        name: "npshr_d",
        unit: "m",
        formula: Some("design.npshr_design"),
    },
    ParamSpec {
        name: "h0",
        unit: "m",
        formula: Some("design.shutoff_head"),
    },
    ParamSpec {
        name: "r3",
        unit: "m",
        formula: Some("design.volute_base_radius"),
    },
    ParamSpec {
        name: "b3",
        unit: "m",
        formula: Some("design.volute_width"),
    },
    ParamSpec {
        name: "c3",
        unit: "m/s",
        formula: Some("design.volute_inlet_velocity"),
    },
    ParamSpec {
        name: "a360",
        unit: "m2",
        formula: Some("design.volute_throat_area"),
    },
    ParamSpec {
        name: "d360",
        unit: "m",
        formula: Some("design.volute_throat_diameter"),
    },
    ParamSpec {
        name: "r4",
        unit: "m",
        formula: Some("design.volute_outer_radius"),
    },
];

/// Busca la especificación de un parámetro del diseño.
pub fn param_spec(name: &str) -> Option<&'static ParamSpec> {
    PARAM_SPECS.iter().find(|spec| spec.name == name)
}

/// Factor de anchura de la voluta por defecto, `B3 = B2 + c·D2` (Srinivasan 2017, ec. 5.8).
pub const DEFAULT_VOLUTE_C: f64 = 0.05;

/// Factor de diámetro efectivo de la garganta por defecto (Srinivasan 2017, D2.2).
pub const DEFAULT_K_EFF: f64 = 1.1;

/// Selecciones que se pueden fijar desde el proyecto.
#[derive(Debug, Clone, Default)]
pub struct DesignOptions {
    pub psi: Option<f64>,
    pub phi: Option<f64>,
    pub beta2_rad: Option<f64>,
    pub blade_count: Option<u32>,
    pub c0: Option<f64>,
    pub cm0: Option<f64>,
    pub a: Option<f64>,
    pub j: Option<f64>,
    pub margin: Option<f64>,
    pub k3: Option<f64>,
    pub volute_width_factor: Option<f64>,
    pub k_eff: Option<f64>,
}

/// Intervalo de búsqueda de β2, en grados.
const BETA2_SEARCH_DEG: (f64, f64) = (1.0, 89.0);

/// Selecciones que dependen solo de `nq`; β2 queda sin fijar (NaN).
///
/// - ψ: Gülich 2020, ec. (3.26) con fT = 1,0.
/// - φ: el que da la anchura relativa media de Gülich, ec. (7.1), con ese ψ y
///   el ηv del motor. Un φ constante por tramos no puede seguir esa curva,
///   porque b2* crece con nq² a φ fijo.
/// - Z, c0 y cm0: tablas por tramos de nq.
fn base_selection(registry: &Registry, nq: f64) -> Result<DesignSelection, DesignError> {
    let one = Unit::dimensionless();
    let psi = evaluate(
        registry,
        "design.head_coefficient",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let eta_v = evaluate(
        registry,
        "design.volumetric_efficiency",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let b2_star = evaluate(
        registry,
        "design.relative_outlet_width",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let phi = evaluate(
        registry,
        "design.flow_coefficient",
        &[
            ("nq", quantity(nq, one.clone())),
            ("psi", quantity(psi, one.clone())),
            ("eta_v", quantity(eta_v, one.clone())),
            ("b2_star", quantity(b2_star, one)),
        ],
    )?
    .value();
    Ok(DesignSelection {
        psi,
        phi,
        beta2_deg: f64::NAN,
        blade_count: blade_count(nq),
        c0: inlet_velocity(nq),
        cm0: inlet_meridional_velocity(nq),
        a: 4.5,
        j: 1.03,
        margin: 1.1,
    })
}

/// Selecciones que el motor toma por defecto para `nq`, con β2 resuelto por
/// Euler para el rendimiento hidráulico `eta_h`.
pub fn table_selection(nq: f64, eta_h: f64) -> Result<DesignSelection, DesignError> {
    let registry = Registry::all();
    let mut selection = base_selection(&registry, nq)?;
    selection.beta2_deg = euler_blade_angle(
        &registry,
        selection.psi,
        selection.phi,
        eta_h,
        selection.blade_count,
    )?;
    Ok(selection)
}

/// Ángulo de salida del álabe β2, en grados, que hace que el rodete dé la
/// altura pedida (Gülich 2020, p. 412, paso 10).
///
/// Resuelve `design.cu2(γ(β2, Z), u2, cm2, β2) = design.euler_cu2` en forma
/// adimensional: con u2 = 1, cm2/u2 = φ y la altura que exige Euler,
/// `c2u/u2 = gH/(ηh·u2²) = ψ/(2ηh)`.
fn euler_blade_angle(
    registry: &Registry,
    psi: f64,
    phi: f64,
    eta_h: f64,
    blade_count: u32,
) -> Result<f64, DesignError> {
    let one = Unit::dimensionless();
    let ms = unit("m/s")?;
    let deg = unit("deg")?;
    let target = psi / (2.0 * eta_h);
    let cu2_over_u2 = |beta2_rad: f64| -> Result<f64, DesignError> {
        let beta2 = Quantity::from_si(beta2_rad, deg.clone());
        let slip = evaluate(
            registry,
            "design.slip_factor",
            &[
                ("beta2", beta2.clone()),
                ("Z", quantity(f64::from(blade_count), one.clone())),
            ],
        )?;
        Ok(evaluate(
            registry,
            "design.cu2",
            &[
                ("slip", slip),
                ("u2", Quantity::from_si(1.0, ms.clone())),
                ("cm2", Quantity::from_si(phi, ms.clone())),
                ("beta2", beta2),
            ],
        )?
        .si)
    };

    let (low, high) = (
        BETA2_SEARCH_DEG.0.to_radians(),
        BETA2_SEARCH_DEG.1.to_radians(),
    );
    let reachable = cu2_over_u2(high)?;
    if reachable < target {
        return Err(DesignError::Input(format!(
            "con ψ = {psi:.3}, φ = {phi:.3} y Z = {blade_count}, ningún β2 da la altura: \
             hace falta cu2/u2 = {target:.3} y el máximo es {reachable:.3}; aumenta Z o reduce ψ"
        )));
    }
    let root = hm_math::bisect(
        |beta2| cu2_over_u2(beta2).map_or(f64::NAN, |value| value - target),
        low,
        high,
        1e-10,
        200,
    )
    .map_err(|error| DesignError::Input(format!("β2 por Euler: {error}")))?;

    let residual = cu2_over_u2(root)? - target;
    if !residual.is_finite() || residual.abs() > 1e-6 {
        return Err(DesignError::Input(format!(
            "β2 por Euler no converge (residuo {residual:e})"
        )));
    }
    Ok(root.to_degrees())
}

/// Calcula un diseño preliminar end-suction.
pub fn design(input: &DesignInput) -> Result<DesignResult, DesignError> {
    design_with_options(input, &DesignOptions::default())
}

/// Calcula un diseño preliminar con selecciones explícitas.
pub fn design_with_options(
    input: &DesignInput,
    options: &DesignOptions,
) -> Result<DesignResult, DesignError> {
    validate_input(input)?;

    let registry = Registry::all();

    let m = unit("m")?;
    let m3s = unit("m3/s")?;
    let ms = unit("m/s")?;
    let w = unit("W")?;
    let nm = unit("N*m")?;
    let one = Unit::dimensionless();

    let nq = evaluate(
        &registry,
        "similarity.nq",
        &[
            ("n", input.n.clone()),
            ("Q", input.q.clone()),
            ("H", input.h.clone()),
        ],
    )?
    .value();

    let table = base_selection(&registry, nq)?;
    let manual_beta2 = options.beta2_rad.map(f64::to_degrees);
    let mut selection = DesignSelection {
        psi: options.psi.unwrap_or(table.psi),
        phi: options.phi.unwrap_or(table.phi),
        // Valor provisional para validar el resto; si no es manual, se
        // resuelve por Euler en cuanto se conoce ηh.
        beta2_deg: manual_beta2.unwrap_or(45.0),
        blade_count: options.blade_count.unwrap_or(table.blade_count),
        c0: options.c0.unwrap_or(table.c0),
        cm0: options.cm0.unwrap_or(table.cm0),
        a: options.a.unwrap_or(table.a),
        j: options.j.unwrap_or(table.j),
        margin: options.margin.unwrap_or(table.margin),
    };
    // R3 = k3·D2/2 (Srinivasan 2017, ec. 5.7) con k3 por defecto el dz/d2
    // mínimo de Gülich 2020, tabla 10.2, para pulsaciones de presión bajas.
    let cutwater_min_ratio = geometry::cutwater_min_ratio(&registry, input, nq)?;
    let k3_value = options.k3.unwrap_or(cutwater_min_ratio);
    let volute_width_value = options.volute_width_factor.unwrap_or(DEFAULT_VOLUTE_C);
    let k_eff_value = options.k_eff.unwrap_or(DEFAULT_K_EFF);
    validate_selection(&selection, k3_value, volute_width_value, k_eff_value)?;

    let specific_diameter_mm = evaluate(
        &registry,
        "design.specific_diameter",
        &[
            ("A", quantity(selection.a, one.clone())),
            ("Q", input.q.clone()),
            ("n", input.n.clone()),
            ("mm", Quantity::from_si(0.001, m.clone())),
        ],
    )?
    .value();

    let eta_h = evaluate(
        &registry,
        "design.hydraulic_efficiency",
        &[("Dn", quantity(specific_diameter_mm, one.clone()))],
    )?
    .value();

    if manual_beta2.is_none() {
        selection.beta2_deg = euler_blade_angle(
            &registry,
            selection.psi,
            selection.phi,
            eta_h,
            selection.blade_count,
        )?;
    }

    let eta_v = evaluate(
        &registry,
        "design.volumetric_efficiency",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();

    let eta = evaluate(
        &registry,
        "design.overall_efficiency",
        &[
            ("eta_h", quantity(eta_h, one.clone())),
            ("eta_v", quantity(eta_v, one.clone())),
            ("eta_m", quantity(input.eta_m, one.clone())),
        ],
    )?
    .value();

    let p_hyd = evaluate(
        &registry,
        "perf.hydraulic_power",
        &[
            ("rho", input.rho.clone()),
            ("Q", input.q.clone()),
            ("H", input.h.clone()),
        ],
    )?;

    let p_hyd_loss = evaluate(
        &registry,
        "design.hydraulic_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta_h", quantity(eta_h, one.clone())),
        ],
    )?;

    let p_vol_loss = evaluate(
        &registry,
        "design.volumetric_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta_h", quantity(eta_h, one.clone())),
            ("eta_v", quantity(eta_v, one.clone())),
        ],
    )?;

    let p_mech_loss = evaluate(
        &registry,
        "design.mechanical_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta_h", quantity(eta_h, one.clone())),
            ("eta_v", quantity(eta_v, one.clone())),
            ("eta_m", quantity(input.eta_m, one.clone())),
        ],
    )?;

    let qth = evaluate(
        &registry,
        "design.qth",
        &[
            ("Q", input.q.clone()),
            ("eta_v", quantity(eta_v, one.clone())),
        ],
    )?;

    let u2 = evaluate(
        &registry,
        "design.u2",
        &[
            ("H", input.h.clone()),
            ("psi", quantity(selection.psi, one.clone())),
        ],
    )?;

    let d2 = evaluate(
        &registry,
        "design.d2",
        &[
            ("H", input.h.clone()),
            ("n", input.n.clone()),
            ("psi", quantity(selection.psi, one.clone())),
        ],
    )?;

    let cm2 = evaluate(
        &registry,
        "design.cm2",
        &[
            ("phi", quantity(selection.phi, one.clone())),
            ("u2", u2.clone()),
        ],
    )?;

    let b2 = evaluate(
        &registry,
        "design.b2",
        &[
            ("Qth", qth.clone()),
            ("D2", d2.clone()),
            ("cm2", cm2.clone()),
        ],
    )?;

    let beta2 = Quantity::from_si(selection.beta2_deg.to_radians(), unit("deg")?);
    let slip = evaluate(
        &registry,
        "design.slip_factor",
        &[
            ("beta2", beta2.clone()),
            ("Z", quantity(selection.blade_count as f64, one.clone())),
        ],
    )?
    .value();
    let cu2 = evaluate(
        &registry,
        "design.cu2",
        &[
            ("slip", quantity(slip, one.clone())),
            ("u2", u2.clone()),
            ("cm2", cm2.clone()),
            ("beta2", beta2),
        ],
    )?;
    let cu2 = with_unit(cu2, &ms);

    let k3 = quantity(k3_value, one.clone());
    let volute_width_factor = quantity(volute_width_value, one.clone());
    let k_eff = quantity(k_eff_value, one.clone());

    let r3 = evaluate(
        &registry,
        "design.volute_base_radius",
        &[("D2", d2.clone()), ("k3", k3)],
    )?;
    let b3 = evaluate(
        &registry,
        "design.volute_width",
        &[
            ("B2", b2.clone()),
            ("D2", d2.clone()),
            ("c", volute_width_factor),
        ],
    )?;
    let c3 = evaluate(
        &registry,
        "design.volute_inlet_velocity",
        &[("cm2", cm2.clone()), ("cu2", cu2.clone())],
    )?;
    let a360 = evaluate(
        &registry,
        "design.volute_throat_area",
        &[("Q", input.q.clone()), ("C3", c3.clone())],
    )?;
    let d360 = evaluate(
        &registry,
        "design.volute_throat_diameter",
        &[("A360", a360.clone()), ("k_eff", k_eff)],
    )?;
    let r4 = evaluate(
        &registry,
        "design.volute_outer_radius",
        &[("R3", r3.clone()), ("D360", d360.clone())],
    )?;

    let m2 = Unit::new("m2", Dimension::AREA, 1.0);
    let mut sections = Vec::with_capacity(9);
    for index in 0..=8 {
        let angle_deg = 45.0 * index as f64;
        let area_si = a360.si * angle_deg / 360.0;
        let diameter_si = 1.1 * (4.0 * area_si / std::f64::consts::PI).sqrt();
        let outer_si = r3.si + diameter_si / 2.0;
        sections.push(VoluteSection {
            angle_deg,
            area: Quantity::from_si(area_si, m2.clone()),
            diameter: Quantity::from_si(diameter_si, m.clone()),
            outer_radius: Quantity::from_si(outer_si, m.clone()),
        });
    }

    let volute = VoluteResult {
        base_radius: with_unit(r3, &m),
        width: with_unit(b3, &m),
        inlet_velocity: with_unit(c3, &ms),
        throat_area: with_unit(a360, &m2),
        throat_diameter: with_unit(d360, &m),
        outer_radius: with_unit(r4, &m),
        sections,
    };

    let d0 = evaluate(
        &registry,
        "design.d0",
        &[
            ("Qth", qth.clone()),
            ("c0", quantity(selection.c0, ms.clone())),
        ],
    )?;

    let d1 = evaluate(
        &registry,
        "design.d1",
        &[
            ("D0", d0.clone()),
            ("J", quantity(selection.j, one.clone())),
        ],
    )?;

    let b1 = evaluate(
        &registry,
        "design.b1",
        &[
            ("Qth", qth.clone()),
            ("D1", d1.clone()),
            ("cm0", quantity(selection.cm0, ms.clone())),
        ],
    )?;

    let p_shaft = evaluate(
        &registry,
        "design.shaft_power",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta", quantity(eta, one.clone())),
        ],
    )?;

    let p_max = evaluate(
        &registry,
        "design.max_power",
        &[
            ("P_shaft", p_shaft.clone()),
            ("margin", quantity(selection.margin, one.clone())),
        ],
    )?;

    let torque = evaluate(
        &registry,
        "mech.shaft_torque",
        &[("P", p_shaft.clone()), ("n", input.n.clone())],
    )?;

    let torque_max = evaluate(
        &registry,
        "mech.shaft_torque",
        &[("P", p_max.clone()), ("n", input.n.clone())],
    )?;

    let sigma = evaluate(
        &registry,
        "design.thoma_sigma",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let npshr_design = evaluate(
        &registry,
        "design.npshr_design",
        &[
            ("sigma", quantity(sigma, one.clone())),
            ("H", input.h.clone()),
        ],
    )?;
    let shutoff_head = evaluate(
        &registry,
        "design.shutoff_head",
        &[("nq", quantity(nq, one.clone())), ("u2", u2.clone())],
    )?;
    let u1 = evaluate(
        &registry,
        "impeller.peripheral_speed",
        &[("d2", d1.clone()), ("n", input.n.clone())],
    )?;
    let w1 = Quantity::from_si(selection.cm0.hypot(u1.si), ms.clone());
    let w2 = Quantity::from_si(cm2.si.hypot(u2.si - cu2.si), ms.clone());

    // Transición del NPSH3 (Gülich §6.3.3, ec. 6.17–6.20), sin espesor de
    // álabe ni prerrotación y con el borde de ataque cilíndrico del álabe 2D.
    let deg = unit("deg")?;
    let m2 = unit("m2")?;
    let beta1b = evaluate(
        &registry,
        "design.blade_inlet_angle",
        &[
            ("c1m", quantity(selection.cm0, ms.clone())),
            ("u1", u1.clone()),
            (
                "i1",
                Quantity::from_si(geometry::DEFAULT_INCIDENCE_DEG.to_radians(), deg.clone()),
            ),
        ],
    )?
    .si;
    let beta1b_quantity = Quantity::from_si(beta1b, deg.clone());
    let eta_v_quantity = quantity(eta_v, one.clone());
    let shockless_flow = evaluate(
        &registry,
        "design.shockless_flow",
        &[
            ("d1", d1.clone()),
            ("b1", b1.clone()),
            ("beta1b", beta1b_quantity.clone()),
            ("u1", u1.clone()),
            ("eta_v", eta_v_quantity.clone()),
        ],
    )?;
    let inlet_area = std::f64::consts::PI * d1.si * b1.si;
    let throat_flow = evaluate(
        &registry,
        "design.throat_flow",
        &[
            ("A1", Quantity::from_si(inlet_area, m2.clone())),
            (
                "A1q",
                Quantity::from_si(inlet_area * beta1b.sin(), m2.clone()),
            ),
            ("u1m", u1.clone()),
            ("eta_v", eta_v_quantity.clone()),
        ],
    )?;
    let volute_matching_flow = evaluate(
        &registry,
        "design.volute_matching_flow",
        &[
            ("u2", u2.clone()),
            (
                "A2",
                Quantity::from_si(std::f64::consts::PI * d2.si * b2.si, m2.clone()),
            ),
            ("slip", quantity(slip, one.clone())),
            ("A3q", Quantity::from_si(volute.throat_area.si, m2.clone())),
            ("eta_v", eta_v_quantity),
            (
                "beta2",
                Quantity::from_si(selection.beta2_deg.to_radians(), deg.clone()),
            ),
        ],
    )?;
    let pressure_side_flow = 0.5 * (shockless_flow.si + throat_flow.si);
    let transition_ratio = volute_matching_flow.si.min(pressure_side_flow) / input.q.si;
    let recirculation_onset = evaluate(
        &registry,
        "design.recirculation_onset",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let leakage_eta_v = evaluate(
        &registry,
        "design.leakage_volumetric_efficiency",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let mechanical_eta_estimate = evaluate(
        &registry,
        "design.mechanical_efficiency_estimate",
        &[
            ("Q", input.q.clone()),
            ("n", input.n.clone()),
            ("Q_ref", quantity(1.0, m3s.clone())),
            ("n_ref", Quantity::from_si(1500.0 / 60.0, unit("1/s")?)),
        ],
    )?
    .value();

    let mut result = DesignResult {
        nq,
        specific_diameter_mm,
        sigma,
        selection,
        d0: with_unit(d0, &m),
        d1: with_unit(d1, &m),
        b1: with_unit(b1, &m),
        u2: with_unit(u2, &ms),
        d2: with_unit(d2, &m),
        qth: with_unit(qth, &m3s),
        cm2: with_unit(cm2, &ms),
        b2: with_unit(b2, &m),
        eta_h,
        eta_v,
        eta,
        p_hyd: with_unit(p_hyd, &w),
        p_shaft: with_unit(p_shaft, &w),
        p_max: with_unit(p_max, &w),
        p_hyd_loss: with_unit(p_hyd_loss, &w),
        p_vol_loss: with_unit(p_vol_loss, &w),
        p_mech_loss: with_unit(p_mech_loss, &w),
        torque: with_unit(torque, &nm),
        torque_max: with_unit(torque_max, &nm),
        npshr_design,
        shutoff_head: with_unit(shutoff_head, &m),
        cutwater_min_ratio,
        u1: with_unit(u1, &ms),
        w1,
        w2,
        beta1b,
        shockless_flow: with_unit(shockless_flow, &m3s),
        throat_flow: with_unit(throat_flow, &m3s),
        volute_matching_flow: with_unit(volute_matching_flow, &m3s),
        transition_ratio,
        recirculation_onset,
        leakage_eta_v,
        mechanical_eta_estimate,
        slip,
        cu2,
        volute,
        diagnostics: Diagnostics::new(),
    };

    result.diagnostics = diagnostics_for(&result, input);
    Ok(result)
}

/// Región de operación preferente de API 610: del 70 % al 120 % del caudal
/// de diseño. El margen de NPSH se exige dentro de ella.
pub const PREFERRED_OPERATING_REGION: (f64, f64) = (0.7, 1.2);

/// Extremos de la curva preliminar, en fracción del caudal de diseño.
pub const CURVE_RANGE: (f64, f64) = (0.3, 1.3);

/// Coeficiente de la velocidad absoluta en `NPSH3 = λc·c1m²/2g + λw·w1²/2g`
/// para entrada axial (Gülich 2020, ec. (6.10)–(6.11), p. 337).
pub const NPSH_LAMBDA_C: f64 = 1.1;

/// Coeficiente de la velocidad relativa para NPSH3 con entrada sin choque.
/// Gülich 2020, tabla D6.3, p. 338, da 0,1..0,3; se toma el valor central.
pub const NPSH_LAMBDA_W: f64 = 0.2;

/// Exponente de `λw ∝ (tan β1)^0,57` fuera del punto de diseño
/// (Gülich 2020, ec. (6.21), p. 343). Sin prerrotación, tan β1 ∝ Q.
pub const NPSH_LAMBDA_W_EXPONENT: f64 = 0.57;

/// Exponente de `NPSH3 ∝ Q^x` por encima del caudal de entrada sin choque.
/// Gülich 2020, ec. (6.22), p. 343, da x = 2..3; se toma 3, el extremo
/// conservador.
pub const NPSH_OVERLOAD_EXPONENT: f64 = 3.0;

/// NPSH3 por velocidades de entrada para la fracción `ratio = Q/Qd`, sin la
/// constante 1/2g (solo se usa como cociente).
fn npsh3_velocity_shape(result: &DesignResult, ratio: f64) -> f64 {
    let c1m = result.selection.cm0 * ratio;
    let u1 = result.u1.si;
    let lambda_w = NPSH_LAMBDA_W * ratio.powf(NPSH_LAMBDA_W_EXPONENT);
    NPSH_LAMBDA_C * c1m.powi(2) + lambda_w * (c1m.powi(2) + u1.powi(2))
}

/// NPSHr relativo al de diseño para una fracción `ratio = Q/Qd`.
///
/// La transición a la subida brusca es Qsa (`transition_ratio`), calculada
/// de la geometría con Gülich ec. (6.17)–(6.20):
///
/// - Hasta Qsa, la forma de `NPSH3 = λc·c1m²/2g + λw·w1²/2g` con
///   `c1m ∝ Q`, `w1² = c1m² + u1²` y `λw ∝ (Q/Qd)^0,57` (ec. (6.10) y
///   (6.21)). El NPSH3 baja al reducir el caudal, como en la fig. 6.23 para
///   nq bajos y medios.
/// - Por encima, `NPSH(Qsa)·(Q/Qsa)^3` (ec. (6.22)).
///
/// La parte plana vale 1 en Qd: `npshr_design` es su nivel. Si Qsa < Qd, el
/// punto de diseño ya está en la subida y su NPSHr supera `npshr_design`.
pub fn npshr_ratio(result: &DesignResult, ratio: f64) -> f64 {
    let transition = result.transition_ratio;
    let flat = |q: f64| npsh3_velocity_shape(result, q) / npsh3_velocity_shape(result, 1.0);
    if ratio <= transition {
        flat(ratio)
    } else {
        flat(transition) * (ratio / transition).powf(NPSH_OVERLOAD_EXPONENT)
    }
}

/// NPSH disponible necesario según Gülich 2020, tabla 6.2, p. 379.
///
/// - Siempre: `NPSHA ≥ max(1,25·NPSH3; NPSH3 + 0,6 m)`.
/// - Con 10 ≤ u1 ≤ 50 m/s (agua): `σA ≥ σ3 + FR·FF·{0,05 + 2(u1/100)³}`, con
///   `σ = 2g·NPSH/u1²` (ec. (6.5)), FF = 1 (agua) y FR = 1 entre 0,8 y
///   1,0 Qd o 1,2 fuera (caudal parcial, o Q > QSF con QSF = Qd).
///
/// Por encima de 50 m/s la tabla pide experiencia o datos de cavidad; se
/// aplica solo el mínimo y `npsh_diagnostics` lo avisa.
pub fn required_npsha(npsh3: f64, u1: f64, ratio: f64) -> f64 {
    let minimum = (1.25 * npsh3).max(npsh3 + 0.6);
    if !(10.0..=50.0).contains(&u1) {
        return minimum;
    }
    // La tabla da FR = 1 entre 0,8 y 1,1 Qd y 1,2 con Q > QSF. Como se supone
    // entrada sin choque en Qd (QSF = Qd), por encima de Qd manda 1,2.
    let risk = if ratio > 0.8 && ratio <= 1.0 {
        1.0
    } else {
        1.2
    };
    let fluid = 1.0;
    let g = 9.806_65;
    let extra = risk * fluid * (0.05 + 2.0 * (u1 / 100.0).powi(3)) * u1.powi(2) / (2.0 * g);
    (npsh3 + extra).max(minimum)
}

/// Media de la anchura relativa de salida `b2* = b2/d2` de Gülich 2020,
/// ec. (7.1), p. 412.
pub fn gulich_b2_star(nq: f64) -> f64 {
    let x = nq / 100.0;
    0.017 + 0.262 * x - 0.08 * x.powi(2) + 0.0093 * x.powi(3)
}

/// Intervalo admitido para `b2*/b2*(ec. 7.1)`.
///
/// La fig. 7.2 de Gülich muestra una banda de unos ±20 % alrededor de la
/// media, leída a ojo. D1.A1, un diseño publicado, queda un 30 % por debajo,
/// así que se usa un intervalo más ancho para no marcar diseños válidos.
pub const B2_STAR_BAND: (f64, f64) = (0.6, 1.4);

/// Intervalo habitual del ángulo de salida del álabe, en grados
/// (Gülich 2020, p. 412).
pub const BETA2_RANGE_DEG: (f64, f64) = (15.0, 45.0);

/// Subida mínima deseada `H0/Hd` con nq < 40 (Gülich 2020, §7.1.2, p. 403).
pub const MIN_HEAD_RISE: f64 = 1.2;

/// Diferencia admitida entre la altura de Euler y la pedida cuando β2 se
/// fija a mano. Con β2 calculado por Euler la diferencia es nula.
pub const EULER_HEAD_TOLERANCE: f64 = 0.03;

/// Qsa/Qd mínimo sin aviso: Gülich (p. 411) quiere la entrada sin choque
/// cerca o algo por encima del punto de diseño; el 5 % de tolerancia es del
/// motor.
pub const MIN_TRANSITION_RATIO: f64 = 0.95;

/// Dispersión del inicio de la recirculación según Gülich 2020, p. 237.
pub const RECIRCULATION_SCATTER: f64 = 0.1;

/// nq por encima del cual el álabe cilíndrico de la geometría es solo una
/// simplificación (Gülich 2020, p. 413: β2B variable a lo ancho por encima de
/// nq ≈ 40..60; se toma el extremo inferior).
pub const CYLINDRICAL_BLADE_MAX_NQ: f64 = 40.0;

/// Deceleración mínima `w2/w1` (criterio de de Haller; Gülich 2020, p. 413).
pub const DE_HALLER_MIN: f64 = 0.7;

/// Altura de la curva preliminar en `ratio` veces el caudal de diseño, m:
/// parábola de H0 al punto de diseño.
pub fn curve_head(input: &DesignInput, result: &DesignResult, ratio: f64) -> f64 {
    let h_design = input.h.si;
    let shutoff_ratio = result.shutoff_head.si / h_design;
    h_design * (shutoff_ratio - (shutoff_ratio - 1.0) * ratio.powi(2))
}

/// Curva preliminar de prestaciones entre 0.3 y 1.3 veces el caudal de diseño.
///
/// Es un modelo fenomenológico anclado en el punto de diseño, no un modelo de
/// pérdidas detallado. La forma de H(Q) y η(Q) se calibrará con datos medidos.
pub fn performance_curve(
    input: &DesignInput,
    result: &DesignResult,
    points: usize,
) -> Vec<CurvePoint> {
    let count = points.clamp(2, 1000);
    let q_design = input.q.si;
    let eta_design = result.eta;
    let rho = input.rho.si;
    let g = 9.806_65;
    let npshr_design = result.npshr_design.si;

    let m = Unit::new("m", Dimension::LENGTH, 1.0);
    let m3h = Unit::new("m3/h", Dimension::VOLUME.div(Dimension::TIME), 1.0 / 3600.0);
    let kw = Unit::new("kW", Dimension::POWER, 1000.0);

    let mut curve = Vec::with_capacity(count);

    for index in 0..count {
        let ratio =
            CURVE_RANGE.0 + (CURVE_RANGE.1 - CURVE_RANGE.0) * index as f64 / (count - 1) as f64;
        let q = q_design * ratio;
        let h = curve_head(input, result, ratio);
        let eta = eta_design
            * if ratio <= 1.0 {
                1.0 - 0.60 * (ratio - 1.0).powi(2)
            } else {
                1.0 - 0.35 * (ratio - 1.0).powi(2)
            };
        let eta = eta.max(1e-4);
        let p_shaft = rho * g * q * h / eta;
        let npshr = npshr_design * npshr_ratio(result, ratio);

        curve.push(CurvePoint {
            q: Quantity::from_si(q, m3h.clone()),
            h: Quantity::from_si(h, m.clone()),
            eta,
            p_shaft: Quantity::from_si(p_shaft, kw.clone()),
            npshr: Quantity::from_si(npshr, m.clone()),
        });
    }

    curve
}

/// Valor en SI que el diseño asigna a un parámetro `auto` conocido.
///
/// `nq`, `dn`, `sigma` y las eficiencias son números adimensionales. El resto
/// usa la unidad SI de su magnitud.
pub fn designed_si(result: &DesignResult, name: &str) -> Option<f64> {
    Some(match name {
        "nq" => result.nq,
        "dn" => result.specific_diameter_mm,
        "sigma" => result.sigma,
        "d0" => result.d0.si,
        "d1" => result.d1.si,
        "b1" => result.b1.si,
        "u2" => result.u2.si,
        "d2" => result.d2.si,
        "qth" => result.qth.si,
        "cm2" => result.cm2.si,
        "b2" => result.b2.si,
        "eta_h" => result.eta_h,
        "eta_v" => result.eta_v,
        "eta" => result.eta,
        "p_hyd" => result.p_hyd.si,
        "p_shaft" => result.p_shaft.si,
        "p_max" => result.p_max.si,
        "p_hyd_loss" => result.p_hyd_loss.si,
        "p_vol_loss" => result.p_vol_loss.si,
        "p_mech_loss" => result.p_mech_loss.si,
        "torque" => result.torque.si,
        "slip" => result.slip,
        "cu2" => result.cu2.si,
        "npshr_d" => result.npshr_design.si,
        "h0" => result.shutoff_head.si,
        "r3" => result.volute.base_radius.si,
        "b3" => result.volute.width.si,
        "c3" => result.volute.inlet_velocity.si,
        "a360" => result.volute.throat_area.si,
        "d360" => result.volute.throat_diameter.si,
        "r4" => result.volute.outer_radius.si,
        _ => return None,
    })
}

/// Acepta el redondeo con el que se escriben los `auto` y rechaza un valor
/// sustituido a mano.
pub fn stored_si_matches(expected: f64, actual: f64) -> bool {
    if !expected.is_finite() || !actual.is_finite() {
        return false;
    }
    let scale = expected.abs().max(1e-9);
    (expected - actual).abs() <= 1e-4 * scale
}

fn diagnostics_for(result: &DesignResult, input: &DesignInput) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();

    if !(10.0..=100.0).contains(&result.nq) {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-001",
                format!("nq = {:.1} fuera del rango preliminar 10..100", result.nq),
            )
            .with_hint("El diseño preliminar se calibró para bombas centrífugas end-suction."),
        );
    }

    if result.u2.si > 60.0 {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-002",
                format!(
                    "u2 = {:.1} m/s supera el límite preliminar de 60 m/s",
                    result.u2.si
                ),
            )
            .with_hint("Revisa el coeficiente de altura o la velocidad de giro."),
        );
    }

    let width_ratio = result.b2.si / result.d2.si;
    let reference_width = gulich_b2_star(result.nq);
    let (low, high) = B2_STAR_BAND;
    if !(low * reference_width..=high * reference_width).contains(&width_ratio) {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-003",
                format!(
                    "B2/D2 = {width_ratio:.3} lejos de la media de Gülich para nq = {:.1} \
                     ({reference_width:.3}; se admite {:.3}..{:.3})",
                    result.nq,
                    low * reference_width,
                    high * reference_width
                ),
            )
            .with_hint(
                "Revisa el coeficiente de caudal y el diámetro exterior (Gülich 2020, ec. 7.1).",
            ),
        );
    }

    let (beta_low, beta_high) = BETA2_RANGE_DEG;
    if !(beta_low..=beta_high).contains(&result.selection.beta2_deg) {
        diagnostics.push(Diagnostic::warning(
            "HM-DES-004",
            format!(
                "β2 = {:.1}° fuera del rango habitual {beta_low}..{beta_high}° (Gülich 2020, p. 412)",
                result.selection.beta2_deg
            ),
        ));
    }

    let head_rise = result.shutoff_head.si / input.h.si;
    if head_rise < 1.0 {
        diagnostics.push(
            Diagnostic::error(
                "HM-DES-016",
                format!(
                    "H0/Hd = {head_rise:.2}: la altura a caudal cero queda por debajo de la de diseño \
                     y la curva H(Q) subiría con el caudal"
                ),
            )
            .with_hint(
                "ψ es mayor que ψo = 1,25·e^(−0,3·nq/100) (Gülich ec. 3.28); reduce el coeficiente de altura.",
            ),
        );
    } else if result.nq < 40.0 && head_rise < MIN_HEAD_RISE {
        // Las correlaciones medias de Gülich (3.26) y (3.28) ya dan
        // H0/Hd = 1,08..1,24 con nq < 40: es informativo, no un defecto.
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-017",
                format!(
                    "H0/Hd = {head_rise:.2}, por debajo del {MIN_HEAD_RISE} que Gülich recomienda con \
                     nq < 40 (p. 403): curva Q-H plana"
                ),
            )
            .with_hint("Si hace falta una curva más estable, reduce ψ o estrecha la voluta."),
        );
    }

    let g = 9.806_65;
    let euler_head = result.eta_h * result.u2.si * result.cu2.si / g;
    let head_error = euler_head / input.h.si - 1.0;
    if head_error.abs() > EULER_HEAD_TOLERANCE {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-019",
                format!(
                    "con β2 = {:.1}°, Euler da H = ηh·u2·cu2/g = {euler_head:.2} m, un {:+.1} % \
                     respecto a la altura pedida",
                    result.selection.beta2_deg,
                    100.0 * head_error
                ),
            )
            .with_hint(
                "Deja β2 sin fijar para que se calcule por Euler, o ajusta ψ, φ o Z \
                 (recalc --reselect lo vuelve a calcular).",
            ),
        );
    }

    let k3 = result.volute.base_radius.si / (result.d2.si / 2.0);
    if k3 < result.cutwater_min_ratio - 1e-9 {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-023",
                format!(
                    "lengua de la voluta en dz/d2 = {k3:.3}, por debajo del {:.3} de Gülich \
                     (tabla 10.2): riesgo de pulsaciones de presión",
                    result.cutwater_min_ratio
                ),
            )
            .with_hint("Aumenta k3 o déjalo sin fijar para que tome el valor de Gülich."),
        );
    }

    if result.transition_ratio < MIN_TRANSITION_RATIO {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-025",
                format!(
                    "el NPSH3 empieza a subir con fuerza en {:.2}·Qd, antes del punto de diseño \
                     (Gülich 2020, §6.3.3): QSF = {:.2}·Qd y Qc = {:.2}·Qd",
                    result.transition_ratio,
                    result.shockless_flow.si / input.q.si,
                    result.volute_matching_flow.si / input.q.si
                ),
            )
            .with_hint(
                "Con Qc bajo, agranda la garganta de la voluta; con QSF bajo, sube β1B o el \
                 área de entrada.",
            ),
        );
    }

    let (por_low, _) = PREFERRED_OPERATING_REGION;
    if result.recirculation_onset + RECIRCULATION_SCATTER >= por_low {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-026",
                format!(
                    "la recirculación a la entrada puede empezar en {:.2}·Qd (±{RECIRCULATION_SCATTER}), \
                     dentro de la región preferente desde {por_low}·Qd (Gülich 2020, p. 237)",
                    result.recirculation_onset
                ),
            )
            .with_hint("Evita operar por debajo de ese caudal o confírmalo con ensayo."),
        );
    }

    if result.nq > CYLINDRICAL_BLADE_MAX_NQ {
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-024",
                format!(
                    "con nq = {:.1} el álabe cilíndrico (2D) de la geometría es una simplificación: \
                     por encima de nq ≈ 40..60 β2B debería variar a lo ancho (Gülich 2020, p. 413)",
                    result.nq
                ),
            )
            .with_hint("Usa la línea media del informe solo como punto de partida del álabe 3D."),
        );
    }

    let deceleration = result.w2.si / result.w1.si;
    if deceleration < DE_HALLER_MIN {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-014",
                format!(
                    "w2/w1 = {deceleration:.2} por debajo de {DE_HALLER_MIN} (criterio de de Haller): \
                     riesgo de desprendimiento en el rodete"
                ),
            )
            .with_hint("Reduce la deceleración: revisa β2, el diámetro de entrada o el coeficiente de caudal."),
        );
    }

    if !(4..=12).contains(&result.selection.blade_count) {
        diagnostics.push(Diagnostic::warning(
            "HM-DES-005",
            format!(
                "Z = {} fuera del rango preliminar 4..12",
                result.selection.blade_count
            ),
        ));
    }

    match &input.npsha {
        Some(npsha) => npsh_diagnostics(&mut diagnostics, npsha.si, result, input),
        None => diagnostics.push(
            Diagnostic::warning(
                "HM-DES-012",
                "NPSHa no indicado; no se evaluó el margen de cavitación",
            )
            .with_hint("Indica el NPSH disponible de la instalación (--npsha en design, npsha en [operating])."),
        ),
    }

    if result.cu2.si <= 0.0 {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-008",
                "la componente tangencial cu2 no es positiva; revisa β2 y el caudal",
            )
            .with_hint("El triángulo de velocidades no cierra para este punto de diseño."),
        );
    }

    diagnostics
}

/// Peor margen `NPSHa − NPSHA necesario` entre dos fracciones del caudal de
/// diseño.
///
/// Devuelve el margen en metros, el caudal en m3/h y el NPSHA necesario.
fn worst_npsh_margin(
    npsha: f64,
    result: &DesignResult,
    q_design: f64,
    from: f64,
    to: f64,
) -> (f64, f64, f64) {
    const STEPS: usize = 100;
    (0..=STEPS)
        .map(|index| from + (to - from) * index as f64 / STEPS as f64)
        .map(|ratio| {
            let npsh3 = result.npshr_design.si * npshr_ratio(result, ratio);
            let required = required_npsha(npsh3, result.u1.si, ratio);
            (npsha - required, q_design * ratio * 3600.0, required)
        })
        .fold((f64::INFINITY, 0.0, 0.0), |worst, candidate| {
            if candidate.0 < worst.0 {
                candidate
            } else {
                worst
            }
        })
}

fn npsh_diagnostics(
    diagnostics: &mut Diagnostics,
    npsha: f64,
    result: &DesignResult,
    input: &DesignInput,
) {
    let q_design = input.q.si;
    let (por_low, por_high) = PREFERRED_OPERATING_REGION;

    if result.u1.si > 50.0 {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-015",
                format!(
                    "u1 = {:.1} m/s supera 50 m/s: la tabla 6.2 de Gülich pide experiencia o \
                     datos de cavidad; solo se aplicó el margen mínimo",
                    result.u1.si
                ),
            )
            .with_hint("Contrasta el NPSHA con datos del rodete o de una bomba semejante."),
        );
    }

    let (margin, flow, required) = worst_npsh_margin(npsha, result, q_design, por_low, por_high);
    if margin < 0.0 {
        diagnostics.push(
            Diagnostic::error(
                "HM-DES-007",
                format!(
                    "NPSHa = {npsha:.2} m no llega al necesario {required:.2} m a Q = {flow:.0} m3/h \
                     dentro de la región preferente ({por_low}..{por_high} Qd; Gülich 2020, tabla 6.2)"
                ),
            )
            .with_hint("Aumenta el NPSHa o reduce la velocidad de giro."),
        );
        return;
    }
    diagnostics.push(Diagnostic::info(
        "HM-DES-006",
        format!(
            "NPSHa supera el necesario (Gülich 2020, tabla 6.2) en al menos {margin:.2} m \
             en la región preferente ({por_low}..{por_high} Qd)"
        ),
    ));

    let (low, high) = CURVE_RANGE;
    let below = worst_npsh_margin(npsha, result, q_design, low, por_low);
    let above = worst_npsh_margin(npsha, result, q_design, por_high, high);
    let (margin, flow, required) = if below.0 <= above.0 { below } else { above };
    if margin < 0.0 {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-011",
                format!(
                    "fuera de la región preferente el NPSHa no llega al necesario {required:.2} m \
                     a Q = {flow:.0} m3/h"
                ),
            )
            .with_hint("Evita operar ahí o confirma el NPSHr con el fabricante."),
        );
    }
}

/// Rechaza selecciones fuera de su dominio físico.
fn validate_selection(
    selection: &DesignSelection,
    k3: f64,
    volute_width_factor: f64,
    k_eff: f64,
) -> Result<(), DesignError> {
    let checks: [(&str, f64, bool, &str); 12] = [
        ("psi", selection.psi, selection.psi > 0.0, "mayor que 0"),
        ("phi", selection.phi, selection.phi > 0.0, "mayor que 0"),
        (
            "beta2",
            selection.beta2_deg,
            selection.beta2_deg > 0.0 && selection.beta2_deg < 90.0,
            "entre 0 y 90 deg",
        ),
        (
            "blade_count",
            f64::from(selection.blade_count),
            selection.blade_count >= 1,
            "al menos 1",
        ),
        ("c0", selection.c0, selection.c0 > 0.0, "mayor que 0 m/s"),
        ("cm0", selection.cm0, selection.cm0 > 0.0, "mayor que 0 m/s"),
        ("A", selection.a, selection.a > 0.0, "mayor que 0"),
        ("J", selection.j, selection.j > 0.0, "mayor que 0"),
        (
            "margin",
            selection.margin,
            selection.margin >= 1.0,
            "al menos 1 (P_max no puede ser menor que P_eje)",
        ),
        (
            "k3",
            k3,
            k3 > 1.0,
            "mayor que 1 (la voluta empieza fuera del rodete)",
        ),
        (
            "volute_c",
            volute_width_factor,
            volute_width_factor > 0.0,
            "mayor que 0",
        ),
        (
            "k_eff",
            k_eff,
            k_eff >= 1.0,
            "al menos 1 (la sección no puede ser menor que el área de paso)",
        ),
    ];

    for (name, value, valid, requirement) in checks {
        if !value.is_finite() || !valid {
            return Err(DesignError::Input(format!(
                "`{name}` = {value} no es válido; debe ser {requirement}"
            )));
        }
    }
    Ok(())
}

fn validate_input(input: &DesignInput) -> Result<(), DesignError> {
    require_dimension("Q", &input.q, "m3/s")?;
    require_dimension("H", &input.h, "m")?;
    require_dimension("n", &input.n, "1/s")?;
    require_dimension("rho", &input.rho, "kg/m3")?;

    if let Some(npsha) = &input.npsha {
        require_dimension("NPSHa", npsha, "m")?;
        if !npsha.si.is_finite() {
            return Err(DesignError::Input("NPSHa no es finito".to_string()));
        }
    }

    for (name, value) in [
        ("Q", input.q.si),
        ("H", input.h.si),
        ("n", input.n.si),
        ("rho", input.rho.si),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(DesignError::Input(format!(
                "{name} debe ser finito y positivo"
            )));
        }
    }

    if !input.eta_m.is_finite() || input.eta_m <= 0.0 || input.eta_m >= 1.0 {
        return Err(DesignError::Input(
            "la eficiencia mecánica debe ser mayor que 0 y menor que 1".to_string(),
        ));
    }

    Ok(())
}

fn require_dimension(name: &str, quantity: &Quantity, unit_text: &str) -> Result<(), DesignError> {
    let expected = parse_unit(unit_text)?;
    if quantity.dim() != expected.dim {
        return Err(DesignError::Input(format!(
            "{name} debe tener dimensiones de {unit_text}"
        )));
    }
    Ok(())
}

fn unit(text: &str) -> Result<Unit, DesignError> {
    Ok(parse_unit(text)?)
}

fn quantity(value: f64, unit: Unit) -> Quantity {
    Quantity::from_si(value, unit)
}

fn with_unit(quantity: Quantity, unit: &Unit) -> Quantity {
    Quantity::from_si(quantity.si, unit.clone())
}

fn evaluate(
    registry: &Registry,
    id: &str,
    pairs: &[(&str, Quantity)],
) -> Result<Quantity, DesignError> {
    let inputs: BTreeMap<String, Quantity> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Ok(registry.evaluate(id, &inputs)?)
}

/// Número de álabes.
fn blade_count(nq: f64) -> u32 {
    if nq < 25.0 {
        8
    } else if nq < 40.0 {
        7
    } else if nq < 60.0 {
        6
    } else {
        5
    }
}

/// Velocidad en el ojo, m/s.
fn inlet_velocity(nq: f64) -> f64 {
    if nq < 25.0 {
        3.2
    } else if nq < 40.0 {
        3.6
    } else if nq < 60.0 {
        4.0
    } else {
        4.5
    }
}

/// Velocidad meridional de entrada, m/s.
fn inlet_meridional_velocity(nq: f64) -> f64 {
    if nq < 25.0 {
        2.0
    } else if nq < 40.0 {
        2.25
    } else if nq < 60.0 {
        2.5
    } else {
        2.8
    }
}
