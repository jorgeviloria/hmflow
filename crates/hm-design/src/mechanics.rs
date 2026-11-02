//! Mecánica preliminar: eje por torsión, cubo y empujes hidráulicos.
//!
//! Son estimaciones para dimensionar, no comprobaciones del rotor: el eje
//! sale solo de la torsión (Srinivasan 2017, §4.14), sin flexión por el
//! empuje radial, sin velocidades críticas, chaveta, cojinetes ni cierre.
//! Nada de esto se guarda en el `.hmf`; se recalcula desde el diseño.

use std::collections::BTreeMap;

use hm_core::{Diagnostic, Diagnostics};
use hm_formula::Registry;
use hm_units::{Quantity, Unit, parse_unit};

use crate::{DesignError, DesignInput, DesignResult, PREFERRED_OPERATING_REGION, curve_head};

/// Tensión tangencial admisible del eje por defecto, Pa: la del ejemplo D1.A1
/// de Srinivasan 2017 (SS316, fu = 180 MPa con FS = 8). Elección del motor
/// para agua fría hasta que el material del eje sea un dato del proyecto; con
/// temperatura alta Srinivasan baja a 10,1 MPa (D1.B).
pub const DEFAULT_SHAFT_SHEAR_STRESS: f64 = 22.5e6;

/// dh/ds: centro del intervalo 1,2..1,3 de Srinivasan 2017, ec. (4.48).
pub const HUB_RATIO: f64 = 1.25;

/// kR,opt de una voluta simple en el punto de diseño. Gülich 2020 (tabla
/// 9.7) da 0,03..0,08; se toma el extremo alto porque el empuje sirve para
/// dimensionar eje y cojinetes.
pub const RADIAL_DESIGN_COEFFICIENT: f64 = 0.08;

/// Intervalo de nq de la fig. 9.46 de Gülich 2020; fuera de él kR0 se
/// evalúa en el extremo.
pub const RADIAL_NQ_RANGE: (f64, f64) = (10.0, 95.0);

/// Intervalo de f_ax de la ec. (9.2.12) de Gülich 2020.
pub const AXIAL_THRUST_FACTOR: (f64, f64) = (0.7, 0.9);

/// Fracción del ojo que puede tapar el cubo sin aviso: (dh/D0)² = 0,25, es
/// decir dh = D0/2. Elección del motor: D0 se calcula sin cubo (end-suction,
/// Srinivasan ec. 4.2) y, si el cubo entra en el ojo, por encima de este
/// valor c0 real sube más de un tercio.
pub const HUB_EYE_BLOCKAGE: f64 = 0.25;

/// Mecánica preliminar de un diseño, en SI.
#[derive(Debug, Clone)]
pub struct Mechanics {
    /// Par de cálculo del eje (con la sobrecarga de potencia), N·m.
    pub shaft_torque: f64,
    /// Tensión tangencial admisible usada, Pa.
    pub shaft_shear_stress: f64,
    /// Diámetro mínimo del eje por torsión, m.
    pub shaft_min_diameter: f64,
    /// Diámetro del cubo con ese eje, m.
    pub hub_diameter: f64,
    /// kR0, a válvula cerrada.
    pub radial_coefficient_shutoff: f64,
    /// kR,opt, en el punto de diseño.
    pub radial_coefficient_design: f64,
    /// Anchura con tapas usada en el empuje radial, m (hoy b2: sin espesor
    /// de las tapas).
    pub radial_width: f64,
    /// Empuje radial a válvula cerrada, N.
    pub radial_thrust_shutoff: f64,
    /// Empuje radial en el punto de diseño, N.
    pub radial_thrust_design: f64,
    /// Empuje radial en el extremo alto de la región preferente, N.
    pub radial_thrust_overload: f64,
    /// Diámetro del anillo de la tapa anterior supuesto, m (D1).
    pub front_seal_diameter: f64,
    /// Diámetro del cierre del eje supuesto, m (el del cubo).
    pub shaft_seal_diameter: f64,
    /// Empuje axial hidráulico estimado, extremos bajo y alto, N.
    pub axial_thrust: (f64, f64),
}

fn evaluate(registry: &Registry, id: &str, pairs: &[(&str, Quantity)]) -> Result<f64, DesignError> {
    let inputs: BTreeMap<String, Quantity> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Ok(registry.evaluate(id, &inputs)?.si)
}

