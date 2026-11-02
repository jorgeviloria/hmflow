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
    pub torque: Quantity,
    pub torque_max: Quantity,
    pub npshr_design: Quantity,
    pub shutoff_head: Quantity,
    pub diagnostics: Diagnostics,
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

/// Calcula un diseño preliminar end-suction.
pub fn design(input: &DesignInput) -> Result<DesignResult, DesignError> {
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

    let selection = DesignSelection {
        psi: head_coefficient(nq),
        phi: flow_coefficient(nq),
        beta2_deg: blade_angle_deg(nq),
        blade_count: blade_count(nq),
        c0: inlet_velocity(nq),
        cm0: inlet_meridional_velocity(nq),
        a: 4.5,
        j: 1.03,
        margin: 1.1,
    };

    let n_rpm = input.n.si * 60.0;
    let specific_diameter_mm = selection.a * 1000.0 * (input.q.si / n_rpm).cbrt();

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

    let sigma = 0.0012 * nq.powf(4.0 / 3.0);
    let npshr_design = Quantity::from_si(sigma * input.h.si, m.clone());
    let shutoff_ratio = 1.25 + 0.004 * nq;
    let shutoff_head = Quantity::from_si(shutoff_ratio * input.h.si, m.clone());

    let mut result = DesignResult {
        nq,
        specific_diameter_mm,
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
        torque: with_unit(torque, &nm),
        torque_max: with_unit(torque_max, &nm),
        npshr_design,
        shutoff_head,
        diagnostics: Diagnostics::new(),
    };

    result.diagnostics = diagnostics_for(&result, input);
    Ok(result)
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
    let count = points.max(2);
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
        let ratio = 0.3 + (1.3 - 0.3) * index as f64 / (count - 1) as f64;
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
        let npshr = npshr_design * ratio.powi(2);

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

    if let Some(npsha) = &input.npsha {
        let margin = npsha.si - result.npshr_design.si;
        if margin < 0.5 {
            diagnostics.push(
                Diagnostic::warning(
                    "HM-DES-007",
                    format!("NPSHa - NPSHr = {margin:.2} m; el margen preliminar es 0.5 m"),
                )
                .with_hint("Aumenta el NPSHa o reduce la velocidad de giro."),
            );
        } else {
            diagnostics.push(Diagnostic::info(
                "HM-DES-006",
                format!("NPSHa - NPSHr = {margin:.2} m"),
            ));
        }
    }

    diagnostics
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

    if !input.eta_m.is_finite() || !(0.0..1.0).contains(&input.eta_m) {
        return Err(DesignError::Input(
            "la eficiencia mecánica debe estar entre 0 y 1".to_string(),
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
