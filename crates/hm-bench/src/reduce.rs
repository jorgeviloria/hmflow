//! De lecturas a H, P, η y NPSH₃ a la velocidad especificada.

use std::collections::BTreeMap;

use hm_design::fluid::{water_density, water_vapor_pressure};
use hm_formula::Registry;
use hm_math::mean;
use hm_units::{Quantity, constants, parse_unit};

use crate::case::{BenchCase, BenchError, NpshReading, Reading};

/// Caída de altura que define el NPSH₃.
const NPSH3_DROP: f64 = 0.03;

/// Pasos del inicio de una serie que fijan la altura de referencia.
const REFERENCE_STEPS: usize = 3;

/// Incertidumbres relativas de un punto.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointUncertainty {
    /// En Qd = 0 no hay caudal al que referirla.
    pub q: Option<f64>,
    pub h: f64,
    pub p: f64,
    pub eta: Option<f64>,
}

/// Un punto de la curva a la velocidad especificada, en SI.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub point: u32,
    pub q: f64,
    pub h: f64,
    pub p: Option<f64>,
    pub eta: Option<f64>,
    /// Velocidad media leída; `None` si no se midió.
    pub n: Option<f64>,
    pub readings: usize,
    /// Semiamplitud de H entre lecturas, en m.
    pub h_spread: f64,
    pub uncertainty: Option<PointUncertainty>,
}

/// Una serie de NPSH reducida.
#[derive(Debug, Clone, PartialEq)]
pub struct NpshSeries {
    pub q_target: f64,
    pub q: f64,
    pub h_reference: f64,
    /// `None`: la serie no llegó al 3 %.
    pub npsh3: Option<f64>,
    pub steps: usize,
}

/// Resultado de reducir un caso.
#[derive(Debug, Clone, PartialEq)]
pub struct Reduction {
    /// Velocidad a la que están referidos los puntos, en 1/s.
    pub speed: f64,
    pub points: Vec<Point>,
    pub npsh: Vec<NpshSeries>,
    /// Lo que no se pudo corregir o no se cumple.
    pub notes: Vec<String>,
}

#[derive(Clone, Copy)]
struct Single {
    q: f64,
    h: f64,
    p: Option<f64>,
    eta: Option<f64>,
    n: Option<f64>,
    /// Altura de velocidad entre tomas, en m, sin corregir de velocidad.
    velocity_head: f64,
    rho: f64,
}

struct Reducer<'a> {
    case: &'a BenchCase,
    registry: Registry,
}

