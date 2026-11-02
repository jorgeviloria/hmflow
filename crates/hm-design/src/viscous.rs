//! Efecto de la viscosidad en las prestaciones (Gülich 2020, tabla 13.1).
//!
//! El rodete se diseña para el punto pedido con agua. Con un fluido viscoso
//! la misma bomba da menos caudal, menos altura y menos rendimiento; aquí se
//! estiman esos factores en el punto de diseño. El diseño no se corrige: si
//! el punto viscoso es el que importa, hay que diseñar para el punto de agua
//! equivalente (Gülich ec. 13.11), que queda pendiente.

use std::collections::BTreeMap;

use hm_core::{Diagnostic, Diagnostics};
use hm_formula::Registry;
use hm_units::{Quantity, Unit};

use crate::{DesignError, DesignInput, DesignResult};

/// Pérdida de altura a partir de la cual se avisa (2 %). Elección del motor,
/// no de Gülich: por debajo, la corrección queda dentro de la incertidumbre
/// del propio método (±15 %, Gülich p. 1014).
pub const VISCOUS_HEAD_LOSS: f64 = 0.02;

/// Pérdida de rendimiento a partir de la cual se avisa (5 %).
pub const VISCOUS_EFFICIENCY_LOSS: f64 = 0.05;

/// Viscosidad cinemática del agua de referencia, en m²/s: agua a 20 °C según
/// IAPWS R12-08 con la densidad de Kell (1,0034 cSt). Las prestaciones del
/// motor son las de agua fría y los factores se toman relativos a ella.
pub const REFERENCE_WATER_NU: f64 = 1.003_4e-6;

/// Viscosidad cinemática máxima del método, en m²/s (tabla 13.1).
pub const VISCOUS_MAX_NU: f64 = 4000e-6;

/// Intervalo de nq del método (tabla 13.1).
pub const VISCOUS_NQ_RANGE: (f64, f64) = (7.0, 50.0);

/// Factores de corrección y prestaciones con el fluido viscoso en el punto
/// de diseño.
#[derive(Debug, Clone)]
pub struct ViscousCorrection {
    /// Viscosidad cinemática, m²/s.
    pub nu: f64,
    /// Re modificado de Gülich (13.1.2).
    pub reynolds_mod: f64,
    pub f_q: f64,
    pub f_h: f64,
    pub f_eta: f64,
    pub flow: Quantity,
    pub head: Quantity,
    pub efficiency: f64,
    pub shaft_power: Quantity,
}

fn evaluate(registry: &Registry, id: &str, pairs: &[(&str, Quantity)]) -> Result<f64, DesignError> {
    let inputs: BTreeMap<String, Quantity> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Ok(registry.evaluate(id, &inputs)?.si)
}

/// Corrección por viscosidad del punto de diseño de `result` para una
/// viscosidad cinemática `nu` en m²/s.
pub fn viscous_correction(
    input: &DesignInput,
    result: &DesignResult,
    nu: f64,
) -> Result<ViscousCorrection, DesignError> {
    if !nu.is_finite() || nu <= 0.0 {
        return Err(DesignError::Input(format!(
            "la viscosidad debe ser positiva: {nu} m2/s"
        )));
    }
    let registry = Registry::all();
    let one = Unit::dimensionless();
    let factors = |viscosity: f64| -> Result<(f64, f64, f64), DesignError> {
        let reynolds_mod = evaluate(
            &registry,
            "design.viscous_reynolds",
            &[
                ("n", input.n.clone()),
                ("d2", result.d2.clone()),
                (
                    "nu",
                    Quantity::from_si(viscosity, hm_units::parse_unit("m2/s")?),
                ),
                ("nq", Quantity::from_si(result.nq, one.clone())),
            ],
        )?;
        let re = Quantity::from_si(reynolds_mod, one.clone());
        let f_h = evaluate(
            &registry,
            "design.viscous_head_factor",
            &[("Re_mod", re.clone())],
        )?;
        let f_eta = evaluate(
            &registry,
            "design.viscous_efficiency_factor",
            &[("Re_mod", re)],
        )?;
        Ok((reynolds_mod, f_h, f_eta))
    };

    // Los factores de Gülich pasan de prestaciones medidas con agua fría a
    // prestaciones con el fluido viscoso. Con el agua misma no valen 1
    // (fη ≈ 0,987 a 20 °C), así que se toman relativos al agua de referencia
    // y no se atribuye ganancia por debajo de su viscosidad.
    let (reynolds_mod, f_h_fluid, f_eta_fluid) = factors(nu)?;
    let (_, f_h_water, f_eta_water) = factors(REFERENCE_WATER_NU)?;
    let f_h = (f_h_fluid / f_h_water).min(1.0);
    let f_eta = (f_eta_fluid / f_eta_water).min(1.0);
    let f_q = f_h;

    let flow = f_q * input.q.si;
    let head = f_h * input.h.si;
    let efficiency = f_eta * result.eta;
    let shaft_power = input.rho.si * 9.806_65 * flow * head / efficiency;

    Ok(ViscousCorrection {
        nu,
        reynolds_mod,
        f_q,
        f_h,
        f_eta,
        flow: Quantity::from_si(flow, input.q.unit.clone()),
        head: Quantity::from_si(head, input.h.unit.clone()),
        efficiency,
        shaft_power: Quantity::from_si(shaft_power, result.p_shaft.unit.clone()),
    })
}

/// Avisos de la corrección por viscosidad.
pub fn viscous_diagnostics(correction: &ViscousCorrection, nq: f64) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let (low, high) = VISCOUS_NQ_RANGE;
    if correction.nu > VISCOUS_MAX_NU || !(low..=high).contains(&nq) {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-021",
                format!(
                    "ν = {:.1} cSt y nq = {nq:.1} fuera del rango de la corrección por viscosidad \
                     (ν ≤ 4000 cSt, nq {low}..{high}; Gülich 2020, tabla 13.1)",
                    correction.nu * 1e6
                ),
            )
            .with_hint("Los factores son una extrapolación; confírmalos con ensayo."),
        );
    }
    if 1.0 - correction.f_h > VISCOUS_HEAD_LOSS || 1.0 - correction.f_eta > VISCOUS_EFFICIENCY_LOSS
    {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-020",
                format!(
                    "con ν = {:.1} cSt la bomba da Q·{:.3}, H·{:.3} y η·{:.3} \
                     (Gülich 2020, tabla 13.1): el diseño para agua no cumple el punto pedido",
                    correction.nu * 1e6,
                    correction.f_q,
                    correction.f_h,
                    correction.f_eta
                ),
            )
            .with_hint(
                "Diseña para el punto de agua equivalente, Qw = Qv/fQ y Hw = Hv/fH \
                 (Gülich ec. 13.11), o revisa la potencia del motor.",
            ),
        );
    }
    diagnostics
}