/// Mecánica preliminar de `result` con la tensión admisible `shear_stress`
/// en Pa.
pub fn mechanics(
    input: &DesignInput,
    result: &DesignResult,
    shear_stress: f64,
) -> Result<Mechanics, DesignError> {
    if !shear_stress.is_finite() || shear_stress <= 0.0 {
        return Err(DesignError::Input(format!(
            "la tensión admisible del eje debe ser positiva: {shear_stress} Pa"
        )));
    }
    let registry = Registry::all();
    let one = Unit::dimensionless();
    let m = parse_unit("m")?;
    let pure = |value: f64| Quantity::from_si(value, one.clone());
    let metres = |value: f64| Quantity::from_si(value, m.clone());

    let shaft_torque = result.torque_max.si;
    let shaft_min_diameter = evaluate(
        &registry,
        "mech.shaft_min_diameter",
        &[
            ("T", result.torque_max.clone()),
            ("tau", Quantity::from_si(shear_stress, parse_unit("Pa")?)),
        ],
    )?;
    let hub_diameter = evaluate(
        &registry,
        "mech.hub_diameter",
        &[
            ("ds", metres(shaft_min_diameter)),
            ("k_hub", pure(HUB_RATIO)),
        ],
    )?;

    let nq = result.nq.clamp(RADIAL_NQ_RANGE.0, RADIAL_NQ_RANGE.1);
    let k0 = evaluate(
        &registry,
        "mech.radial_shutoff_coefficient",
        &[("nq", pure(nq))],
    )?;
    let k_opt = RADIAL_DESIGN_COEFFICIENT;
    let radial_width = result.b2.si;
    let radial = |ratio: f64| -> Result<f64, DesignError> {
        let k = evaluate(
            &registry,
            "mech.radial_thrust_coefficient",
            &[
                ("q", pure(ratio)),
                ("kR0", pure(k0)),
                ("kR_opt", pure(k_opt)),
            ],
        )?;
        evaluate(
            &registry,
            "mech.radial_thrust",
            &[
                ("kR", pure(k)),
                ("rho", input.rho.clone()),
                ("H", metres(curve_head(input, result, ratio))),
                ("d2", result.d2.clone()),
                ("b2tot", metres(radial_width)),
            ],
        )
    };

    let front_seal_diameter = result.d1.si;
    let shaft_seal_diameter = hub_diameter;
    let axial = |factor: f64| -> Result<f64, DesignError> {
        evaluate(
            &registry,
            "mech.axial_thrust_estimate",
            &[
                ("f_ax", pure(factor)),
                ("rho", input.rho.clone()),
                ("H", input.h.clone()),
                ("dsp", metres(front_seal_diameter)),
                ("dD", metres(shaft_seal_diameter)),
            ],
        )
    };

    Ok(Mechanics {
        shaft_torque,
        shaft_shear_stress: shear_stress,
        shaft_min_diameter,
        hub_diameter,
        radial_coefficient_shutoff: k0,
        radial_coefficient_design: k_opt,
        radial_width,
        radial_thrust_shutoff: radial(0.0)?,
        radial_thrust_design: radial(1.0)?,
        radial_thrust_overload: radial(PREFERRED_OPERATING_REGION.1)?,
        front_seal_diameter,
        shaft_seal_diameter,
        axial_thrust: (axial(AXIAL_THRUST_FACTOR.0)?, axial(AXIAL_THRUST_FACTOR.1)?),
    })
}

/// Avisos de la mecánica preliminar.
pub fn mechanics_diagnostics(mechanics: &Mechanics, result: &DesignResult) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let d0 = result.d0.si;
    let blockage = (mechanics.hub_diameter / d0).powi(2);
    if blockage > HUB_EYE_BLOCKAGE {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-027",
                format!(
                    "el cubo del eje mínimo (dh = {:.1} mm) equivale al {:.0} % del ojo \
                     (D0 = {:.1} mm, calculado sin cubo): si el cubo o la tuerca entran \
                     en el ojo, c0 sube",
                    mechanics.hub_diameter * 1000.0,
                    blockage * 100.0,
                    d0 * 1000.0
                ),
            )
            .with_hint(format!(
                "Con el cubo dentro del ojo, D0 = √(D0² + dh²) = {:.1} mm \
                 (Srinivasan 2017, ec. 4.2).",
                (d0 * d0 + mechanics.hub_diameter.powi(2)).sqrt() * 1000.0
            )),
        );
    }
    let (low, high) = RADIAL_NQ_RANGE;
    if !(low..=high).contains(&result.nq) {
        diagnostics.push(Diagnostic::info(
            "HM-DES-028",
            format!(
                "nq = {:.1} fuera de la fig. 9.46 de Gülich (nq {low}..{high}): \
                 kR0 se tomó en el extremo",
                result.nq
            ),
        ));
    }
    diagnostics
}
