//! Diseño preliminar de bombas centrífugas end-suction OH1/OH2.
//!
//! El motor reproduce el ejemplo resuelto D1.A1 de Srinivasan 2017. Las
//! selecciones empíricas (psi, phi, beta2, número de álabes, velocidades de
//! entrada) están documentadas y se aplican por rango de `nq`. El tramo
//! 25–40 se ancla en D1.A1; los demás tramos son selecciones preliminares
//! pendientes de contraste experimental.

use std::collections::BTreeMap;

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

/// Parámetros que genera o consume el diseño end-suction v0.1.
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

/// Radio base de la voluta por defecto, `R3 = k3·D2/2` (Srinivasan 2017, ec. 5.7).
pub const DEFAULT_K3: f64 = 1.04;

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

/// Intervalo de validez de [`water_density`], en °C.
pub const WATER_DENSITY_RANGE_C: (f64, f64) = (0.0, 150.0);

/// Densidad del agua líquida a 1 atm, en kg/m³, para una temperatura en
/// kelvin.
///
/// Correlación de Kell (1975), J. Chem. Eng. Data 20(1), 97–105, válida de
/// 0 a 150 °C. Da 998,20 kg/m³ a 20 °C y 971,80 kg/m³ a 80 °C. Fuera de ese
/// intervalo devuelve `None`.
pub fn water_density(temperature_k: f64) -> Option<f64> {
    let t = temperature_k - 273.15;
    if !t.is_finite() || !(WATER_DENSITY_RANGE_C.0..=WATER_DENSITY_RANGE_C.1).contains(&t) {
        return None;
    }
    let numerator =
        999.839_52 + 16.945_176 * t - 7.987_040_1e-3 * t.powi(2) - 46.170_461e-6 * t.powi(3)
            + 105.563_02e-9 * t.powi(4)
            - 280.542_53e-12 * t.powi(5);
    Some(numerator / (1.0 + 16.879_850e-3 * t))
}

/// Selecciones que el motor toma de sus tablas para un `nq` dado, sin
/// ninguna opción del proyecto.
pub fn table_selection(nq: f64) -> DesignSelection {
    DesignSelection {
        psi: head_coefficient(nq),
        phi: flow_coefficient(nq),
        beta2_deg: blade_angle_deg(nq),
        blade_count: blade_count(nq),
        c0: inlet_velocity(nq),
        cm0: inlet_meridional_velocity(nq),
        a: 4.5,
        j: 1.03,
        margin: 1.1,
    }
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

    let table = table_selection(nq);
    let selection = DesignSelection {
        psi: options.psi.unwrap_or(table.psi),
        phi: options.phi.unwrap_or(table.phi),
        beta2_deg: options
            .beta2_rad
            .map(f64::to_degrees)
            .unwrap_or(table.beta2_deg),
        blade_count: options.blade_count.unwrap_or(table.blade_count),
        c0: options.c0.unwrap_or(table.c0),
        cm0: options.cm0.unwrap_or(table.cm0),
        a: options.a.unwrap_or(table.a),
        j: options.j.unwrap_or(table.j),
        margin: options.margin.unwrap_or(table.margin),
    };
    let k3_value = options.k3.unwrap_or(DEFAULT_K3);
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
        &[("nq", quantity(nq, one.clone())), ("H", input.h.clone())],
    )?;

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
        shutoff_head,
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

/// Margen mínimo preliminar NPSHa − NPSHr, en metros.
pub const NPSH_MARGIN_M: f64 = 0.5;

/// Subida del NPSHr a caudal parcial: `NPSHr = NPSHr_d·(1 + k·(1 − Q/Qd)²)`.
///
/// Coeficiente fenomenológico sin fuente ni calibración. Solo representa la
/// tendencia (el NPSHr vuelve a subir por recirculación a caudal parcial);
/// el valor real depende del rodete y se debe medir.
pub const PART_LOAD_NPSH_RISE: f64 = 1.5;

