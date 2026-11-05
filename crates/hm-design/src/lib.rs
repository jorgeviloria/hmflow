//! Diseño preliminar de bombas centrífugas end-suction OH1/OH2.
//!
//! Con las selecciones publicadas, el motor reproduce el ejemplo D1.A1 de
//! Srinivasan 2017. ψ y φ salen de Gülich. β2 cierra Euler con el bloqueo del
//! álabe. El número de álabes se mueve desde 7 hasta que β2 queda entre 15°
//! y 45°. El ojo, cuando no se fija a mano, minimiza el NPSH3 descontando el
//! cubo.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

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

#[derive(Debug, Clone, Error)]
pub enum DesignError {
    #[error(transparent)]
    Formula(#[from] FormulaError),

    #[error(transparent)]
    Unit(#[from] UnitError),

    #[error("entrada inválida: {0}")]
    Input(String),
}

/// Error de una entrada concreta, con el nombre del parámetro del proyecto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    pub name: &'static str,
    pub message: String,
}

/// Datos de entrada del diseño preliminar.
#[derive(Debug, Clone)]
pub struct DesignInput {
    pub q: Quantity,
    pub h: Quantity,
    pub n: Quantity,
    pub rho: Quantity,
    /// Rendimiento mecánico de cojinetes y cierre, sin el rozamiento de discos
    /// (desde `end_suction_v0.4` los discos van aparte). Sin dato, usa
    /// [`default_mechanical_efficiency`].
    pub eta_m: f64,
    pub npsha: Option<Quantity>,
    /// Viscosidad cinemática del fluido, m²/s. `None`, no positiva o no
    /// finita: agua a 20 °C ([`REFERENCE_WATER_NU`]). Entra en el rozamiento
    /// de discos y en la fuga del anillo. La corrección viscosa de la curva
    /// (tabla 13.1) sigue siendo un contraste y no redimensiona el rodete.
    pub nu: Option<f64>,
}

impl DesignInput {
    /// Valida las entradas del diseño y devuelve el primer campo inválido.
    pub fn validate(&self) -> Result<(), FieldError> {
        validate_input(self)
    }
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
    /// Pérdida por rozamiento de discos (Gülich ec. 3.10.13), W.
    pub p_disk_loss: Quantity,
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
    /// w1q/w1m en el punto de diseño (Gülich §7.2.1, paso 15): velocidad en la
    /// garganta del rodete frente a la relativa de entrada.
    pub inlet_deceleration: f64,
    /// Carga efectiva del álabe ξeff = 2π·ψ/(ηh·zLa·L*·(w1* + w2*)), con
    /// L* = L/d2 y w* = w/u2 (Gülich 2020, ec. 7.1.10). Se compara con
    /// [`blade_loading_allowable`]. Con nq > 40 la línea media 2D es mucho más
    /// corta que el álabe 3D y ξeff sale 2..3 veces el admisible: solo se avisa
    /// (HM-DES-036) con nq ≤ [`CYLINDRICAL_BLADE_MAX_NQ`].
    pub blade_loading: f64,
    /// Caudal de la bomba en que c3q = c2 en la garganta de la voluta, Qc
    /// (Gülich ec. 6.22a, con la meridional). Sin cruce vale
    /// [`NO_MATCHING_FLOW_RATIO`]·Qd.
    pub volute_matching_flow: Quantity,
    /// Qsa = min(Qc, (QSF + QW)/2), comienzo de la subida brusca del NPSH3
    /// (Gülich p. 342), como fracción del caudal de diseño.
    pub transition_ratio: f64,
    /// Inicio estimado de la recirculación a la entrada, fracción de Qd.
    pub recirculation_onset: f64,
    /// ηv del método (Gülich 3.5.10, o el manual); igual a `eta_v`. Se conserva
    /// como campo del informe.
    pub leakage_eta_v: f64,
    /// ηm estimado de Gülich (ec. 3.10.14), para contraste.
    pub mechanical_eta_estimate: f64,
    /// Pérdida por rozamiento de discos frente a la potencia útil, PRR/Pu
    /// (Gülich ec. 3.10.13), para contraste.
    pub disk_friction_ratio: f64,
    /// ηh estadístico de Gülich (ec. 3.9.6), para contraste.
    pub statistical_eta_h: f64,
    /// η por análisis de pérdidas (Gülich ec. 3.10.15) con `statistical_eta_h`,
    /// `leakage_eta_v`, el `eta_m` de la entrada y `disk_friction_ratio`.
    /// Desde `end_suction_v0.4` es igual a `eta`.
    pub loss_eta: f64,
    /// Fuga por el anillo delantero frente al caudal, Qsp/Q, calculada con la
    /// geometría del anillo (Gülich tabla 3.7). Contraste.
    pub seal_leakage_ratio: f64,
    /// ηv con esa fuga (Gülich ec. 3.10.4, sin agujeros de equilibrio).
    /// Contraste: el método sigue con `leakage_eta_v`.
    pub seal_eta_v: f64,
    pub slip: f64,
    pub cu2: Quantity,
    /// Caudal en que la altura teórica se anula, en fracción de Qd
    /// (Gülich ec. 3.2.7).
    pub zero_head_flow_ratio: f64,
    /// Pendiente de H/Hd frente a Q/Qd en el punto de diseño: la tangente a la
    /// recta que pasa por Q(Hth = 0) (Gülich §4.3.5).
    pub head_slope: f64,
    pub volute: VoluteResult,
    /// El ojo automático descuenta el cubo: `d0` es el diámetro equivalente
    /// del área libre. Con `c0`, `cm0` o `J` manuales, `d0` es el de
    /// Srinivasan y el cubo no entra en el área.
    pub eye_excludes_hub: bool,
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
    /// Diámetro de la garganta circular por conservación del momento cinético
    /// (Gülich T7.7.7), de contraste: el motor dimensiona con la razón c3q/c2.
    pub momentum_throat_diameter: Quantity,
    /// Ángulo de envolvente de la voluta doble, grados; `None` si es simple.
    pub wrap_deg: Option<f64>,
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
/// - v0.8: el espesor del álabe y d1/d2 entran en Euler; Z de 5 a 9 según β2; el
///   ojo minimiza el NPSH3 con el cubo restado.
/// - v0.9: el motor sigue a Gülich donde la auditoría lo vio apartarse
///   (`docs/validation/formula-audit.md`): un solo ηv, garganta y Qc, anillo
///   laminar, rugosidad, Z entre 5 y 7, avisos de entrada y carga del álabe.
pub const METHOD: &str = "end_suction_v0.9";

/// Métodos anteriores que `recalc` sabe migrar a [`METHOD`].
pub const LEGACY_METHODS: &[&str] = &[
    "end_suction_v0.1",
    "end_suction_v0.2",
    "end_suction_v0.3",
    "end_suction_v0.4",
    "end_suction_v0.5",
    "end_suction_v0.6",
    "end_suction_v0.7",
    "end_suction_v0.8",
];

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
    // Opciones del método v0.9. Se guardan con su valor efectivo; en
    // `eta_v_fixed`, `impeller_roughness` y `volute_wrap_deg`, 0 significa
    // «sin dato» (ηv estimado, rodete liso, voluta simple).
    ParamSpec {
        name: "eta_v_fixed",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "balance_holes",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "volute_ratio",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "impeller_roughness",
        unit: "m",
        formula: None,
    },
    ParamSpec {
        name: "volute_channels",
        unit: "1",
        formula: None,
    },
    ParamSpec {
        name: "volute_wrap_deg",
        unit: "deg",
        formula: None,
    },
    ParamSpec {
        name: "incidence_deg",
        unit: "deg",
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
        formula: Some("design.statistical_hydraulic_efficiency"),
    },
    ParamSpec {
        name: "eta_v",
        unit: "1",
        formula: Some("design.volumetric_efficiency_gulich"),
    },
    ParamSpec {
        name: "eta",
        unit: "1",
        formula: Some("design.loss_efficiency"),
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
        formula: Some("design.bearing_seal_loss"),
    },
    ParamSpec {
        name: "p_disk_loss",
        unit: "W",
        formula: Some("design.disk_friction_loss"),
    },
    ParamSpec {
        name: "npshr_d",
        unit: "m",
        formula: Some("design.npsh3_inlet"),
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
        formula: Some("design.volute_throat_area_ratio"),
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

/// Razón de deceleración c3q/c2 de la garganta de la voluta por defecto:
/// Gülich 2020, tabla 10.2 (fila 9, «no mucho menor que 0,8») y p. 180. Con
/// 1,0 sale la garganta de Srinivasan (D2.2).
pub const DEFAULT_VOLUTE_RATIO: f64 = 0.8;

/// Intervalo admitido para la razón c3q/c2 de la garganta.
pub const VOLUTE_RATIO_RANGE: (f64, f64) = (0.5, 1.0);

/// Intervalo del ángulo de envolvente de una voluta doble en que vale la
/// tabla 9.7 (1), en grados.
pub const VOLUTE_WRAP_RANGE_DEG: (f64, f64) = (90.0, 180.0);

/// Espesor del borde de ataque de la lengua frente a d2 (Gülich 2020, p. 493).
const CUTWATER_THICKNESS_RATIO: f64 = 0.02;

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
    /// Razón c3q/c2 de la garganta de la voluta. Sin dato,
    /// [`DEFAULT_VOLUTE_RATIO`]. Entre 0,5 y 1.
    pub volute_ratio: Option<f64>,
    /// Holgura diametral del anillo de desgaste delantero, m. Sin dato, la de
    /// Gülich ec. (3.12). Solo entra en el contraste de fugas `seal_eta_v`.
    pub seal_clearance: Option<f64>,
    /// Rendimiento volumétrico del rodete. Sin dato, el de Gülich 3.5.10
    /// ([`gulich_volumetric_efficiency`]). Entre 0 y 1, sin incluir los
    /// extremos. Dimensiona Qth, φ y b2 y entra en el análisis de pérdidas.
    pub eta_v: Option<f64>,
    /// Ángulo de envolvente de la voluta interior, en grados, de una voluta
    /// doble (90..180°). `None` es una voluta simple. Solo cambia el empuje
    /// radial (Gülich tabla 9.7 (1)): kR = FDsp·kR0 de 0 a 1,1·Qd.
    pub volute_wrap_deg: Option<f64>,
    /// Incidencia del álabe a la entrada i1, en grados: β1B es arctan(τ1·cm0/u1)
    /// más i1. Sin dato, [`geometry::DEFAULT_INCIDENCE_DEG`] (2°, centro de los
    /// 0..4° de Gülich p. 411). Entre 0 y [`INCIDENCE_RANGE_DEG`].1. Mueve
    /// QSF, QW, QPS y w1q/w1m (HM-DES-035), no el NPSH3.
    pub incidence_deg: Option<f64>,
    /// Número de lenguas de la voluta zLe: 1 en una voluta simple (por
    /// defecto), 2 en una doble. Con un zLe par se evita un Z par (Gülich
    /// p. 491).
    pub volute_channels: Option<u32>,
    /// Rugosidad de arena equivalente del rodete, m (Gülich 3.10.5). Sin dato
    /// el rodete se trata como liso, que es lo que dejaba la v0.8. Multiplica
    /// el rozamiento de discos por fR (3.6.6).
    pub impeller_roughness: Option<f64>,
    /// El rodete tiene agujeros de equilibrio: la fuga de Gülich 3.5.10 se
    /// duplica (zH = 2).
    pub balance_holes: bool,
}

impl DesignOptions {
    /// Valida las opciones que no dependen de valores calculados.
    pub fn validate(&self) -> Result<(), FieldError> {
        if let Some(value) = self.eta_v
            && (!value.is_finite() || value <= 0.0 || value >= 1.0)
        {
            return Err(FieldError {
                name: "eta_v_fixed",
                message: format!(
                    "`eta_v` (parámetro `eta_v_fixed`, opción `--eta-v`) = {value} no es válido; debe estar entre 0 y 1 sin incluirlos"
                ),
            });
        }

        if let Some(value) = self.volute_wrap_deg
            && (!value.is_finite()
                || !(VOLUTE_WRAP_RANGE_DEG.0..=VOLUTE_WRAP_RANGE_DEG.1).contains(&value))
        {
            return Err(FieldError {
                name: "volute_wrap_deg",
                message: format!(
                    "`volute_wrap_deg` = {value} no es válido; debe estar entre {} y {} grados",
                    VOLUTE_WRAP_RANGE_DEG.0, VOLUTE_WRAP_RANGE_DEG.1
                ),
            });
        }

        if let Some(value) = self.incidence_deg
            && (!value.is_finite()
                || !(INCIDENCE_RANGE_DEG.0..=INCIDENCE_RANGE_DEG.1).contains(&value))
        {
            return Err(FieldError {
                name: "incidence_deg",
                message: format!(
                    "`incidence_deg` = {value} no es válido; debe estar entre {} y {} grados",
                    INCIDENCE_RANGE_DEG.0, INCIDENCE_RANGE_DEG.1
                ),
            });
        }

        if self.volute_channels == Some(0) {
            return Err(FieldError {
                name: "volute_channels",
                message: "`volute_channels` = 0 no es válido; debe ser al menos 1".to_string(),
            });
        }

        if let Some(value) = self.volute_ratio
            && (!value.is_finite()
                || !(VOLUTE_RATIO_RANGE.0..=VOLUTE_RATIO_RANGE.1).contains(&value))
        {
            return Err(FieldError {
                name: "volute_ratio",
                message: format!(
                    "`volute_ratio` = {value} no es válido; debe estar entre {} y {}",
                    VOLUTE_RATIO_RANGE.0, VOLUTE_RATIO_RANGE.1
                ),
            });
        }

        Ok(())
    }
}

/// Caudales entre los que vale el ηh estadístico de Gülich (ec. 3.9.6,
/// tabla 3.9), m³/s. Fuera de ahí el motor lo extrapola.
pub const ETA_H_FLOW_RANGE: (f64, f64) = (0.005, 1.0);

/// nq que separa los dos tramos de la fuga de Gülich 3.5.10.
const LEAKAGE_NQ_SPLIT: f64 = 27.0;

/// ηv de Gülich (ec. 3.5.9 y 3.5.10): `1/(1 + a·zH/nq^m)` con (a, m) =
/// (4,1, 1,6) si nq < 27 y (0,15, 0,6) si no; `zH = 2` con agujeros de
/// equilibrio. Es el único ηv del método desde la v0.9.
pub fn gulich_volumetric_efficiency(nq: f64, balance_holes: bool) -> Result<f64, DesignError> {
    let registry = Registry::all();
    let one = Unit::dimensionless();
    let (a, m) = if nq < LEAKAGE_NQ_SPLIT {
        (4.1, 1.6)
    } else {
        (0.15, 0.6)
    };
    Ok(evaluate(
        &registry,
        "design.volumetric_efficiency_gulich",
        &[
            ("nq", quantity(nq, one.clone())),
            (
                "zH",
                quantity(if balance_holes { 2.0 } else { 1.0 }, one.clone()),
            ),
            ("a", quantity(a, one.clone())),
            ("m", quantity(m, one)),
        ],
    )?
    .value())
}

/// Rugosidad relativa máxima ε/r2 que admite el motor para el rodete. Elección
/// del motor: una fundición rugosa da ε/r2 ≈ 1e-4..1e-3; por encima, la ec.
/// 3.6.6 deja de describir un rodete y el rozamiento de discos se dispara.
const MAX_RELATIVE_ROUGHNESS: f64 = 0.01;

/// Qc/Qd que se informa cuando la voluta no cruza c3q = c2: finito, para que el
/// informe lo escriba como número, y lejos del rango de la curva (hasta 1,3·Qd).
pub const NO_MATCHING_FLOW_RATIO: f64 = 10.0;

/// Intervalo de búsqueda de β2, en grados.
const BETA2_SEARCH_DEG: (f64, f64) = (1.0, 89.0);

/// Número de álabes con que empieza la búsqueda, el de D1.A1 y el mayor de los
/// recomendados. No es el de una bomba concreta.
const BLADE_COUNT_START: u32 = 7;

/// Z recomendado por Gülich 2020, p. 408: de 5 a 7 álabes con 10 < nq < 120.
const BLADE_COUNT_RECOMMENDED: (u32, u32) = (5, 7);

/// Z que solo se prueba si con 5..7 β2 no cabe en 15..45°: «8 o más no se
/// recomienda» por la estabilidad de la curva Q-H (p. 408). Pasar de 7 avisa
/// con HM-DES-034; fuera de 4..12 avisa además HM-DES-005.
const BLADE_COUNT_EXTENDED: (u32, u32) = (8, 9);

/// Z máximo sin aviso HM-DES-034.
const BLADE_COUNT_MAX_RECOMMENDED: u32 = BLADE_COUNT_RECOMMENDED.1;

/// Holgura mínima entre el cubo y el diámetro del álabe a la entrada, m.
const EYE_HUB_CLEARANCE: f64 = 0.005;

/// Tope del ojo automático frente a D2, para que el álabe tenga desarrollo.
const EYE_TO_IMPELLER: f64 = 0.85;

/// Metal del paso por encima del cual τ se congela: 60 % del paso.
const BLADE_BLOCKAGE_LIMIT: f64 = 0.6;

/// J por defecto cuando se fijan las velocidades de entrada y no el diámetro
/// del álabe. Es el de D1.A1 (Srinivasan 2017, p. 415), no una tabla de nq.
const DEFAULT_EYE_RATIO: f64 = 1.03;

/// Puntos con que se busca el máximo de potencia de la curva preliminar. El
/// mismo recuento que usa la mecánica del motor.
const POWER_CURVE_POINTS: usize = 101;

/// Selecciones que dependen solo de `nq`. Z, c0 y cm0 no salen de aquí: el
/// primero cierra β2 y los otros salen del ojo.
///
/// - ψ: Gülich 2020, ec. (3.26) con fT = 1,0.
/// - φ: el que da la anchura relativa media de Gülich, ec. (7.1), con ese ψ y
///   el ηv del método. Un φ constante por tramos no puede seguir esa curva,
///   porque b2* crece con nq² a φ fijo.
fn base_selection(
    registry: &Registry,
    nq: f64,
    eta_v: f64,
) -> Result<DesignSelection, DesignError> {
    let one = Unit::dimensionless();
    let psi = evaluate(
        registry,
        "design.head_coefficient",
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
        blade_count: BLADE_COUNT_START,
        c0: f64::NAN,
        cm0: f64::NAN,
        a: 4.5,
        j: DEFAULT_EYE_RATIO,
        margin: 1.1,
    })
}

/// ψ, φ y un β2 de Euler sin espesor ni relación de radios, para el `nq` dado.
///
/// Z, c0 y cm0 de este resultado no son la selección del motor: dependen del
/// caudal, del cubo y del espesor, y salen de [`design`].
pub fn table_selection(nq: f64, eta_h: f64) -> Result<DesignSelection, DesignError> {
    let registry = Registry::all();
    let eta_v = gulich_volumetric_efficiency(nq, false)?;
    let mut selection = base_selection(&registry, nq, eta_v)?;
    selection.beta2_deg = euler_blade_angle(
        &registry,
        &EulerTarget {
            psi: selection.psi,
            phi: selection.phi,
            eta_h,
            blade_count: BLADE_COUNT_START,
            outlet_diameter: 0.0,
            trailing_thickness: 0.0,
            radius_ratio: 0.0,
        },
    )?;
    Ok(selection)
}

/// Bloqueo de álabes τ ≥ 1.
///
/// τ = 1 / (1 − min(Z·e/(π·d·sen β), 0,6)). En la notación del pack, τ = 1
/// es paso libre: la meridional local del triángulo es τ veces la media.
pub fn blade_blockage(blade_count: u32, thickness: f64, diameter: f64, beta_rad: f64) -> f64 {
    if blade_count == 0 || thickness <= 0.0 || diameter <= 0.0 {
        return 1.0;
    }
    let sin_beta = beta_rad.sin().abs().max(1e-4);
    let metal = f64::from(blade_count) * thickness / (std::f64::consts::PI * diameter * sin_beta);
    1.0 / (1.0 - metal.min(BLADE_BLOCKAGE_LIMIT))
}

/// Frase que explica que el caudal queda fuera del rango del ηh estadístico.
fn eta_h_range_note(q: f64) -> String {
    format!(
        "El caudal ({q:.4} m³/s) está fuera de 0,005..1 m³/s, el rango del ηh estadístico de \
         Gülich (ec. 3.9.6); fuera de él ηh se extrapola y puede quedar muy bajo"
    )
}

fn eta_h_flow_in_range(q: f64) -> bool {
    (ETA_H_FLOW_RANGE.0..=ETA_H_FLOW_RANGE.1).contains(&q)
}

/// Re* de transición laminar/turbulento del anillo (Gülich 3.7.10).
const SEAL_TRANSITION_REYNOLDS: f64 = 2000.0;

/// El flujo del anillo se trata como laminar para el factor de rotación si
/// `Re* = √(Re² + ¼·Reu²) < 2000` (Gülich 3.7.10): entonces vale 3.7.14, y si
/// no 3.7.13. `re = 2·s·cax/ν` y `reu = 2·s·usp/ν`.
pub fn seal_rotation_is_laminar(re: f64, reu: f64) -> bool {
    (re * re + 0.25 * reu * reu).sqrt() < SEAL_TRANSITION_REYNOLDS
}

/// nq de referencia de la carga admisible del álabe (Gülich 7.1.11).
const BLADE_LOADING_REF_NQ: f64 = 40.0;

/// Margen sobre la carga admisible a partir del cual se avisa: la banda de
/// 7.1.11 es ±15 %.
const BLADE_LOADING_MARGIN: f64 = 0.15;

/// Carga admisible del álabe ξal = (40/nq)^0,77 (Gülich 2020, ec. 7.1.11,
/// p. 407; banda ±15 %).
pub fn blade_loading_allowable(nq: f64) -> f64 {
    (BLADE_LOADING_REF_NQ / nq).powf(0.77)
}

/// Banda de w1q/w1m en el BEP con entrada axial (Gülich 2020, p. 415).
pub const INLET_THROAT_RATIO_BAND: (f64, f64) = (0.75, 0.85);

/// Deceleración de la velocidad relativa de entrada a la de la garganta,
/// `w1q/w1m = (QLa/A1q)/√(c1m² + u1m²)` (Gülich 2020, §7.2.1 paso 15). `a1q_total`
/// es `zLa·A1q`. El borde de ataque 2D es cilíndrico: d1m = d1 y `u1m = u1`.
pub fn inlet_throat_ratio(q_la: f64, a1q_total: f64, c1m: f64, u1m: f64) -> f64 {
    (q_la / a1q_total) / c1m.hypot(u1m)
}

/// Área total de garganta de la entrada, `zLa·A1q`, con el espesor de los
/// álabes: `A1·sen β1B/τ1` (Gülich 2020, ec. 6.18). Con τ1 = 1 (sin espesor)
/// es `A1·sen β1B` y QW coincide con QSF.
pub fn inlet_throat_area(a1: f64, beta1b_rad: f64, tau1: f64) -> f64 {
    a1 * beta1b_rad.sin() / tau1
}

/// Viscosidad cinemática que usan discos y anillo, m²/s.
fn fluid_nu(input: &DesignInput) -> f64 {
    match input.nu {
        Some(nu) if nu.is_finite() && nu > 0.0 => nu,
        _ => REFERENCE_WATER_NU,
    }
}

/// Datos de Euler para resolver β2. Diámetro o espesor nulos dejan τ2 = 1.
struct EulerTarget {
    psi: f64,
    phi: f64,
    eta_h: f64,
    blade_count: u32,
    outlet_diameter: f64,
    /// Borde de salida, e/2, m.
    trailing_thickness: f64,
    /// d1/d2. Wiesner aplica kw cuando supera εLim.
    radius_ratio: f64,
}

/// Ángulo de salida del álabe β2, en grados, que hace que el rodete dé la
/// altura pedida (Gülich 2020, p. 412, paso 10).
///
/// Resuelve `design.cu2` con el deslizamiento de Wiesner (incluido kw si
/// `radius_ratio` = d1/d2 supera εLim) igual a `ψ/(2ηh)`. `u2` = 1 y
/// `cm2/u2` = φ.
fn euler_blade_angle(registry: &Registry, target: &EulerTarget) -> Result<f64, DesignError> {
    let one = Unit::dimensionless();
    let ms = unit("m/s")?;
    let deg = unit("deg")?;
    let needed = target.psi / (2.0 * target.eta_h);
    let cu2_over_u2 = |beta2_rad: f64| -> Result<f64, DesignError> {
        let beta2 = Quantity::from_si(beta2_rad, deg.clone());
        let tau = blade_blockage(
            target.blade_count,
            target.trailing_thickness,
            target.outlet_diameter,
            beta2_rad,
        );
        let slip = evaluate(
            registry,
            "design.slip_factor",
            &[
                ("beta2", beta2.clone()),
                ("Z", quantity(f64::from(target.blade_count), one.clone())),
                ("eps", quantity(target.radius_ratio, one.clone())),
            ],
        )?;
        Ok(evaluate(
            registry,
            "design.cu2",
            &[
                ("slip", slip),
                ("u2", Quantity::from_si(1.0, ms.clone())),
                ("cm2", Quantity::from_si(target.phi, ms.clone())),
                ("beta2", beta2),
                ("tau", quantity(tau, one.clone())),
            ],
        )?
        .si)
    };

    let (low, high) = (
        BETA2_SEARCH_DEG.0.to_radians(),
        BETA2_SEARCH_DEG.1.to_radians(),
    );
    // cu2/u2 = γ(β2) − τ·φ/tan β2 no es monótona: √sen β2 en el deslizamiento
    // la hace pasar por un máximo antes de 89°. La altura es alcanzable si
    // ese máximo llega a `needed`, y la raíz buscada es la de la rama
    // creciente (menor β2), donde más β2 da más altura.
    let peak = golden_section_min(
        |beta2| cu2_over_u2(beta2).map_or(f64::INFINITY, |value| -value),
        low,
        high,
    );
    let reachable = cu2_over_u2(peak)?;
    if reachable < needed {
        // cu2/u2 no llega a 1 con ningún Z: más álabes solo acercan a ese límite.
        let advice = if needed >= 1.0 {
            "ningún número de álabes la alcanza: reduce ψ"
        } else {
            "aumenta Z o reduce ψ"
        };
        return Err(DesignError::Input(format!(
            "con ψ = {psi:.3}, φ = {phi:.3} y Z = {blade_count}, ningún β2 da la altura: \
             hace falta cu2/u2 = {needed:.3} y el máximo es {reachable:.3}; {advice}",
            psi = target.psi,
            phi = target.phi,
            blade_count = target.blade_count,
        )));
    }
    let root = hm_math::bisect(
        |beta2| cu2_over_u2(beta2).map_or(f64::NAN, |value| value - needed),
        low,
        peak,
        1e-10,
        200,
    )
    .map_err(|error| DesignError::Input(format!("β2 por Euler: {error}")))?;

    let residual = cu2_over_u2(root)? - needed;
    if !residual.is_finite() || residual.abs() > 1e-6 {
        return Err(DesignError::Input(format!(
            "β2 por Euler no converge (residuo {residual:e})"
        )));
    }
    Ok(root.to_degrees())
}

struct OutletGeometry {
    blade_count: u32,
    beta2_deg: f64,
    tau2: f64,
    /// Espesor nominal del álabe, m. El borde de salida es la mitad.
    thickness: f64,
}

/// Pedido de salida. `blade_count` y `beta2_rad` vacíos activan el recorrido.
struct OutletRequest {
    psi: f64,
    phi: f64,
    eta_h: f64,
    nq: f64,
    u2: f64,
    d2: f64,
    d1: f64,
    blade_count: Option<u32>,
    beta2_rad: Option<f64>,
    /// Lenguas de la voluta, zLe.
    volute_channels: u32,
}

/// Z y β2. Con Z manual se resuelve solo β2. Con β2 manual, Z se queda en
/// [`BLADE_COUNT_START`] salvo que también se haya fijado. Sin ninguno, el libro
/// fija Z antes que β2 (§7.2.1, pasos 5 y 10) y no da un criterio entre 5, 6 y
/// 7: se prueba de 7 a 5 y luego 8 y 9, y se queda el primero cuyo β2 cabe en
/// 15..45°. Con un número par de lenguas solo se prueban Z impares. Un Z cuyo
/// β2 no alcanza la altura se salta; si ninguno cabe, el de β2 más cercano al
/// rango (HM-DES-004), y si ninguno resuelve, el error del último.
fn select_outlet(
    registry: &Registry,
    request: &OutletRequest,
) -> Result<OutletGeometry, DesignError> {
    let eps = if request.d2 > 0.0 {
        (request.d1 / request.d2).clamp(0.0, 0.999)
    } else {
        0.0
    };
    let solve = |z: u32| -> Result<OutletGeometry, DesignError> {
        let (_, thickness, trailing) =
            mechanics::blade_thicknesses(z, request.u2, request.nq, request.d2)?;
        let beta2_deg = if let Some(beta2_rad) = request.beta2_rad {
            beta2_rad.to_degrees()
        } else {
            euler_blade_angle(
                registry,
                &EulerTarget {
                    psi: request.psi,
                    phi: request.phi,
                    eta_h: request.eta_h,
                    blade_count: z,
                    outlet_diameter: request.d2,
                    trailing_thickness: trailing,
                    radius_ratio: eps,
                },
            )?
        };
        let tau2 = blade_blockage(z, trailing, request.d2, beta2_deg.to_radians());
        Ok(OutletGeometry {
            blade_count: z,
            beta2_deg,
            tau2,
            thickness,
        })
    };

    if let Some(z) = request.blade_count {
        return solve(z);
    }
    if request.beta2_rad.is_some() {
        return solve(BLADE_COUNT_START);
    }

    let allowed = |z: &u32| !request.volute_channels.is_multiple_of(2) || !z.is_multiple_of(2);
    let candidates = (BLADE_COUNT_RECOMMENDED.0..=BLADE_COUNT_RECOMMENDED.1)
        .rev()
        .chain(BLADE_COUNT_EXTENDED.0..=BLADE_COUNT_EXTENDED.1)
        .filter(allowed);
    let (low, high) = BETA2_RANGE_DEG;
    let mut nearest: Option<(f64, OutletGeometry)> = None;
    let mut last_error = None;
    for z in candidates {
        match solve(z) {
            Ok(outlet) if (low..=high).contains(&outlet.beta2_deg) => return Ok(outlet),
            Ok(outlet) => {
                let miss = (low - outlet.beta2_deg).max(outlet.beta2_deg - high);
                if nearest.as_ref().is_none_or(|(best, _)| miss < *best) {
                    nearest = Some((miss, outlet));
                }
            }
            Err(error) => last_error = Some(error),
        }
    }
    match (nearest, last_error) {
        (Some((_, outlet)), _) => Ok(outlet),
        (None, Some(error)) => Err(error),
        (None, None) => Err(DesignError::Input(
            "no hay un número de álabes que resuelva la altura".to_string(),
        )),
    }
}

/// Mínimo de `f` en `[a, b]` por sección áurea. `f` se supone unimodal.
fn golden_section_min(mut f: impl FnMut(f64) -> f64, mut a: f64, mut b: f64) -> f64 {
    let phi = (1.0 + 5.0_f64.sqrt()) / 2.0;
    let mut c = b - (b - a) / phi;
    let mut d = a + (b - a) / phi;
    let mut fc = f(c);
    let mut fd = f(d);
    for _ in 0..64 {
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = b - (b - a) / phi;
            fc = f(c);
        } else {
            a = c;
            c = d;
            fc = fd;
            d = a + (b - a) / phi;
            fd = f(d);
        }
    }
    if fc < fd { c } else { d }
}