impl Reducer<'_> {
    fn eval(&self, id: &str, inputs: &[(&str, f64, &str)]) -> Result<f64, BenchError> {
        let formula_error = |error: String| BenchError::Formula(format!("{id}: {error}"));
        let mut map = BTreeMap::new();
        for (name, value, unit) in inputs {
            let unit = parse_unit(unit).map_err(|e| formula_error(e.to_string()))?;
            map.insert((*name).to_string(), Quantity::from_si(*value, unit));
        }
        self.registry
            .evaluate(id, &map)
            .map(|quantity| quantity.si)
            .map_err(|e| formula_error(e.to_string()))
    }

    fn density(&self, t_water: f64) -> Result<f64, BenchError> {
        water_density(t_water).ok_or_else(|| water_error(t_water))
    }

    /// Velocidades en las tomas; cero si falta algún diámetro.
    fn velocities(&self, q: f64) -> Result<(f64, f64), BenchError> {
        let rig = &self.case.rig;
        let (Some(suction), Some(discharge)) = (rig.d_suction, rig.d_discharge) else {
            return Ok((0.0, 0.0));
        };
        let velocity =
            |d: f64| self.eval("bench.pipe_velocity", &[("Q", q, "m3/s"), ("D", d, "m")]);
        Ok((velocity(suction)?, velocity(discharge)?))
    }

    /// Presión de succión en la misma referencia que la de descarga.
    fn suction_gauge(&self, p_suction: f64) -> f64 {
        match (self.case.rig.suction_absolute, self.case.rig.p_atm) {
            (true, Some(p_atm)) => p_suction - p_atm,
            _ => p_suction,
        }
    }

    fn head(&self, q: f64, p_d: f64, p_s: f64, rho: f64) -> Result<(f64, f64), BenchError> {
        let (c_s, c_d) = self.velocities(q)?;
        let rig = &self.case.rig;
        let head = self.eval(
            "bench.total_head",
            &[
                ("p_d", p_d, "Pa"),
                ("p_s", self.suction_gauge(p_s), "Pa"),
                ("rho", rho, "kg/m3"),
                ("z_d", rig.z_discharge.unwrap_or(0.0), "m"),
                ("z_s", rig.z_suction.unwrap_or(0.0), "m"),
                ("c_d", c_d, "m/s"),
                ("c_s", c_s, "m/s"),
            ],
        )?;
        let g = constants::g().si;
        Ok((head, (c_d * c_d - c_s * c_s) / (2.0 * g)))
    }

    fn similar(
        &self,
        id: &str,
        name: &str,
        value: f64,
        unit: &str,
        n: f64,
    ) -> Result<f64, BenchError> {
        self.eval(
            id,
            &[
                (name, value, unit),
                ("n", n, "1/s"),
                ("n_sp", self.case.specified.n, "1/s"),
            ],
        )
    }

    fn reading(&self, reading: &Reading) -> Result<Single, BenchError> {
        let rho = self.density(reading.t_water)?;
        let (h, velocity_head) =
            self.head(reading.q, reading.p_discharge, reading.p_suction, rho)?;
        let power = match (reading.n, reading.torque) {
            (Some(n), Some(torque)) => Some(self.eval(
                "bench.shaft_power",
                &[("n", n, "1/s"), ("T", torque, "N*m")],
            )?),
            _ => None,
        };
        let eta = power
            .map(|power| self.efficiency(rho, reading.q, h, power))
            .transpose()?;

        let (q, h_star, p_star) = match reading.n {
            Some(n) => (
                self.similar("bench.similar_flow", "Q", reading.q, "m3/s", n)?,
                self.similar("bench.similar_head", "H", h, "m", n)?,
                power
                    .map(|power| self.similar("bench.similar_power", "P", power, "W", n))
                    .transpose()?,
            ),
            None => (reading.q, h, None),
        };
        Ok(Single {
            q,
            h: h_star,
            p: p_star,
            eta,
            n: reading.n,
            velocity_head,
            rho,
        })
    }

    fn efficiency(&self, rho: f64, q: f64, h: f64, power: f64) -> Result<f64, BenchError> {
        let hydraulic = self.eval(
            "perf.hydraulic_power",
            &[("rho", rho, "kg/m3"), ("Q", q, "m3/s"), ("H", h, "m")],
        )?;
        self.eval(
            "perf.efficiency",
            &[("P_hyd", hydraulic, "W"), ("P_shaft", power, "W")],
        )
    }

    fn point(&self, point: u32, readings: &[&Reading]) -> Result<Point, BenchError> {
        let singles = readings
            .iter()
            .map(|reading| self.reading(reading))
            .collect::<Result<Vec<_>, _>>()?;
        let of = |get: fn(&Single) -> f64| {
            mean(&singles.iter().map(get).collect::<Vec<_>>()).unwrap_or(0.0)
        };
        let of_all = |get: fn(&Single) -> Option<f64>| {
            let values: Option<Vec<f64>> = singles.iter().map(get).collect();
            values.and_then(|values| mean(&values))
        };
        let highest = singles.iter().map(|s| s.h).fold(f64::MIN, f64::max);
        let lowest = singles.iter().map(|s| s.h).fold(f64::MAX, f64::min);

        let mut result = Point {
            point,
            q: of(|s| s.q),
            h: of(|s| s.h),
            p: of_all(|s| s.p),
            eta: of_all(|s| s.eta),
            n: of_all(|s| s.n),
            readings: singles.len(),
            h_spread: (highest - lowest) / 2.0,
            uncertainty: None,
        };
        result.uncertainty =
            self.uncertainty(readings, &result, of(|s| s.velocity_head), of(|s| s.rho));
        Ok(result)
    }

    /// Propagación de `docs/validation/caso-de-banco.md`. `None` si falta la
    /// incertidumbre de algún instrumento, o la velocidad o el par.
    fn uncertainty(
        &self,
        readings: &[&Reading],
        point: &Point,
        velocity_head: f64,
        rho: f64,
    ) -> Option<PointUncertainty> {
        let u = self.case.uncertainties;
        let average = |get: fn(&Reading) -> Option<f64>| {
            let values: Option<Vec<f64>> = readings.iter().map(|r| get(r)).collect();
            mean(&values?)
        };
        let n = average(|r| r.n)?;
        let torque = average(|r| r.torque)?;
        let q = average(|r| Some(r.q))?;
        let p_d = average(|r| Some(r.p_discharge))?;
        let p_s = average(|r| Some(r.p_suction))?;

        let u_q = u.flow?.relative(q);
        let u_n = u.speed?.relative(n)?;
        let u_t = u.torque?.relative(torque)?;
        let u_pd = u.p_discharge?.absolute(p_d);
        let u_ps = u.p_suction?.absolute(p_s);

        let g = constants::g().si;
        let pressure = u_pd.hypot(u_ps) / (rho * g);
        let velocity = 2.0 * u_q.unwrap_or(0.0) * velocity_head;
        let u_h_raw = pressure.hypot(velocity) / point.h.abs();

        Some(PointUncertainty {
            q: u_q.map(|u_q| u_q.hypot(u_n)),
            h: u_h_raw.hypot(2.0 * u_n),
            p: u_t.hypot(2.0 * u_n),
            eta: (q > 0.0).then(|| {
                let (u_q, u_h) = (u_q.unwrap_or(0.0), u_h_raw);
                (u_q * u_q + u_h * u_h + u_t * u_t + u_n * u_n).sqrt()
            }),
        })
    }

    fn npsh_step(&self, reading: &NpshReading) -> Result<(f64, f64, f64), BenchError> {
        let rho = self.density(reading.t_water)?;
        let p_v =
            water_vapor_pressure(reading.t_water).ok_or_else(|| water_error(reading.t_water))?;
        let (h, _) = self.head(reading.q, reading.p_discharge, reading.p_suction, rho)?;
        let (c_s, _) = self.velocities(reading.q)?;
        let rig = &self.case.rig;
        let p_abs = reading.p_suction
            + if rig.suction_absolute {
                0.0
            } else {
                rig.p_atm.unwrap_or(0.0)
            };
        let npsha = self.eval(
            "bench.npsha",
            &[
                ("p_abs", p_abs, "Pa"),
                ("p_v", p_v, "Pa"),
                ("rho", rho, "kg/m3"),
                ("z_s", rig.z_suction.unwrap_or(0.0), "m"),
                ("c_s", c_s, "m/s"),
            ],
        )?;
        match reading.n {
            Some(n) => Ok((
                self.similar("bench.similar_flow", "Q", reading.q, "m3/s", n)?,
                self.similar("bench.similar_head", "H", h, "m", n)?,
                self.similar("bench.similar_head", "H", npsha, "m", n)?,
            )),
            None => Ok((reading.q, h, npsha)),
        }
    }

    fn series(&self, q_target: f64, steps: &[&NpshReading]) -> Result<NpshSeries, BenchError> {
        let mut ordered = steps.to_vec();
        ordered.sort_by_key(|reading| reading.step);
        let reduced = ordered
            .iter()
            .map(|reading| self.npsh_step(reading))
            .collect::<Result<Vec<_>, _>>()?;

        let flows: Vec<f64> = reduced.iter().map(|step| step.0).collect();
        let heads: Vec<f64> = reduced.iter().map(|step| step.1).collect();
        let reference = mean(&heads[..REFERENCE_STEPS.min(heads.len())]).unwrap_or(0.0);
        Ok(NpshSeries {
            q_target,
            q: mean(&flows).unwrap_or(0.0),
            h_reference: reference,
            npsh3: npsh3(&reduced, reference),
            steps: reduced.len(),
        })
    }
}

