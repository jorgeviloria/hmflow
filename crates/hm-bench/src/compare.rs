//! Lo medido frente a la curva del modelo.

use hm_design::CurvePoint;

use crate::case::BenchError;
use crate::reduce::Reduction;

/// Tolerancia usada para altura y rendimiento. Es la misma de
/// `crates/hm-design/tests/case_b1.rs`, aplicada como banda simétrica y
/// **por verificar** contra ISO 9906.
const GRADE_2B: f64 = 0.05;

/// Diferencia de velocidad admitida entre el caso y el modelo.
const SPEED_TOLERANCE: f64 = 0.001;

/// Un punto medido frente al modelo. Las desviaciones son (modelo − medido)/medido.
#[derive(Debug, Clone, PartialEq)]
pub struct PointComparison {
    pub point: u32,
    pub q: f64,
    pub h_measured: f64,
    pub h_model: f64,
    pub h_deviation: f64,
    pub eta_deviation: Option<f64>,
    pub p_deviation: Option<f64>,
    pub in_band: bool,
}

/// Una serie de NPSH frente al modelo.
#[derive(Debug, Clone, PartialEq)]
pub struct NpshComparison {
    pub q: f64,
    pub npsh3: f64,
    pub npshr_model: f64,
    pub deviation: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub points: Vec<PointComparison>,
    pub npsh: Vec<NpshComparison>,
    /// Puntos fuera del rango de la curva del modelo: no se extrapola.
    pub outside: Vec<u32>,
}

struct ModelAt {
    h: f64,
    eta: f64,
    p: f64,
    npshr: f64,
}

/// Interpolación lineal; `None` fuera de la curva.
fn model_at(curve: &[CurvePoint], q: f64) -> Option<ModelAt> {
    let pair = curve
        .windows(2)
        .find(|pair| pair[0].q.si <= q && q <= pair[1].q.si)?;
    let (a, b) = (&pair[0], &pair[1]);
    let t = (q - a.q.si) / (b.q.si - a.q.si);
    let at = |x: f64, y: f64| hm_math::lerp(x, y, t);
    Some(ModelAt {
        h: at(a.h.si, b.h.si),
        eta: at(a.eta, b.eta),
        p: at(a.p_shaft.si, b.p_shaft.si),
        npshr: at(a.npshr.si, b.npshr.si),
    })
}

fn deviation(model: f64, measured: f64) -> f64 {
    (model - measured) / measured
}

/// Compara una reducción con la curva del modelo a `model_speed` (1/s).
pub fn compare(
    reduction: &Reduction,
    curve: &[CurvePoint],
    model_speed: f64,
) -> Result<Comparison, BenchError> {
    if (model_speed / reduction.speed - 1.0).abs() > SPEED_TOLERANCE {
        return Err(BenchError::Speed {
            model: model_speed * 60.0,
            case: reduction.speed * 60.0,
        });
    }

    let mut points = Vec::new();
    let mut outside = Vec::new();
    for point in &reduction.points {
        let Some(model) = model_at(curve, point.q) else {
            outside.push(point.point);
            continue;
        };
        let h_deviation = deviation(model.h, point.h);
        let eta_deviation = point.eta.map(|eta| deviation(model.eta, eta));
        let in_band =
            h_deviation.abs() <= GRADE_2B && eta_deviation.is_none_or(|eta| eta.abs() <= GRADE_2B);
        points.push(PointComparison {
            point: point.point,
            q: point.q,
            h_measured: point.h,
            h_model: model.h,
            h_deviation,
            eta_deviation,
            p_deviation: point.p.map(|p| deviation(model.p, p)),
            in_band,
        });
    }

    let npsh = reduction
        .npsh
        .iter()
        .filter_map(|series| {
            let npsh3 = series.npsh3?;
            let model = model_at(curve, series.q)?;
            Some(NpshComparison {
                q: series.q,
                npsh3,
                npshr_model: model.npshr,
                deviation: deviation(model.npshr, npsh3),
            })
        })
        .collect();

    Ok(Comparison {
        points,
        npsh,
        outside,
    })
}