/// Diámetro del álabe a la entrada que minimiza λc·c² + λw·(c² + u²), con el
/// cubo restado del área. Devuelve `(d1, c1m, d0 equivalente)`.
fn minimize_eye(qth: f64, n: f64, d2: f64, hub: f64) -> Result<(f64, f64, f64), DesignError> {
    let low = hub + EYE_HUB_CLEARANCE;
    let high = (EYE_TO_IMPELLER * d2).max(low + 1.0e-3);
    if !high.is_finite() || !low.is_finite() || high <= low || hub >= d2 {
        return Err(DesignError::Input(format!(
            "el cubo ({:.1} mm) no cabe en el rodete (D2 = {:.1} mm)",
            hub * 1000.0,
            d2 * 1000.0
        )));
    }
    let objective = |d1: f64| {
        let free = d1 * d1 - hub * hub;
        if free <= 1.0e-8 {
            return f64::INFINITY;
        }
        let c = qth / (std::f64::consts::PI / 4.0 * free);
        let u = std::f64::consts::PI * d1 * n;
        NPSH_LAMBDA_C * c * c + NPSH_LAMBDA_W * (c * c + u * u)
    };
    let d1 = golden_section_min(objective, low, high);
    let free = d1 * d1 - hub * hub;
    if free <= 0.0 {
        return Err(DesignError::Input(
            "el ojo automático no deja área libre alrededor del cubo".to_string(),
        ));
    }
    let c1m = qth / (std::f64::consts::PI / 4.0 * free);
    Ok((d1, c1m, free.sqrt()))
}