fn water_error(t_water: f64) -> BenchError {
    BenchError::Formula(format!(
        "sin propiedades del agua a {:.1} °C",
        t_water - 273.15
    ))
}

/// NPSH donde la altura cruza el 97 % de la referencia. Sin cruce no hay valor:
/// nunca se extrapola.
fn npsh3(steps: &[(f64, f64, f64)], reference: f64) -> Option<f64> {
    let target = reference * (1.0 - NPSH3_DROP);
    let start = REFERENCE_STEPS.saturating_sub(1);
    steps.get(start..)?.windows(2).find_map(|pair| {
        let (_, h_before, npsh_before) = pair[0];
        let (_, h_after, npsh_after) = pair[1];
        (h_before >= target && h_after < target).then(|| {
            npsh_before + (npsh_after - npsh_before) * (h_before - target) / (h_before - h_after)
        })
    })
}

/// Reduce un caso. Lo que falta no se inventa: se omite y queda en `notes`.
pub fn reduce(case: &BenchCase) -> Result<Reduction, BenchError> {
    let registry = Registry::all();
    let reducer = Reducer { case, registry };

    let mut by_point: BTreeMap<u32, Vec<&Reading>> = BTreeMap::new();
    for reading in &case.curve {
        by_point.entry(reading.point).or_default().push(reading);
    }
    let points = by_point
        .iter()
        .map(|(point, readings)| reducer.point(*point, readings))
        .collect::<Result<Vec<_>, _>>()?;

    let mut targets: Vec<f64> = Vec::new();
    for reading in &case.npsh {
        if !targets.contains(&reading.q_target) {
            targets.push(reading.q_target);
        }
    }
    let npsh = targets
        .iter()
        .map(|target| {
            let steps: Vec<&NpshReading> = case
                .npsh
                .iter()
                .filter(|reading| reading.q_target == *target)
                .collect();
            reducer.series(*target, &steps)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let notes = notes(case, &points, &npsh);
    Ok(Reduction {
        speed: case.specified.n,
        points,
        npsh,
        notes,
    })
}

fn notes(case: &BenchCase, points: &[Point], npsh: &[NpshSeries]) -> Vec<String> {
    let mut notes = Vec::new();
    let rig = &case.rig;
    if rig.d_suction.is_none() || rig.d_discharge.is_none() {
        notes.push("sin diámetros en las tomas: H no incluye la altura de velocidad".to_string());
    }
    if rig.z_suction.is_none() || rig.z_discharge.is_none() {
        notes.push("sin alturas de las tomas: H no incluye la diferencia de cotas".to_string());
    }
    if case.curve.iter().any(|reading| reading.n.is_none()) {
        notes.push("sin velocidad medida: H y Q no se corrigen a la especificada".to_string());
    }
    if case.curve.iter().any(|reading| reading.torque.is_none()) {
        notes.push("sin par medido: no hay P ni η".to_string());
    }
    if points.iter().all(|point| point.uncertainty.is_none()) {
        notes.push("sin incertidumbre declarada".to_string());
    }
    for point in points {
        let declared = point.uncertainty.map(|u| u.h * point.h.abs());
        if declared.is_some_and(|declared| point.h_spread > declared) {
            notes.push(format!(
                "punto {}: la dispersión de H (±{:.2} m) supera la incertidumbre declarada",
                point.point, point.h_spread
            ));
        }
    }
    for series in npsh {
        if series.npsh3.is_none() {
            notes.push(format!(
                "serie {:.1} L/s: no llegó al 3 %, no hay NPSH3",
                series.q_target * 1000.0
            ));
        }
    }
    notes
}
