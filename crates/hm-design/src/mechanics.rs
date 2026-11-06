//! Mecánica preliminar: eje por torsión, cubo, espesores, holgura de los
//! anillos, empujes hidráulicos y motor.
//!
//! Son estimaciones para dimensionar, no comprobaciones del rotor: el eje
//! sale solo de la torsión (Srinivasan 2017, §4.14), sin flexión por el
//! empuje radial, sin velocidades críticas, chaveta, cojinetes ni cierre.
//! Nada de esto se guarda en el `.hmf`; se recalcula desde el diseño.

use std::collections::BTreeMap;

use hm_core::{Diagnostic, Diagnostics};
use hm_formula::Registry;
use hm_units::{Quantity, Unit, parse_unit};

use crate::{
    DesignError, DesignInput, DesignResult, PREFERRED_OPERATING_REGION, curve_head,
    performance_curve,
};

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

/// Caudal relativo hasta el que vale el kR constante de una voluta doble
/// (Gülich 2020, tabla 9.7 (1) y nota 5 de la p. 705).
pub const DOUBLE_VOLUTE_MAX_FLOW: f64 = 1.1;

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

/// Espesor mínimo del álabe de un rodete fundido, relativo a d2 (Gülich
/// 2020, tabla 14.3).
pub const CAST_MIN_THICKNESS_RATIO: f64 = 0.015;

/// Espesor mínimo que exige la fundición, m: extremo bajo de 3..5 mm
/// (Gülich 2020, p. 413). Depende del proceso; elección del motor.
pub const CAST_MIN_THICKNESS: f64 = 0.003;

/// Diámetro de referencia de la ec. (3.12) de Gülich 2020, m.
pub const SEAL_CLEARANCE_REFERENCE: f64 = 0.1;

/// Margen del motor de API 610 (ISO 13709): (potencia nominal hasta la que
/// se aplica, W; factor). Por encima del último tramo, el último factor.
pub const DRIVER_MARGINS: [(f64, f64); 3] = [(22e3, 1.25), (55e3, 1.15), (f64::INFINITY, 1.10)];

/// Potencias normalizadas de motores de IEC 60072-1, kW.
pub const IEC_MOTOR_RATINGS_KW: [f64; 41] = [
    0.06, 0.09, 0.12, 0.18, 0.25, 0.37, 0.55, 0.75, 1.1, 1.5, 2.2, 3.0, 4.0, 5.5, 7.5, 11.0, 15.0,
    18.5, 22.0, 30.0, 37.0, 45.0, 55.0, 75.0, 90.0, 110.0, 132.0, 160.0, 200.0, 250.0, 315.0,
    355.0, 400.0, 450.0, 500.0, 560.0, 630.0, 710.0, 800.0, 900.0, 1000.0,
];

/// Puntos con que se busca la potencia máxima de la curva.
const DRIVER_CURVE_POINTS: usize = 101;

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
    /// Cubo mínimo que pide el par, m. Sin carcasa es `hub_diameter`.
    pub hub_min_diameter: f64,
    /// Ángulo de envolvente de la voluta doble en que se basa el empuje radial,
    /// grados; `None` es una voluta simple.
    pub volute_wrap_deg: Option<f64>,
    /// kR0, a válvula cerrada.
    pub radial_coefficient_shutoff: f64,
    /// kR,opt, en el punto de diseño.
    pub radial_coefficient_design: f64,
    /// Espesor relativo del álabe e* = e/d2 de la ec. (T14.3.1).
    pub blade_thickness_ratio: f64,
    /// Espesor nominal del álabe, m: el mayor de e*·d2, 0,015·d2 y el
    /// mínimo de fundición.
    pub blade_thickness: f64,
    /// Espesor del borde de salida, m (e/2, ec. T14.3.2).
    pub trailing_edge_thickness: f64,
    /// Espesor de las tapas, m (1,25·e).
    pub shroud_thickness: f64,
    /// Anchura con tapas usada en el empuje radial, b2 + 2·eRS, m.
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
    /// Holgura diametral del anillo de la tapa anterior, m (ec. 3.12).
    pub seal_clearance: f64,
    /// Holgura típica de Gülich, ec. (3.12), m. Sin datos de fuga, la de arriba.
    pub usual_seal_clearance: f64,
    /// Potencia en el eje en el punto nominal, W.
    pub rated_power: f64,
    /// Margen del motor de API 610 para esa potencia.
    pub driver_margin: f64,
    /// Potencia mínima del motor, W.
    pub driver_power: f64,
    /// Motor normalizado de IEC 60072-1, W; `None` si supera la serie.
    pub motor_rating: Option<f64>,
    /// Potencia máxima en el eje a lo largo de la curva preliminar, W.
    pub curve_max_power: f64,
}