/// c0 que minimiza el NPSH3 sin cubo, con el J dado. Forma cerrada
/// c/u = √(λw / (2(λc+λw))).
fn hubless_inlet_speed(qth: f64, n: f64, j: f64) -> Result<f64, DesignError> {
    if j <= 0.0 || n <= 0.0 || qth <= 0.0 {
        return Err(DesignError::Input(
            "el ojo sin cubo necesita J, n y Qth positivos".to_string(),
        ));
    }
    let k = (NPSH_LAMBDA_W / (2.0 * (NPSH_LAMBDA_C + NPSH_LAMBDA_W))).sqrt();
    let d0 = (4.0 * qth / (k * std::f64::consts::PI.powi(2) * j * n)).cbrt();
    Ok(qth / (std::f64::consts::PI / 4.0 * d0 * d0))
}

/// `(c0, cm0, J, el ojo descuenta el cubo)`.
fn choose_inlet(
    options: &DesignOptions,
    qth: f64,
    n: f64,
    d2: f64,
    hub: f64,
) -> Result<(f64, f64, f64, bool), DesignError> {
    if options.c0.is_none() && options.cm0.is_none() && options.j.is_none() {
        let (d1, c1m, d0) = minimize_eye(qth, n, d2, hub)?;
        return Ok((c1m, c1m, d1 / d0, true));
    }
    let c0 = match (options.c0, options.cm0) {
        (Some(c0), _) => c0,
        (None, Some(cm0)) => cm0,
        (None, None) => hubless_inlet_speed(qth, n, options.j.unwrap_or(DEFAULT_EYE_RATIO))?,
    };
    let cm0 = options.cm0.unwrap_or(c0);
    let j = options.j.unwrap_or(DEFAULT_EYE_RATIO);
    Ok((c0, cm0, j, false))
}

