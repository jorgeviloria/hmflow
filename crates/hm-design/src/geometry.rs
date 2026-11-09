//! Geometría neutra para el CAD: álabe y salida de la voluta.
//!
//! Son datos (puntos y cotas en SI), no sólidos: el sólido lo genera el
//! kernel de la Parte 2 a partir de ellos. Nada de esto se guarda en el
//! `.hmf`; se recalcula desde el diseño.

use std::collections::BTreeMap;

use hm_formula::Registry;
use hm_units::{Quantity, Unit, parse_unit};

use crate::{DesignError, DesignInput, DesignResult};

/// Incidencia del álabe a la entrada, en grados: centro del intervalo
/// 0..4° de Gülich 2020, p. 411, para rodetes sin exigencia especial de
/// cavitación.
pub const DEFAULT_INCIDENCE_DEG: f64 = 2.0;

/// Rango de L/R1 en que vale la ec. (1.44): el de la fig. 1.20 de Gülich.
pub const DIFFUSER_LENGTH_RANGE: (f64, f64) = (1.0, 50.0);

/// Número de tramos de la línea media del álabe.
pub const BLADE_STEPS: usize = 20;

/// Número de tramos de la línea media cuando β(r) lleva desviación: la curva
/// ya no es una recta y 20 tramos se quedan cortos para integrar θ.
pub const BLADE_STEPS_SHAPED: usize = 60;

/// Diámetros nominales DN de tubería (ISO 6708), en mm. El DN se toma como
/// diámetro interior aproximado de la tubuladura de descarga.
pub const NOMINAL_DIAMETERS_MM: [f64; 19] = [
    25.0, 32.0, 40.0, 50.0, 65.0, 80.0, 100.0, 125.0, 150.0, 200.0, 250.0, 300.0, 350.0, 400.0,
    450.0, 500.0, 600.0, 700.0, 800.0,
];

/// Un punto de la línea media de un álabe cilíndrico (2D).
#[derive(Debug, Clone, PartialEq)]
pub struct BladePoint {
    /// Radio, m.
    pub r: f64,
    /// Ángulo del álabe con la dirección periférica, rad.
    pub beta: f64,
    /// Ángulo de envolvimiento acumulado desde la entrada, rad.
    pub theta: f64,
    /// Anchura meridional del canal, m.
    pub width: f64,
}

/// Geometría del rodete.
#[derive(Debug, Clone)]
pub struct ImpellerGeometry {
    /// Ángulo del álabe a la entrada, rad.
    pub beta1b: f64,
    /// Ángulo de envolvimiento total, rad.
    pub wrap_angle: f64,
    /// Línea media de la entrada (r1) a la salida (r2).
    pub blade: Vec<BladePoint>,
}

/// Salida de la voluta: lengua y difusor hasta la tubuladura.
#[derive(Debug, Clone)]
pub struct VoluteOutlet {
    /// Radio de la lengua, m (el radio base de la voluta, R3).
    pub cutwater_radius: f64,
    /// Radio mínimo de la lengua según Gülich, tabla 10.2, m.
    pub cutwater_min_radius: f64,
    /// Tubuladura y difusor del modelo. `None` con carcasa, que tiene los suyos.
    pub model: Option<ModelOutlet>,
}

/// Geometría completa.
#[derive(Debug, Clone)]
pub struct Geometry {
    pub impeller: ImpellerGeometry,
    pub outlet: VoluteOutlet,
}

fn evaluate(registry: &Registry, id: &str, pairs: &[(&str, Quantity)]) -> Result<f64, DesignError> {
    let inputs: BTreeMap<String, Quantity> = pairs
        .iter()
        .map(|(name, value)| ((*name).to_string(), value.clone()))
        .collect();
    Ok(registry.evaluate(id, &inputs)?.si)
}

/// Ángulo de envolvimiento de una línea media dada por pares (r, β),
/// integrando θ = ∫ dr/(r tan β) por trapecios, como el método punto a punto
/// de Srinivasan 2017 (tabla D1.A3).
pub fn wrap_angle(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let (r0, b0) = pair[0];
            let (r1, b1) = pair[1];
            let g0 = 1.0 / (r0 * b0.tan());
            let g1 = 1.0 / (r1 * b1.tan());
            0.5 * (g0 + g1) * (r1 - r0)
        })
        .sum()
}