/// Margen del motor de API 610 para una potencia nominal en W.
pub fn driver_margin(rated_power: f64) -> f64 {
    DRIVER_MARGINS
        .iter()
        .find(|(limit, _)| rated_power <= *limit)
        .map_or(DRIVER_MARGINS[DRIVER_MARGINS.len() - 1].1, |(_, factor)| {
            *factor
        })
}

/// Motor normalizado de IEC 60072-1 inmediatamente mayor o igual, en W.
pub fn motor_rating(power: f64) -> Option<f64> {
    IEC_MOTOR_RATINGS_KW
        .iter()
        .map(|kw| kw * 1000.0)
        .find(|watts| *watts >= power * (1.0 - 1e-12))
}

fn evaluate(registry: &Registry, id: &str, pairs: &[(&str, Quantity)]) -> Result<f64, DesignError> {
    let inputs: BTreeMap<String, Quantity> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Ok(registry.evaluate(id, &inputs)?.si)
}

/// Espesor nominal del álabe, el relativo e* y el del borde de salida, m.
///
/// e = máx(e*·d2, 0,015·d2, 0,003 m) y el borde de salida es e/2
/// (Gülich 2020, tabla 14.3). Lo comparten el triángulo de salida y esta
/// mecánica, para que el bloqueo τ y el espesor informado no se separen.
pub fn blade_thicknesses(
    blade_count: u32,
    u2: f64,
    nq: f64,
    d2: f64,
) -> Result<(f64, f64, f64), DesignError> {
    if blade_count == 0 || !u2.is_finite() || !nq.is_finite() || !d2.is_finite() || d2 <= 0.0 {
        return Err(DesignError::Input(
            "el espesor del álabe necesita Z, u2, nq y D2 válidos".to_string(),
        ));
    }
    let registry = Registry::all();
    let one = Unit::dimensionless();
    let ratio = evaluate(
        &registry,
        "mech.blade_thickness_ratio",
        &[
            ("z", Quantity::from_si(f64::from(blade_count), one.clone())),
            ("u2", Quantity::from_si(u2, parse_unit("m/s")?)),
            ("u_ref", Quantity::from_si(100.0, parse_unit("m/s")?)),
            ("nq", Quantity::from_si(nq, one)),
        ],
    )?;
    let thickness = (ratio * d2)
        .max(CAST_MIN_THICKNESS_RATIO * d2)
        .max(CAST_MIN_THICKNESS);
    Ok((ratio, thickness, thickness / 2.0))
}