/// Intervalo admitido de la incidencia i1, en grados. Gülich p. 411 da 0..4°;
/// el tope de 10° deja margen a una medida de planta sin admitir un ángulo
/// que ya no es incidencia.
pub const INCIDENCE_RANGE_DEG: (f64, f64) = (0.0, 10.0);

/// β1B y τ1. β1B = arctan(τ1·cm0/u1) + i1, y τ1 depende de β1B.
fn leading_edge(
    registry: &Registry,
    incidence_deg: f64,
    cm0: f64,
    u1: f64,
    d1: f64,
    thickness: f64,
    blade_count: u32,
) -> Result<(f64, f64), DesignError> {
    let deg = unit("deg")?;
    let ms = unit("m/s")?;
    let incidence = incidence_deg.to_radians();
    let mut beta = (cm0 / u1.max(1.0e-9)).atan() + incidence;
    let mut tau = blade_blockage(blade_count, thickness, d1, beta);
    for _ in 0..20 {
        let next = evaluate(
            registry,
            "design.blade_inlet_angle",
            &[
                ("c1m", Quantity::from_si(tau * cm0, ms.clone())),
                ("u1", Quantity::from_si(u1, ms.clone())),
                ("i1", Quantity::from_si(incidence, deg.clone())),
            ],
        )?
        .si;
        if (next - beta).abs() < 1.0e-10 {
            beta = next;
            break;
        }
        beta = next;
        tau = blade_blockage(blade_count, thickness, d1, beta);
    }
    tau = blade_blockage(blade_count, thickness, d1, beta);
    Ok((beta, tau))
}