/// Longitud de la línea media del álabe, m: suma de los tramos
/// `√(Δr² + (r̄·Δθ)²)` con r̄ el radio medio de cada tramo.
pub fn blade_length(points: &[BladePoint]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let radial = pair[1].r - pair[0].r;
            let mean_radius = 0.5 * (pair[0].r + pair[1].r);
            radial.hypot(mean_radius * (pair[1].theta - pair[0].theta))
        })
        .sum()
}

/// β a la fracción `fraction` (0 a 1) de r, en rad: la recta de `beta1` a
/// `beta2` más una desviación cúbica. La desviación es la Bézier con puntos de
/// control (0, 0), (1/3, d1), (2/3, d2) y (1, 0), con `bow_deg` = (d1, d2) en
/// grados. Como los controles en r están equiespaciados, r es lineal en el
/// parámetro y se evalúa directamente en la fracción. Sin desviación es la
/// recta de siempre.
pub fn beta_profile(beta1: f64, beta2: f64, bow_deg: (f64, f64), fraction: f64) -> f64 {
    let f = fraction;
    let (d1, d2) = (bow_deg.0.to_radians(), bow_deg.1.to_radians());
    let bow = 3.0 * (1.0 - f) * (1.0 - f) * f * d1 + 3.0 * (1.0 - f) * f * f * d2;
    beta1 + (beta2 - beta1) * f + bow
}

/// Línea media de un álabe cilíndrico de r1 a r2 con β lineal en r y la
/// anchura meridional b = Qth/(2π r cm) con cm lineal de cm1 a cm2.
///
/// Con cm1 = cm0 y cm2 la del triángulo de salida, b vale B1 en r1 y B2 en
/// r2, como `design.b1` y `design.b2`.
pub fn camber_line(
    radii: (f64, f64),
    betas: (f64, f64),
    cms: (f64, f64),
    qth: f64,
    steps: usize,
) -> Vec<BladePoint> {
    camber_line_shaped(radii, betas, cms, qth, (0.0, 0.0), steps)
}

/// Como [`camber_line`], con la desviación cúbica de β de [`beta_profile`]
/// (`bow_deg` en grados). Los extremos y las anchuras no cambian.
pub fn camber_line_shaped(
    (r1, r2): (f64, f64),
    (beta1, beta2): (f64, f64),
    (cm1, cm2): (f64, f64),
    qth: f64,
    bow_deg: (f64, f64),
    steps: usize,
) -> Vec<BladePoint> {
    let steps = steps.max(1);
    let mut points: Vec<BladePoint> = Vec::with_capacity(steps + 1);
    for index in 0..=steps {
        let fraction = index as f64 / steps as f64;
        let r = r1 + (r2 - r1) * fraction;
        let beta = beta_profile(beta1, beta2, bow_deg, fraction);
        let cm = cm1 + (cm2 - cm1) * fraction;
        let theta = match points.last() {
            Some(previous) => {
                previous.theta + wrap_angle(&[(previous.r, previous.beta), (r, beta)])
            }
            None => 0.0,
        };
        points.push(BladePoint {
            r,
            beta,
            theta,
            width: qth / (2.0 * std::f64::consts::PI * r * cm),
        });
    }
    points
}

/// Geometría del rodete y de la salida de la voluta de un diseño.
pub fn geometry(input: &DesignInput, result: &DesignResult) -> Result<Geometry, DesignError> {
    let registry = Registry::all();

    let beta1b = result.beta1b;
    let bow = result.selection.blade_bow_deg;
    let blade = camber_line_shaped(
        (result.d1.si / 2.0, result.d2.si / 2.0),
        (beta1b, result.selection.beta2_deg.to_radians()),
        (result.selection.cm0, result.cm2.si),
        result.qth.si,
        bow,
        if bow == (0.0, 0.0) {
            BLADE_STEPS
        } else {
            BLADE_STEPS_SHAPED
        },
    );
    let wrap = blade.last().map_or(0.0, |point| point.theta);

    // Con carcasa no hay voluta del modelo: ni se calcula ni puede parar el diseño.
    let model = result
        .model_volute
        .as_ref()
        .map(|model| model_outlet(&registry, model.throat_diameter.si / 2.0))
        .transpose()?;

    let k3 = result.volute.base_radius.si / (result.d2.si / 2.0);
    let cutwater_min_ratio = cutwater_min_ratio(&registry, input, result.nq)?;

    Ok(Geometry {
        impeller: ImpellerGeometry {
            beta1b,
            wrap_angle: wrap,
            blade,
        },
        outlet: VoluteOutlet {
            cutwater_radius: k3 * result.d2.si / 2.0,
            cutwater_min_radius: cutwater_min_ratio * result.d2.si / 2.0,
            model,
        },
    })
}