/// Diámetro mínimo del eje por torsión y el del cubo, m.
///
/// El ojo automático usa el par de `P·margen`, antes de subir el par al
/// máximo de la curva. [`mechanics`] vuelve a calcular el cubo con
/// `torque_max`, que puede ser mayor.
pub fn shaft_and_hub(torque: f64, shear: f64) -> Result<(f64, f64), DesignError> {
    if !torque.is_finite() || torque <= 0.0 || !shear.is_finite() || shear <= 0.0 {
        return Err(DesignError::Input(
            "el eje necesita un par y una tensión admisible positivos".to_string(),
        ));
    }
    let registry = Registry::all();
    let ds = evaluate(
        &registry,
        "mech.shaft_min_diameter",
        &[
            ("T", Quantity::from_si(torque, parse_unit("N*m")?)),
            ("tau", Quantity::from_si(shear, parse_unit("Pa")?)),
        ],
    )?;
    let hub = evaluate(
        &registry,
        "mech.hub_diameter",
        &[
            ("ds", Quantity::from_si(ds, parse_unit("m")?)),
            ("k_hub", Quantity::from_si(HUB_RATIO, Unit::dimensionless())),
        ],
    )?;
    Ok((ds, hub))
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
    let (shaft_min_diameter, hub_min_diameter) = shaft_and_hub(shaft_torque, shear_stress)?;
    let hub_diameter = result
        .casing
        .as_ref()
        .map_or(hub_min_diameter, |casing| casing.hub_diameter);

    let nq = result.nq.clamp(RADIAL_NQ_RANGE.0, RADIAL_NQ_RANGE.1);
    let k0 = evaluate(
        &registry,
        "mech.radial_shutoff_coefficient",
        &[("nq", pure(nq))],
    )?;
    let k_opt = RADIAL_DESIGN_COEFFICIENT;
    // Voluta doble: kR = FDsp·kR0, constante hasta 1,1·Qd. Por encima el
    // libro no da dato y no se baja del empuje de voluta simple.
    let double_k0 = match result.volute.wrap_deg {
        Some(wrap) => {
            let factor = evaluate(
                &registry,
                "mech.double_volute_factor",
                &[("eps_deg", pure(wrap))],
            )?;
            Some(factor * k0)
        }
        None => None,
    };

    let d2 = result.d2.si;
    let (blade_thickness_ratio, blade_thickness, trailing_edge_thickness) =
        blade_thicknesses(result.selection.blade_count, result.u2.si, result.nq, d2)?;
    let shroud_thickness = evaluate(
        &registry,
        "mech.shroud_thickness",
        &[("e", metres(blade_thickness))],
    )?;
    let radial_width = result.b2.si + 2.0 * shroud_thickness;
    let radial = |ratio: f64| -> Result<f64, DesignError> {
        let single = evaluate(
            &registry,
            "mech.radial_thrust_coefficient",
            &[
                ("q", pure(ratio)),
                ("kR0", pure(k0)),
                ("kR_opt", pure(k_opt)),
            ],
        )?;
        let k = match double_k0 {
            Some(double) if ratio <= DOUBLE_VOLUTE_MAX_FLOW => double,
            Some(double) => double.max(single),
            None => single,
        };
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

    let front_seal_diameter = result
        .casing
        .as_ref()
        .map_or(result.d1.si, |casing| casing.neck_diameter);
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

    let usual_seal_clearance = evaluate(
        &registry,
        "mech.seal_clearance",
        &[
            ("dsp", metres(front_seal_diameter)),
            ("d_ref", metres(SEAL_CLEARANCE_REFERENCE)),
        ],
    )?;
    let declared_clearance = result
        .casing
        .as_ref()
        .and_then(|casing| casing.leakage.as_ref())
        .map(|leakage| leakage.seal_clearance);

    let rated_power = result.p_shaft.si;
    let margin = driver_margin(rated_power);
    let driver_power = evaluate(
        &registry,
        "mech.driver_power",
        &[("P", result.p_shaft.clone()), ("margin", pure(margin))],
    )?;
    let curve_max_power = performance_curve(input, result, DRIVER_CURVE_POINTS)
        .iter()
        .map(|point| point.p_shaft.si)
        .fold(rated_power, f64::max);

    Ok(Mechanics {
        shaft_torque,
        shaft_shear_stress: shear_stress,
        shaft_min_diameter,
        hub_diameter,
        hub_min_diameter,
        volute_wrap_deg: result.volute.wrap_deg,
        radial_coefficient_shutoff: double_k0.unwrap_or(k0),
        radial_coefficient_design: double_k0.unwrap_or(k_opt),
        blade_thickness_ratio,
        blade_thickness,
        trailing_edge_thickness,
        shroud_thickness,
        radial_width,
        radial_thrust_shutoff: radial(0.0)?,
        radial_thrust_design: radial(1.0)?,
        radial_thrust_overload: radial(PREFERRED_OPERATING_REGION.1)?,
        front_seal_diameter,
        shaft_seal_diameter,
        axial_thrust: (axial(AXIAL_THRUST_FACTOR.0)?, axial(AXIAL_THRUST_FACTOR.1)?),
        seal_clearance: declared_clearance.unwrap_or(usual_seal_clearance),
        usual_seal_clearance,
        rated_power,
        driver_margin: margin,
        driver_power,
        motor_rating: motor_rating(driver_power),
        curve_max_power,
    })
}

/// Avisos de la mecánica preliminar.
/// b3/b2 que cita Gülich 2020, sec. 7.8, pp. 492–493: de 1,05..1,2 con nq alto
/// a 2,0..4,0 con nq bajo. No es un límite: el libro dice que se elige «in wide
/// limits without major effect on the efficiency». Fuera, solo se informa.
pub const VOLUTE_WIDTH_RATIO_RANGE: (f64, f64) = (1.05, 4.0);

/// Dispersión de las holguras ejecutadas frente a la ec. (3.12), Gülich p. 110.
const SEAL_CLEARANCE_SPREAD: f64 = 0.30;

/// Lo que no impide diseñar, pero hay que mirar antes de fabricar.
fn casing_diagnostics(mechanics: &Mechanics, result: &DesignResult) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let Some(casing) = &result.casing else {
        return diagnostics;
    };

    let eye_with_walls = result.d1.si + 2.0 * mechanics.shroud_thickness;
    if eye_with_walls > casing.neck_diameter {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-041",
                format!(
                    "el ojo de {:.1} mm más dos tapas de {:.1} mm suman {:.1} mm y el cuello \
                     mide {:.1} mm",
                    result.d1.si * 1000.0,
                    mechanics.shroud_thickness * 1000.0,
                    eye_with_walls * 1000.0,
                    casing.neck_diameter * 1000.0
                ),
            )
            .with_hint("Baja `eye_diameter_max` o revisa el espesor de la tapa en el cuello."),
        );
    }

    if mechanics.shaft_min_diameter > casing.bore_diameter {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-042",
                format!(
                    "el eje mínimo sale {:.1} mm y el agujero de la carcasa es de {:.3} mm: \
                     revisa el criterio de tensión admisible con el material real del eje",
                    mechanics.shaft_min_diameter * 1000.0,
                    casing.bore_diameter * 1000.0
                ),
            )
            .with_hint(
                "El mínimo usa una tensión admisible conservadora, no el material real del eje.",
            ),
        );
    }

    if casing.hub_diameter < mechanics.hub_min_diameter {
        diagnostics.push(
            Diagnostic::warning(
                "HM-DES-044",
                format!(
                    "el cubo de {:.1} mm queda por debajo de los {:.1} mm que da el criterio de \
                     tensión admisible",
                    casing.hub_diameter * 1000.0,
                    mechanics.hub_min_diameter * 1000.0
                ),
            )
            .with_hint(
                "Sale de la misma tensión conservadora que el eje mínimo, no del material real.",
            ),
        );
    }

    let clearance = mechanics.seal_clearance / mechanics.usual_seal_clearance - 1.0;
    if clearance.abs() > SEAL_CLEARANCE_SPREAD {
        diagnostics.push(Diagnostic::info(
            "HM-DES-045",
            format!(
                "holgura de {:.2} mm, a {:+.0} % de los {:.2} mm de Gülich, ec. (3.12): las \
                 ejecutadas quedan en ±30 %",
                mechanics.seal_clearance * 1000.0,
                100.0 * clearance,
                mechanics.usual_seal_clearance * 1000.0
            ),
        ));
    }

    let ratio = casing.b3 / result.b2.si;
    let (low, high) = VOLUTE_WIDTH_RATIO_RANGE;
    if !(low..=high).contains(&ratio) {
        diagnostics.push(Diagnostic::info(
            "HM-DES-043",
            format!(
                "b3/b2 = {ratio:.2}, fuera de los valores que cita Gülich ({low}..{high}, \
                 pp. 492–493)"
            ),
        ));
    }
    diagnostics
}