/// Calcula un diseño preliminar end-suction.
pub fn design(input: &DesignInput) -> Result<DesignResult, DesignError> {
    design_with_options(input, &DesignOptions::default())
}

/// Rendimiento mecánico por defecto, sin rozamiento de discos: la estimación
/// de Gülich 2020, ec. (3.10.14), con QRef = 1 m³/s y nRef = 1500 rpm.
pub fn default_mechanical_efficiency(q: &Quantity, n: &Quantity) -> Result<f64, DesignError> {
    let registry = Registry::all();
    Ok(evaluate(
        &registry,
        "design.mechanical_efficiency_estimate",
        &[
            ("Q", q.clone()),
            ("n", n.clone()),
            ("Q_ref", Quantity::from_si(1.0, unit("m3/s")?)),
            ("n_ref", Quantity::from_si(1500.0 / 60.0, unit("1/s")?)),
        ],
    )?
    .value())
}

/// Calcula un diseño preliminar con selecciones explícitas.
pub fn design_with_options(
    input: &DesignInput,
    options: &DesignOptions,
) -> Result<DesignResult, DesignError> {
    input
        .validate()
        .map_err(|error| DesignError::Input(error.message))?;

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

    options
        .validate()
        .map_err(|error| DesignError::Input(error.message))?;

    let eta_v = match options.eta_v {
        Some(value) => value,
        None => gulich_volumetric_efficiency(nq, options.balance_holes)?,
    };
    let table = base_selection(&registry, nq, eta_v)?;
    let manual_beta2 = options.beta2_rad.map(f64::to_degrees);
    let mut selection = DesignSelection {
        psi: options.psi.unwrap_or(table.psi),
        phi: options.phi.unwrap_or(table.phi),
        // Provisional, solo para rechazar un manual absurdo. El definitivo
        // sale de select_outlet, con el espesor y d1/d2.
        beta2_deg: manual_beta2.unwrap_or(20.0),
        blade_count: options.blade_count.unwrap_or(BLADE_COUNT_START),
        // Provisional. choose_inlet lo sustituye; aquí basta un positivo
        // para que un c0 o cm0 manual no finito falle antes.
        c0: options.c0.or(options.cm0).unwrap_or(3.0),
        cm0: options.cm0.or(options.c0).unwrap_or(3.0),
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
    let incidence_deg = options
        .incidence_deg
        .unwrap_or(geometry::DEFAULT_INCIDENCE_DEG);
    let volute_channels = options.volute_channels.unwrap_or(1);
    let volute_ratio_value = options.volute_ratio.unwrap_or(DEFAULT_VOLUTE_RATIO);
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

    // end_suction_v0.7: un solo ηh, el estadístico de Gülich (ec. 3.9.6),
    // para cerrar Euler (β2) y para el análisis de pérdidas. Hasta la v0.6
    // Euler usaba el de Srinivasan (diámetro específico) y la potencia el de
    // Gülich: la geometría no daba la altura con el ηh de las pérdidas.
    let eta_h = evaluate(
        &registry,
        "design.statistical_hydraulic_efficiency",
        &[
            ("Q", input.q.clone()),
            ("Q_ref", quantity(1.0, m3s.clone())),
            ("nq", quantity(nq, one.clone())),
        ],
    )?
    .value();

    if !eta_h.is_finite() || eta_h <= 0.0 || eta_h >= 1.0 {
        let note = if eta_h_flow_in_range(input.q.si) {
            String::new()
        } else {
            format!(". {}", eta_h_range_note(input.q.si))
        };
        return Err(DesignError::Input(format!(
            "el ηh estadístico de Gülich sale {eta_h:.3} (nq = {nq:.1}), fuera de (0, 1){note}"
        )));
    }

    let p_hyd = evaluate(
        &registry,
        "perf.hydraulic_power",
        &[
            ("rho", input.rho.clone()),
            ("Q", input.q.clone()),
            ("H", input.h.clone()),
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

    // Discos y potencia antes del ojo: no dependen de d1, β2 ni Z. El cubo
    // del ojo sale de este par (P·margen). Si la curva pide más potencia,
    // torque_max sube al final y la mecánica del eje usa ese par mayor; el
    // ojo no se redimensiona.
    // Un solo ηv (v0.9): el de Gülich 3.5.10 dimensiona y entra en las pérdidas.
    let leakage_eta_v = eta_v;
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
    let nu = Quantity::from_si(fluid_nu(input), unit("m2/s")?);
    let disk_reynolds = evaluate(
        &registry,
        "design.disk_reynolds",
        &[
            ("n", input.n.clone()),
            ("d2", d2.clone()),
            ("nu", nu.clone()),
        ],
    )?;
    let disk_roughness_factor = match options.impeller_roughness {
        None => 1.0,
        Some(eps) => {
            let eps_over_r2 = eps / (0.5 * d2.si);
            let inside_log = 0.2 * eps_over_r2 + 12.5 / disk_reynolds.value();
            if !eps.is_finite()
                || eps < 0.0
                || eps_over_r2 > MAX_RELATIVE_ROUGHNESS
                || !inside_log.is_finite()
                || inside_log >= 1.0
            {
                return Err(DesignError::Input(format!(
                    "`impeller_roughness` = {eps} m no es válida; debe ser finita, no \
                     negativa, con ε/r2 ≤ {MAX_RELATIVE_ROUGHNESS} y 0,2·ε/r2 + 12,5/Re < 1"
                )));
            }
            evaluate(
                &registry,
                "design.disk_roughness_factor",
                &[
                    ("Re", disk_reynolds.clone()),
                    ("eps_over_r2", quantity(eps_over_r2, one.clone())),
                ],
            )?
            .value()
        }
    };
    let disk_friction_coefficient = evaluate(
        &registry,
        "design.disk_friction_coefficient",
        &[
            ("Re", disk_reynolds),
            ("k0", quantity(DISK_K0, one.clone())),
        ],
    )?;
    let disk_friction_coefficient = quantity(
        disk_friction_coefficient.value() * disk_roughness_factor,
        one.clone(),
    );
    let disk_friction_ratio = evaluate(
        &registry,
        "design.disk_friction_ratio",
        &[
            ("k_rr", disk_friction_coefficient),
            ("f_geo", quantity(DISK_F_GEO, one.clone())),
            ("nq", quantity(nq, one.clone())),
            ("psi", quantity(selection.psi, one.clone())),
        ],
    )?
    .value();
    let statistical_eta_h = eta_h;
    let loss_eta = evaluate(
        &registry,
        "design.loss_efficiency",
        &[
            ("eta_h", quantity(statistical_eta_h, one.clone())),
            ("eta_v", quantity(leakage_eta_v, one.clone())),
            ("eta_m", quantity(input.eta_m, one.clone())),
            ("disk_ratio", quantity(disk_friction_ratio, one.clone())),
        ],
    )?
    .value();
    let eta = loss_eta;
    let p_hyd_loss = evaluate(
        &registry,
        "design.hydraulic_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta_h", quantity(statistical_eta_h, one.clone())),
        ],
    )?;
    let p_vol_loss = evaluate(
        &registry,
        "design.volumetric_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("eta_h", quantity(statistical_eta_h, one.clone())),
            ("eta_v", quantity(leakage_eta_v, one.clone())),
        ],
    )?;
    let p_disk_loss = evaluate(
        &registry,
        "design.disk_friction_loss",
        &[
            ("P_hyd", p_hyd.clone()),
            ("disk_ratio", quantity(disk_friction_ratio, one.clone())),
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
    let p_mech_loss = evaluate(
        &registry,
        "design.bearing_seal_loss",
        &[
            ("P_shaft", p_shaft.clone()),
            ("eta_m", quantity(input.eta_m, one.clone())),
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

    let (_, hub) = mechanics::shaft_and_hub(torque_max.si, mechanics::DEFAULT_SHAFT_SHEAR_STRESS)?;
    let (c0, cm0, j, eye_excludes_hub) = choose_inlet(options, qth.si, input.n.si, d2.si, hub)?;
    selection.c0 = c0;
    selection.cm0 = cm0;
    selection.j = j;

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

    let outlet = select_outlet(
        &registry,
        &OutletRequest {
            psi: selection.psi,
            phi: selection.phi,
            eta_h,
            nq,
            u2: u2.si,
            d2: d2.si,
            d1: d1.si,
            blade_count: options.blade_count,
            beta2_rad: options.beta2_rad,
            volute_channels,
        },
    )
    .map_err(|error| match error {
        DesignError::Input(text) if !eta_h_flow_in_range(input.q.si) => {
            DesignError::Input(format!("{text}. {}", eta_h_range_note(input.q.si)))
        }
        other => other,
    })?;
    selection.blade_count = outlet.blade_count;
    selection.beta2_deg = outlet.beta2_deg;
    let tau2 = outlet.tau2;

    let beta2 = Quantity::from_si(selection.beta2_deg.to_radians(), unit("deg")?);
    let radius_ratio = if d2.si > 0.0 {
        (d1.si / d2.si).clamp(0.0, 0.999)
    } else {
        0.0
    };
    let slip = evaluate(
        &registry,
        "design.slip_factor",
        &[
            ("beta2", beta2.clone()),
            ("Z", quantity(f64::from(selection.blade_count), one.clone())),
            ("eps", quantity(radius_ratio, one.clone())),
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
            ("beta2", beta2.clone()),
            ("tau", quantity(tau2, one.clone())),
        ],
    )?;
    let cu2 = with_unit(cu2, &ms);
    let zero_head_flow_ratio = evaluate(
        &registry,
        "design.zero_head_flow_ratio",
        &[
            ("slip", quantity(slip, one.clone())),
            ("beta2", beta2),
            ("phi", quantity(selection.phi, one.clone())),
            ("tau", quantity(tau2, one.clone())),
        ],
    )?
    .value();
    let head_slope = evaluate(
        &registry,
        "design.design_head_slope",
        &[("q0", quantity(zero_head_flow_ratio, one.clone()))],
    )?
    .value();

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
        "design.volute_throat_area_ratio",
        &[
            ("Q", input.q.clone()),
            ("c2", c3.clone()),
            ("ratio", quantity(volute_ratio_value, one.clone())),
        ],
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
        let diameter_si = k_eff_value * (4.0 * area_si / std::f64::consts::PI).sqrt();
        let outer_si = r3.si + diameter_si / 2.0;
        sections.push(VoluteSection {
            angle_deg,
            area: Quantity::from_si(area_si, m2.clone()),
            diameter: Quantity::from_si(diameter_si, m.clone()),
            outer_radius: Quantity::from_si(outer_si, m.clone()),
        });
    }

    let momentum_throat_diameter = evaluate(
        &registry,
        "design.volute_momentum_throat_diameter",
        &[
            ("Q", input.q.clone()),
            ("c2u", cu2.clone()),
            ("r2", Quantity::from_si(0.5 * d2.si, m.clone())),
            (
                "dz",
                Quantity::from_si((k3_value + CUTWATER_THICKNESS_RATIO) * d2.si, m.clone()),
            ),
            ("eps_sp", quantity(2.0 * std::f64::consts::PI, one.clone())),
        ],
    )?;

    let volute = VoluteResult {
        base_radius: with_unit(r3, &m),
        width: with_unit(b3, &m),
        inlet_velocity: with_unit(c3, &ms),
        throat_area: with_unit(a360, &m2),
        throat_diameter: with_unit(d360, &m),
        outer_radius: with_unit(r4, &m),
        momentum_throat_diameter: with_unit(momentum_throat_diameter, &m),
        wrap_deg: options.volute_wrap_deg,
        sections,
    };

    let (seal_leakage_ratio, seal_eta_v, seal_without_drive) = seal_leakage(
        &registry,
        input,
        d1.si,
        d2.si,
        u2.si,
        cu2.si,
        options.seal_clearance,
    )?;

    let sigma = evaluate(
        &registry,
        "design.thoma_sigma",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
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
    // NPSH3 del punto con la meridional del álabe, cm0 (Gülich ec. 6.10).
    // En el ojo automático cm0 = c0. Si se fija c0 y no cm0, cm0 toma c0.
    let npshr_design = evaluate(
        &registry,
        "design.npsh3_inlet",
        &[
            ("c1m", Quantity::from_si(selection.cm0, ms.clone())),
            ("u1", u1.clone()),
            ("lambda_c", quantity(NPSH_LAMBDA_C, one.clone())),
            ("lambda_w", quantity(NPSH_LAMBDA_W, one.clone())),
            ("g", Quantity::from_si(9.806_65, unit("m/s2")?)),
        ],
    )?;
    let w2 = Quantity::from_si(cm2.si.hypot(u2.si - cu2.si), ms.clone());

    // Transición del NPSH3 (Gülich §6.3.3, ec. 6.17–6.20). τ1 entra en β1B
    // y en QSF. A1q es la garganta con espesor (A1·sen β1B/τ1): con τ1 > 1, QW < QSF.
    let deg = unit("deg")?;
    let m2 = unit("m2")?;
    let (beta1b, tau1) = leading_edge(
        &registry,
        incidence_deg,
        selection.cm0,
        u1.si,
        d1.si,
        outlet.thickness,
        selection.blade_count,
    )?;
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
            ("tau", quantity(tau1, one.clone())),
        ],
    )?;
    let inlet_area = std::f64::consts::PI * d1.si * b1.si;
    let inlet_deceleration = inlet_throat_ratio(
        qth.si,
        inlet_throat_area(inlet_area, beta1b, tau1),
        selection.cm0,
        u1.si,
    );
    let throat_flow = evaluate(
        &registry,
        "design.throat_flow",
        &[
            ("A1", Quantity::from_si(inlet_area, m2.clone())),
            (
                "A1q",
                Quantity::from_si(inlet_throat_area(inlet_area, beta1b, tau1), m2.clone()),
            ),
            ("u1m", u1.clone()),
            ("eta_v", eta_v_quantity.clone()),
        ],
    )?;
    // Qc de Gülich 6.22a (con c2m): caudal del rodete en que c3q = c2, con
    // `a = fq·A2·QLe/(zLe·A3q·QLa)`, fq = zLe = 1, QLe = Q (lo que pasa por la
    // voluta) y QLa = Qth (lo que pasa por el rodete). QLa,c pasa al caudal de
    // la bomba con QLe/QLa. Existe siempre que a > 1; con a ≤ 1 la garganta es
    // mayor que A2·ηv, c3q no llega a c2 en ningún caudal y Qc se toma como
    // `NO_MATCHING_FLOW_RATIO`·Qd.
    let a2_outlet = std::f64::consts::PI * d2.si * b2.si;
    let a_ratio = a2_outlet * input.q.si / (volute.throat_area.si * qth.si);
    let volute_matching_flow = if a_ratio > 1.0 {
        let impeller_flow = evaluate(
            &registry,
            "design.volute_matching_flow_cm",
            &[
                ("u2", u2.clone()),
                ("A2", Quantity::from_si(a2_outlet, m2.clone())),
                ("slip", quantity(slip, one.clone())),
                ("a", quantity(a_ratio, one.clone())),
                ("tau", quantity(tau2, one.clone())),
                (
                    "beta2",
                    Quantity::from_si(selection.beta2_deg.to_radians(), deg.clone()),
                ),
            ],
        )?;
        Quantity::from_si(impeller_flow.si * input.q.si / qth.si, m3s.clone())
    } else {
        Quantity::from_si(NO_MATCHING_FLOW_RATIO * input.q.si, m3s.clone())
    };
    let pressure_side_flow = 0.5 * (shockless_flow.si + throat_flow.si);
    let transition_ratio = volute_matching_flow.si.min(pressure_side_flow) / input.q.si;
    let recirculation_onset = evaluate(
        &registry,
        "design.recirculation_onset",
        &[("nq", quantity(nq, one.clone()))],
    )?
    .value();
    let blade = geometry::camber_line(
        (0.5 * d1.si, 0.5 * d2.si),
        (beta1b, selection.beta2_deg.to_radians()),
        (selection.cm0, cm2.si),
        qth.si,
        geometry::BLADE_STEPS,
    );
    let blade_loading = 2.0 * std::f64::consts::PI * selection.psi
        / (eta_h
            * f64::from(selection.blade_count)
            * (geometry::blade_length(&blade) / d2.si)
            * ((w1.si + w2.si) / u2.si));
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
        p_disk_loss: with_unit(p_disk_loss, &w),
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
        inlet_deceleration,
        blade_loading,
        volute_matching_flow: with_unit(volute_matching_flow, &m3s),
        transition_ratio,
        recirculation_onset,
        leakage_eta_v,
        mechanical_eta_estimate,
        disk_friction_ratio,
        statistical_eta_h,
        loss_eta,
        seal_leakage_ratio,
        seal_eta_v,
        slip,
        cu2,
        zero_head_flow_ratio,
        head_slope,
        volute,
        eye_excludes_hub,
        diagnostics: Diagnostics::new(),
    };

    let curve_max = performance_curve(input, &result, POWER_CURVE_POINTS)
        .iter()
        .map(|point| point.p_shaft.si)
        .fold(result.p_shaft.si, f64::max);
    if curve_max > result.p_max.si {
        result.p_max = Quantity::from_si(curve_max, w);
        result.torque_max = evaluate(
            &registry,
            "mech.shaft_torque",
            &[("P", result.p_max.clone()), ("n", input.n.clone())],
        )?;
    }

    result.diagnostics = diagnostics_for(&result, input, options);
    if seal_without_drive {
        result.diagnostics.push(
            Diagnostic::info(
                "HM-DES-032",
                "la altura a través del anillo delantero no es positiva: la fuga de \
                 contraste se tomó nula (Qsp = 0, ηv = 1)",
            )
            .with_hint(
                "El grado de reacción cinemático y la rotación de la cámara lateral \
                 no dejan ΔH. El ηv del método no cambia.",
            ),
        );
    }
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
    // La misma c1m que el NPSH3 del punto: cm0 en el álabe.
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
///   `σ = 2g·NPSH/u1²` (ec. (6.5)) y FF = 1 (agua). FR = 1 solo si
///   0,8 < q ≤ 1,1 y q ≤ qSF; si no, 1,2. q = 0,8 exacto se queda en 1,2.
///
/// `shockless_ratio` es QSF/Qd. Por encima de 50 m/s la tabla pide
/// experiencia o datos de cavidad; se aplica solo el mínimo y
/// `npsh_diagnostics` lo avisa.
pub fn required_npsha(npsh3: f64, u1: f64, ratio: f64, shockless_ratio: f64) -> f64 {
    let minimum = (1.25 * npsh3).max(npsh3 + 0.6);
    if !(10.0..=50.0).contains(&u1) {
        return minimum;
    }
    let risk = if ratio > 0.8 && ratio <= 1.1 && ratio <= shockless_ratio {
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

/// Rotación del fluido en las cámaras laterales sin fuga y con cámaras
/// abiertas (Gülich 2020, ec. 3.10.8 con rw = r2 y tax = 0).
const DISK_K0: f64 = 0.5;

/// Pérdidas de todas las superficies del rodete frente a las de las caras
/// planas de las tapas (Gülich 2020, tabla 3.10, rodetes radiales típicos).
const DISK_F_GEO: f64 = 1.22;

/// Longitud del anillo de desgaste frente a su diámetro: Gülich 2020, p. 111,
/// da 0,1..0,14; se toma el centro.
const SEAL_LENGTH_RATIO: f64 = 0.12;

/// Pérdida de entrada y salida de la holgura (Gülich 2020, ec. 3.7.8: 1..1,2).
const SEAL_ZETA: f64 = 1.1;

/// Rugosidad equivalente de un anillo mecanizado, m (el ejemplo de Gülich 2020,
/// p. 111: 4 µm).
const SEAL_ROUGHNESS: f64 = 4.0e-6;

/// Fuga por el anillo delantero (Gülich 2020, tabla 3.7).
///
/// Devuelve `(Qsp/Q, ηv, sin altura motriz)`. El anillo está en el ojo
/// (dsp = d1). RG es cinemático, `(1 − cu2/(2·u2))` entre 0,2 y 0,95, no
/// la constante 0,75. λ se resuelve por bisección: cax(λ) y λ(cax) se
/// cruzan, y la sustitución directa oscila cuando manda el régimen laminar.
/// Se acepta cuando el cambio relativo baja de 1e-4 y entonces se recalcula
/// cax con esa λ. Si ΔH ≤ 0 no hay fuga (Qsp = 0, ηv = 1) y el tercer valor
/// es verdadero. Si no hay raíz, error.
fn seal_leakage(
    registry: &Registry,
    input: &DesignInput,
    d1: f64,
    d2: f64,
    u2: f64,
    cu2: f64,
    clearance: Option<f64>,
) -> Result<(f64, f64, bool), DesignError> {
    let one = Unit::dimensionless();
    let m = unit("m")?;
    let ms = unit("m/s")?;
    let m2s = unit("m2/s")?;
    let g = Quantity::from_si(9.806_65, unit("m/s2")?);
    let metres = |value: f64| Quantity::from_si(value, m.clone());
    let diametral = match clearance {
        Some(value) => value,
        None => {
            evaluate(
                registry,
                "mech.seal_clearance",
                &[("dsp", metres(d1)), ("d_ref", metres(0.1))],
            )?
            .si
        }
    };
    if !diametral.is_finite() || diametral <= 0.0 || !d1.is_finite() || d1 <= 0.0 {
        return Err(DesignError::Input(
            "la holgura y el diámetro del anillo tienen que ser positivos".to_string(),
        ));
    }
    let s = 0.5 * diametral;
    let length = SEAL_LENGTH_RATIO * d1;
    let nu = Quantity::from_si(fluid_nu(input), m2s.clone());
    let reaction = if u2 > 0.0 && cu2.is_finite() {
        (1.0 - cu2 / (2.0 * u2)).clamp(0.2, 0.95)
    } else {
        return Err(DesignError::Input(
            "el grado de reacción del anillo necesita u2 positivo y cu2 finito".to_string(),
        ));
    };
    let k = evaluate(
        registry,
        "design.seal_rotation_factor",
        &[
            ("u2", Quantity::from_si(u2, ms.clone())),
            ("d2", metres(d2)),
            ("nu", nu.clone()),
            ("s", metres(s)),
            ("dsp", metres(d1)),
            ("L", metres(length)),
        ],
    )?;
    let dh = evaluate(
        registry,
        "design.seal_pressure_difference",
        &[
            ("RG", quantity(reaction, one.clone())),
            ("H", input.h.clone()),
            ("k", k),
            ("u2", Quantity::from_si(u2, ms.clone())),
            ("g", g.clone()),
            ("dsp", metres(d1)),
            ("d2", metres(d2)),
        ],
    )?;
    if !dh.si.is_finite() {
        return Err(DesignError::Input(
            "la altura a través del anillo no es finita".to_string(),
        ));
    }
    if dh.si <= 0.0 {
        return Ok((0.0, 1.0, true));
    }
    let usp = Quantity::from_si(
        0.5 * d1 * 2.0 * std::f64::consts::PI * input.n.si,
        ms.clone(),
    );
    let axial = |lambda: f64| {
        evaluate(
            registry,
            "design.seal_axial_velocity",
            &[
                ("g", g.clone()),
                ("dH", dh.clone()),
                ("zeta", quantity(SEAL_ZETA, one.clone())),
                ("lambda", quantity(lambda, one.clone())),
                ("L", metres(length)),
                ("s", metres(s)),
            ],
        )
    };
    let friction_at = |lambda: f64| -> Result<(f64, f64), DesignError> {
        let cax = axial(lambda)?.si;
        if !cax.is_finite() || cax <= 0.0 {
            return Err(DesignError::Input(
                "la velocidad en el anillo no es positiva".to_string(),
            ));
        }
        // 3.7.13 (turbulento) va dentro de la fórmula del pack. Con Re* < 2000
        // (y por tanto Re < 2000) vale el régimen laminar: λ0 = 96/Re (3.7.11,
        // sin excentricidad) por el factor de rotación 3.7.14. La regla
        // máx(λlam; λtur) es solo para la transición y con Re ≲ 6 la
        // correlación turbulenta tiene un polo.
        let re = 2.0 * s * cax / nu.si;
        let reu = 2.0 * s * usp.si / nu.si;
        let laminar = seal_rotation_is_laminar(re, reu);
        let friction = |usp: Quantity| {
            evaluate(
                registry,
                "design.seal_friction_coefficient",
                &[
                    ("s", metres(s)),
                    ("cax", Quantity::from_si(cax, ms.clone())),
                    ("nu", nu.clone()),
                    ("eps", metres(SEAL_ROUGHNESS)),
                    ("usp", usp),
                ],
            )
        };
        let next = if laminar {
            let lambda0 = evaluate(
                registry,
                "design.seal_friction_laminar",
                &[("Re", quantity(re, one.clone()))],
            )?
            .si;
            let factor = evaluate(
                registry,
                "design.seal_rotation_laminar",
                &[("Reu", quantity(reu, one.clone()))],
            )?
            .si;
            lambda0 * factor
        } else {
            friction(usp.clone())?.si
        };
        if !next.is_finite() || next <= 0.0 {
            return Err(DesignError::Input(
                "el coeficiente de rozamiento del anillo no es positivo".to_string(),
            ));
        }
        Ok((cax, next))
    };
    // A λ chico la fricción queda por encima de λ; a λ grande la rama
    // laminar crece como λ^0,875 y el residuo pasa a negativo.
    let mut low = 1.0e-4;
    let mut high = 0.2;
    let mut low_gap = friction_at(low)?.1 - low;
    let mut high_gap = friction_at(high)?.1 - high;
    for _ in 0..24 {
        if low_gap < 0.0 {
            low *= 0.5;
            low_gap = friction_at(low)?.1 - low;
        } else if high_gap > 0.0 {
            high *= 2.0;
            high_gap = friction_at(high)?.1 - high;
        } else {
            break;
        }
    }
    if low_gap < 0.0 || high_gap > 0.0 || !low_gap.is_finite() || !high_gap.is_finite() {
        return Err(DesignError::Input(
            "la fuga del anillo no converge".to_string(),
        ));
    }
    let lambda = hm_math::bisect(
        |lambda| friction_at(lambda).map_or(f64::NAN, |(_, next)| next - lambda),
        low,
        high,
        1.0e-10,
        80,
    )
    .map_err(|error| DesignError::Input(format!("la fuga del anillo no converge: {error}")))?;
    let (cax, next) = friction_at(lambda)?;
    if (next - lambda).abs() > 1.0e-4 * lambda.max(1.0e-12) {
        return Err(DesignError::Input(
            "la fuga del anillo no converge".to_string(),
        ));
    }
    if !cax.is_finite() || cax <= 0.0 {
        return Err(DesignError::Input(
            "la velocidad en el anillo no es positiva".to_string(),
        ));
    }
    let qsp = evaluate(
        registry,
        "design.seal_leakage_flow",
        &[
            ("dsp", metres(d1)),
            ("s", metres(s)),
            ("cax", Quantity::from_si(cax, ms)),
        ],
    )?;
    let eta_v = evaluate(
        registry,
        "design.seal_volumetric_efficiency",
        &[("Q", input.q.clone()), ("Qsp", qsp.clone())],
    )?
    .value();
    if !qsp.si.is_finite() || !eta_v.is_finite() {
        return Err(DesignError::Input(
            "la fuga del anillo no es finita".to_string(),
        ));
    }
    Ok((qsp.si / input.q.si, eta_v, false))
}

/// Deceleración mínima `w2/w1` (criterio de de Haller; Gülich 2020, p. 413).
pub const DE_HALLER_MIN: f64 = 0.7;

/// Altura de la curva preliminar en `ratio` veces el caudal de diseño, m.
///
/// Desde `end_suction_v0.5`: parábola que vale H0 a caudal cero (Gülich ec.
/// 3.28), Hd en el punto de diseño y tiene allí la pendiente de la tangente a
/// la recta que pasa por Q(Hth = 0) (Gülich §4.3.5). La pendiente depende de
/// la geometría (β2, Z por el deslizamiento y b2 por φ). Es la fórmula
/// `design.curve_head_ratio`.
pub fn curve_head(input: &DesignInput, result: &DesignResult, ratio: f64) -> f64 {
    let (a, b) = curve_head_coefficients(input, result);
    let h0 = result.shutoff_head.si / input.h.si;
    input.h.si * (h0 + a * ratio + b * ratio * ratio)
}

/// Coeficientes lineal y cuadrático de H/Hd en función de Q/Qd.
fn curve_head_coefficients(input: &DesignInput, result: &DesignResult) -> (f64, f64) {
    let h0 = result.shutoff_head.si / input.h.si;
    let slope = result.head_slope;
    (2.0 * (1.0 - h0) - slope, slope - (1.0 - h0))
}

/// Caudal, en fracción de Qd, donde la parábola H(Q) tiene su máximo, si está
/// entre caudal cero y el de diseño. H0 es estadístico y no ve Z ni β2: el
/// máximo es una incompatibilidad de esa parábola.
pub fn curve_head_peak(input: &DesignInput, result: &DesignResult) -> Option<f64> {
    let (a, b) = curve_head_coefficients(input, result);
    if b >= 0.0 {
        return None;
    }
    let peak = -a / (2.0 * b);
    (peak > 0.0 && peak < 1.0).then_some(peak)
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
        "p_disk_loss" => result.p_disk_loss.si,
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

fn diagnostics_for(
    result: &DesignResult,
    input: &DesignInput,
    options: &DesignOptions,
) -> Diagnostics {
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

    if !eta_h_flow_in_range(input.q.si) {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-033",
                format!(
                    "Q = {:.4} m³/s fuera de {}..{} m³/s, el rango del ηh estadístico de Gülich \
                     (ec. 3.9.6): ηh = {:.3} es una extrapolación",
                    input.q.si, ETA_H_FLOW_RANGE.0, ETA_H_FLOW_RANGE.1, result.eta_h
                ),
            )
            .with_hint("Contrasta ηh, y con él β2 y la potencia, con una bomba semejante."),
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

    if let Some(peak) = curve_head_peak(input, result) {
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-031",
                format!(
                    "la parábola H(Q) tiene un máximo en {peak:.2}·Qd ({:.1} m), por encima de H0 \
                     = {:.1} m. H0 estadístico (ec. 3.28) no ve Z ni β2, así que no casa con la \
                     pendiente de §4.3.5",
                    curve_head(input, result, peak),
                    result.shutoff_head.si
                ),
            )
            .with_hint(
                "Esto no demuestra por sí solo una bomba inestable: la parábola junta un H0 \
                 estadístico con una pendiente geométrica.",
            ),
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

    let (ratio_low, ratio_high) = INLET_THROAT_RATIO_BAND;
    if !(ratio_low..=ratio_high).contains(&result.inlet_deceleration) {
        let (what, hint) = if result.inlet_deceleration > ratio_high {
            (
                "garganta estrecha",
                "Riesgo de cavitación a caudal alto: sube β1B o el área de entrada.",
            )
        } else {
            (
                "deceleración excesiva",
                "Riesgo de recirculación anticipada a la entrada: baja β1B o el área de entrada.",
            )
        };
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-035",
                format!(
                    "w1q/w1m = {:.2} en el punto de diseño, fuera de {ratio_low}..{ratio_high} \
                     (Gülich 2020, p. 415): {what}. Depende de la incidencia i1 (por \
                     defecto 2°) y se calcula con el borde de ataque 2D, que no \
                     distingue líneas de corriente",
                    result.inlet_deceleration
                ),
            )
            .with_hint(format!(
                "{hint} Si conoces la incidencia real, fíjala con `incidence_deg` (0..4°, p. 411)."
            )),
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

    let loading_limit = (1.0 + BLADE_LOADING_MARGIN) * blade_loading_allowable(result.nq);
    if result.nq <= CYLINDRICAL_BLADE_MAX_NQ && result.blade_loading > loading_limit {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-036",
                format!(
                    "carga del álabe ξeff = {:.2} por encima de {:.2}, la admisible ξal = {:.2} \
                     más un {:.0} % (Gülich 2020, ec. 7.1.10–7.1.11): el álabe es corto para \
                     su número",
                    result.blade_loading,
                    loading_limit,
                    blade_loading_allowable(result.nq),
                    100.0 * BLADE_LOADING_MARGIN
                ),
            )
            .with_hint(
                "Sube Z o alarga el álabe; Gülich recomienda quedar un 10 % por debajo de ξal \
                 (p. 415).",
            ),
        );
    }

    if result.selection.blade_count > BLADE_COUNT_MAX_RECOMMENDED {
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-034",
                format!(
                    "Z = {}: Gülich no recomienda 8 o más álabes por la estabilidad de la curva \
                     Q-H (p. 408)",
                    result.selection.blade_count
                ),
            )
            .with_hint(if options.blade_count.is_some() {
                "Z fijado a mano: el motor no lo eligió. Con 5..7 álabes mira si β2 queda en \
                 15..45°."
            } else {
                "Con 5..7 álabes β2 se sale de 15..45°; revisa ψ, φ o el ojo."
            }),
        );
    }

    let two_tongues = options.volute_channels.unwrap_or(1) == 2;
    if options.volute_wrap_deg.is_some() != two_tongues {
        diagnostics.push(
            Diagnostic::info(
                "HM-DES-038",
                if two_tongues {
                    "volute_channels = 2 sin `volute_wrap_deg`: el empuje radial se calcula con \
                     voluta simple"
                } else {
                    "`volute_wrap_deg` sin `volute_channels = 2`: el empuje radial se calcula con \
                     voluta doble, pero las lenguas y la selección de Z son de voluta simple"
                },
            )
            .with_hint("Una voluta doble lleva dos lenguas y su ángulo de envolvente (90..180°)."),
        );
    }

    if options.blade_count.is_some_and(|z| z.is_multiple_of(2))
        && options.volute_channels.is_some_and(|z| z.is_multiple_of(2))
    {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-037",
                format!(
                    "Z = {} y {} lenguas de voluta, ambos pares: Gülich evita un Z par con un \
                     zLe par (p. 491) por las pulsaciones de presión",
                    result.selection.blade_count,
                    options.volute_channels.unwrap_or(1)
                ),
            )
            .with_hint("Usa un Z impar, o deja que el motor lo elija."),
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
            let shockless = result.shockless_flow.si / q_design;
            let required = required_npsha(npsh3, result.u1.si, ratio, shockless);
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

/// Valida las tres entradas de duty, antes de estimar ηm.
pub fn validate_duty(q: &Quantity, h: &Quantity, n: &Quantity) -> Result<(), FieldError> {
    validate_duty_dimensions(q, h, n)?;
    validate_duty_positivity(q, h, n)
}

fn validate_duty_dimensions(q: &Quantity, h: &Quantity, n: &Quantity) -> Result<(), FieldError> {
    require_dimension("Q", q, "m3/s")?;
    require_dimension("H", h, "m")?;
    require_dimension("n", n, "1/s")?;
    Ok(())
}

fn validate_duty_positivity(q: &Quantity, h: &Quantity, n: &Quantity) -> Result<(), FieldError> {
    require_positive("Q", q.si)?;
    require_positive("H", h.si)?;
    require_positive("n", n.si)?;
    Ok(())
}

fn validate_input(input: &DesignInput) -> Result<(), FieldError> {
    // Mismo orden que la validación original: dimensiones de duty, rho,
    // NPSHa, positividad de duty y rho, y ηm.
    validate_duty_dimensions(&input.q, &input.h, &input.n)?;
    require_dimension("rho", &input.rho, "kg/m3")?;

    if let Some(npsha) = &input.npsha {
        require_dimension("NPSHa", npsha, "m")?;
        if !npsha.si.is_finite() {
            return Err(FieldError {
                name: "NPSHa",
                message: "NPSHa no es finito".to_string(),
            });
        }
    }

    validate_duty_positivity(&input.q, &input.h, &input.n)?;
    require_positive("rho", input.rho.si)?;

    if !input.eta_m.is_finite() || input.eta_m <= 0.0 || input.eta_m >= 1.0 {
        return Err(FieldError {
            name: "eta_m",
            message: "la eficiencia mecánica debe ser mayor que 0 y menor que 1".to_string(),
        });
    }

    Ok(())
}

fn require_dimension(
    name: &'static str,
    quantity: &Quantity,
    unit_text: &str,
) -> Result<(), FieldError> {
    let expected = parse_unit(unit_text).map_err(|error| FieldError {
        name,
        message: error.to_string(),
    })?;
    if quantity.dim() != expected.dim {
        return Err(FieldError {
            name,
            message: format!("{name} debe tener dimensiones de {unit_text}"),
        });
    }
    Ok(())
}

fn require_positive(name: &'static str, value: f64) -> Result<(), FieldError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(FieldError {
            name,
            message: format!("{name} debe ser finito y positivo"),
        });
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