/// NPSHr relativo al de diseño para una fracción `ratio = Q/Qd`.
///
/// Por encima del caudal de diseño crece con `(Q/Qd)²`; por debajo usa
/// [`PART_LOAD_NPSH_RISE`].
pub fn npshr_ratio(ratio: f64) -> f64 {
    if ratio >= 1.0 {
        ratio.powi(2)
    } else {
        1.0 + PART_LOAD_NPSH_RISE * (1.0 - ratio).powi(2)
    }
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
    let h_design = input.h.si;
    let eta_design = result.eta;
    let rho = input.rho.si;
    let g = 9.806_65;
    let shutoff_ratio = result.shutoff_head.si / h_design;
    let npshr_design = result.npshr_design.si;

    let m = Unit::new("m", Dimension::LENGTH, 1.0);
    let m3h = Unit::new("m3/h", Dimension::VOLUME.div(Dimension::TIME), 1.0 / 3600.0);
    let kw = Unit::new("kW", Dimension::POWER, 1000.0);

    let mut curve = Vec::with_capacity(count);

    for index in 0..count {
        let ratio =
            CURVE_RANGE.0 + (CURVE_RANGE.1 - CURVE_RANGE.0) * index as f64 / (count - 1) as f64;
        let q = q_design * ratio;
        let h = h_design * (shutoff_ratio - (shutoff_ratio - 1.0) * ratio.powi(2));
        let eta = eta_design
            * if ratio <= 1.0 {
                1.0 - 0.60 * (ratio - 1.0).powi(2)
            } else {
                1.0 - 0.35 * (ratio - 1.0).powi(2)
            };
        let eta = eta.max(1e-4);
        let p_shaft = rho * g * q * h / eta;
        let npshr = npshr_design * npshr_ratio(ratio);

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
    if !(0.02..=0.15).contains(&width_ratio) {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-003",
                format!("B2/D2 = {width_ratio:.3} fuera del rango preliminar 0.02..0.15"),
            )
            .with_hint("Revisa el coeficiente de caudal y el diámetro exterior."),
        );
    }

    if !(15.0..=35.0).contains(&result.selection.beta2_deg) {
        diagnostics.push(Diagnostic::warning(
            "HM-DES-004",
            format!(
                "β2 = {:.1}° fuera del rango preliminar 15..35°",
                result.selection.beta2_deg
            ),
        ));
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

/// Peor margen NPSHa − NPSHr entre dos fracciones del caudal de diseño.
///
/// Devuelve el margen en metros y el caudal en m3/h donde ocurre.
fn worst_npsh_margin(
    npsha: f64,
    npshr_design: f64,
    q_design: f64,
    from: f64,
    to: f64,
) -> (f64, f64) {
    const STEPS: usize = 100;
    (0..=STEPS)
        .map(|index| from + (to - from) * index as f64 / STEPS as f64)
        .map(|ratio| {
            (
                npsha - npshr_design * npshr_ratio(ratio),
                q_design * ratio * 3600.0,
            )
        })
        .fold((f64::INFINITY, 0.0), |worst, candidate| {
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
    let npshr_design = result.npshr_design.si;
    let q_design = input.q.si;
    let (por_low, por_high) = PREFERRED_OPERATING_REGION;

    let (margin, flow) = worst_npsh_margin(npsha, npshr_design, q_design, por_low, por_high);
    if margin < NPSH_MARGIN_M {
        diagnostics.push(
            Diagnostic::error(
                "HM-DES-007",
                format!(
                    "margen NPSHa - NPSHr = {margin:.2} m a Q = {flow:.0} m3/h dentro de la región \
                     preferente ({por_low}..{por_high} Qd); el mínimo preliminar es {NPSH_MARGIN_M} m"
                ),
            )
            .with_hint("Aumenta el NPSHa o reduce la velocidad de giro."),
        );
        return;
    }
    diagnostics.push(Diagnostic::info(
        "HM-DES-006",
        format!(
            "margen NPSHa - NPSHr mínimo en la región preferente ({por_low}..{por_high} Qd) = {margin:.2} m"
        ),
    ));

    let (low, high) = CURVE_RANGE;
    let below = worst_npsh_margin(npsha, npshr_design, q_design, low, por_low);
    let above = worst_npsh_margin(npsha, npshr_design, q_design, por_high, high);
    let (margin, flow) = if below.0 <= above.0 { below } else { above };
    if margin < NPSH_MARGIN_M {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-011",
                format!(
                    "fuera de la región preferente el margen NPSHa - NPSHr baja a {margin:.2} m \
                     a Q = {flow:.0} m3/h"
                ),
            )
            .with_hint(
                "El NPSHr a caudal parcial usa un modelo sin calibrar; evita operar ahí o \
                 confirma el NPSHr con el fabricante.",
            ),
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

/// Coeficiente de altura `psi = 2gH/u2^2`.
fn head_coefficient(nq: f64) -> f64 {
    if nq < 25.0 {
        0.95
    } else if nq < 40.0 {
        0.87
    } else if nq < 60.0 {
        0.80
    } else {
        0.70
    }
}

/// Coeficiente de caudal `phi = cm2/u2`.
fn flow_coefficient(nq: f64) -> f64 {
    if nq < 25.0 {
        0.10
    } else if nq < 40.0 {
        0.13
    } else if nq < 60.0 {
        0.16
    } else {
        0.20
    }
}

/// Ángulo de salida del álabe, en grados.
fn blade_angle_deg(nq: f64) -> f64 {
    if nq < 25.0 {
        28.0
    } else if nq < 40.0 {
        23.5
    } else if nq < 60.0 {
        20.0
    } else {
        17.5
    }
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