pub fn mechanics_diagnostics(mechanics: &Mechanics, result: &DesignResult) -> Diagnostics {
    let mut diagnostics = casing_diagnostics(mechanics, result);
    let d0 = result.d0.si;
    let blockage = (mechanics.hub_diameter / d0).powi(2);
    // El ojo automático ya restó el cubo del área. El aviso es para el ojo
    // de Srinivasan, que se calcula sin él.
    if !result.eye_excludes_hub && blockage > HUB_EYE_BLOCKAGE {
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
    match mechanics.motor_rating {
        None => diagnostics.push(
            Diagnostic::warning(
                "HM-DES-030",
                format!(
                    "el motor necesita {:.0} kW, por encima de la serie de IEC 60072-1 \
                     (hasta {:.0} kW): no se eligió motor",
                    mechanics.driver_power / 1000.0,
                    IEC_MOTOR_RATINGS_KW[IEC_MOTOR_RATINGS_KW.len() - 1]
                ),
            )
            .with_hint("Elige el motor con el fabricante."),
        ),
        Some(rating) if mechanics.curve_max_power > rating => diagnostics.push(
            Diagnostic::warning(
                "HM-DES-029",
                format!(
                    "la potencia en el eje llega a {:.1} kW en la curva y el motor de \
                     {:.1} kW no la cubre: la bomba sobrecarga el motor por encima \
                     del punto nominal",
                    mechanics.curve_max_power / 1000.0,
                    rating / 1000.0
                ),
            )
            .with_hint(
                "Elige el motor siguiente de la serie o limita el caudal máximo \
                 de la instalación.",
            ),
        ),
        Some(_) => {}
    }
    diagnostics
}