/// Tubuladura y difusor de la voluta del modelo.
#[derive(Debug, Clone)]
pub struct ModelOutlet {
    /// Diámetro nominal de la tubuladura, m.
    pub nominal_diameter: f64,
    /// Relación de áreas del difusor, salida/garganta.
    pub area_ratio: f64,
    /// Longitud mínima del difusor para no superar el semiángulo admisible, m.
    pub min_length: f64,
    /// Semiángulo con esa longitud, rad.
    pub half_angle: f64,
}

fn model_outlet(registry: &Registry, throat_radius: f64) -> Result<ModelOutlet, DesignError> {
    let m = parse_unit("m")?;
    let nominal = NOMINAL_DIAMETERS_MM
        .iter()
        .map(|mm| mm / 1000.0)
        .find(|diameter| *diameter >= 2.0 * throat_radius)
        .ok_or_else(|| {
            DesignError::Input(format!(
                "la garganta de {:.0} mm supera la mayor tubuladura DN tabulada",
                2000.0 * throat_radius
            ))
        })?;
    let outlet_radius = nominal / 2.0;
    let (min_length, half_angle) = if outlet_radius <= throat_radius {
        (0.0, 0.0)
    } else {
        let rise = outlet_radius - throat_radius;
        let allowable = |length: f64| -> f64 {
            evaluate(
                registry,
                "design.diffuser_allowable_angle",
                &[
                    ("R1", Quantity::from_si(throat_radius, m.clone())),
                    ("L", Quantity::from_si(length, m.clone())),
                ],
            )
            .unwrap_or(f64::NAN)
        };
        // Semiángulo real menos admisible, dentro del rango de la fig. 1.20
        // (L/R1 = 1..50): decrece con L.
        let excess = |length: f64| (rise / length).atan() - allowable(length);
        let (shortest, longest) = (
            DIFFUSER_LENGTH_RANGE.0 * throat_radius,
            DIFFUSER_LENGTH_RANGE.1 * throat_radius,
        );
        let length = if excess(shortest) <= 0.0 {
            shortest
        } else if excess(longest) > 0.0 {
            return Err(DesignError::Input(format!(
                "el difusor de {:.0} a {:.0} mm necesita L/R1 > {}, fuera de la fig. 1.20 de Gülich",
                2000.0 * throat_radius,
                2000.0 * outlet_radius,
                DIFFUSER_LENGTH_RANGE.1
            )));
        } else {
            hm_math::bisect(excess, shortest, longest, 1e-12, 300)
                .map_err(|error| DesignError::Input(format!("longitud del difusor: {error}")))?
        };
        let angle = (rise / length).atan();
        if !angle.is_finite() || angle > allowable(length) + 1e-6 {
            return Err(DesignError::Input(
                "la longitud del difusor no converge".to_string(),
            ));
        }
        (length, angle)
    };

    Ok(ModelOutlet {
        nominal_diameter: nominal,
        area_ratio: (outlet_radius / throat_radius).powi(2),
        min_length,
        half_angle,
    })
}

/// dz/d2 mínimo de la lengua según Gülich 2020, tabla 10.2.
pub(crate) fn cutwater_min_ratio(
    registry: &Registry,
    input: &DesignInput,
    nq: f64,
) -> Result<f64, DesignError> {
    evaluate(
        registry,
        "design.cutwater_ratio",
        &[
            ("nq", Quantity::from_si(nq, Unit::dimensionless())),
            ("rho", input.rho.clone()),
            ("H", input.h.clone()),
            ("rho_ref", Quantity::from_si(1000.0, parse_unit("kg/m3")?)),
            ("H_ref", Quantity::from_si(1000.0, parse_unit("m")?)),
        ],
    )
}
