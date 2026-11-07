//! Curvas planas para el modelo geométrico.
//!
//! El parámetro `t` va de 0 a 1. Los extremos de `point_at` y de `sample` son
//! exactos. El arco mide el ángulo desde el eje +x, en sentido antihorario, en
//! radianes. Este crate no elige unidades: quien lo llama decide si el plano
//! está en milímetros.

#![deny(clippy::print_stdout, clippy::print_stderr, clippy::exit)]

use std::fmt;

/// Punto del plano.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn distance(self, other: Self) -> f64 {
        (other.x - self.x).hypot(other.y - self.y)
    }

    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// Vector libre del plano.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    pub x: f64,
    pub y: f64,
}

impl Vector {
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    #[must_use]
    pub fn norm(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn try_unit(self) -> Result<Self, GeometryError> {
        let norm = self.norm();
        if norm > 0.0 && norm.is_finite() {
            Ok(Self::new(self.x / norm, self.y / norm))
        } else {
            Err(GeometryError::NoTangent)
        }
    }
}

impl std::ops::Add<Vector> for Point {
    type Output = Self;

    fn add(self, vector: Vector) -> Self {
        Self::new(self.x + vector.x, self.y + vector.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Vector;

    fn sub(self, other: Self) -> Vector {
        Vector::new(self.x - other.x, self.y - other.y)
    }
}

impl std::ops::Add for Vector {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl std::ops::Mul<f64> for Vector {
    type Output = Self;

    fn mul(self, scale: f64) -> Self {
        Self::new(self.x * scale, self.y * scale)
    }
}

/// Fallo al construir o evaluar una curva.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    /// `t` no es finito o está fuera de 0..=1.
    OutOfRange,
    /// La curva no tiene longitud: un punto, un radio nulo o un barrido nulo.
    Degenerate,
    /// La derivada es nula en ese parámetro.
    NoTangent,
    /// Hacen falta al menos dos puntos para muestrear, incluidos los extremos.
    TooFewSamples,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OutOfRange => "el parámetro está fuera de 0 a 1",
            Self::Degenerate => "la curva no tiene longitud ni tangente",
            Self::NoTangent => "la derivada es nula y no hay tangente",
            Self::TooFewSamples => "hacen falta al menos dos puntos",
        })
    }
}

impl std::error::Error for GeometryError {}

/// Curva parametrizada de 0 a 1.
pub trait Curve {
    /// Punto en `t`. En 0 y en 1 es el extremo, sin redondeo de más.
    fn point_at(&self, t: f64) -> Result<Point, GeometryError>;

    /// Tangente unitaria en el sentido de `t` creciente.
    fn tangent_at(&self, t: f64) -> Result<Vector, GeometryError>;

    /// Longitud de arco.
    fn length(&self) -> f64;

    /// `points` muestras, incluidos los dos extremos.
    fn sample(&self, points: usize) -> Result<Vec<Point>, GeometryError> {
        if points < 2 {
            return Err(GeometryError::TooFewSamples);
        }
        let last = points - 1;
        (0..points)
            .map(|index| self.point_at(index as f64 / last as f64))
            .collect()
    }
}

fn parameter(t: f64) -> Result<f64, GeometryError> {
    if t.is_finite() && (0.0..=1.0).contains(&t) {
        Ok(t)
    } else {
        Err(GeometryError::OutOfRange)
    }
}

/// Segmento recto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    start: Point,
    end: Point,
}

impl Line {
    /// Rechaza un segmento de longitud nula o con coordenadas no finitas.
    pub fn new(start: Point, end: Point) -> Result<Self, GeometryError> {
        if !start.is_finite() || !end.is_finite() || start.distance(end) == 0.0 {
            return Err(GeometryError::Degenerate);
        }
        Ok(Self { start, end })
    }

    #[must_use]
    pub fn start(self) -> Point {
        self.start
    }

    #[must_use]
    pub fn end(self) -> Point {
        self.end
    }
}

impl Curve for Line {
    fn point_at(&self, t: f64) -> Result<Point, GeometryError> {
        let t = parameter(t)?;
        if t <= 0.0 {
            return Ok(self.start);
        }
        if t >= 1.0 {
            return Ok(self.end);
        }
        Ok(self.start + (self.end - self.start) * t)
    }

    fn tangent_at(&self, t: f64) -> Result<Vector, GeometryError> {
        parameter(t)?;
        (self.end - self.start).try_unit()
    }

    fn length(&self) -> f64 {
        self.start.distance(self.end)
    }
}

/// Arco circular. `sweep` es el recorrido con signo, en radianes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    center: Point,
    radius: f64,
    start_angle: f64,
    sweep: f64,
}

impl Arc {
    /// `sweep` positivo recorre el arco en sentido antihorario.
    pub fn new(
        center: Point,
        radius: f64,
        start_angle: f64,
        sweep: f64,
    ) -> Result<Self, GeometryError> {
        if !center.is_finite()
            || !radius.is_finite()
            || !start_angle.is_finite()
            || !sweep.is_finite()
            || radius <= 0.0
            || sweep == 0.0
        {
            return Err(GeometryError::Degenerate);
        }
        Ok(Self {
            center,
            radius,
            start_angle,
            sweep,
        })
    }

    #[must_use]
    pub fn center(self) -> Point {
        self.center
    }

    #[must_use]
    pub fn radius(self) -> f64 {
        self.radius
    }

    fn angle_at(&self, t: f64) -> f64 {
        self.start_angle + t * self.sweep
    }

    fn offset(&self, angle: f64) -> Vector {
        Vector::new(self.radius * angle.cos(), self.radius * angle.sin())
    }
}

impl Curve for Arc {
    fn point_at(&self, t: f64) -> Result<Point, GeometryError> {
        let t = parameter(t)?;
        Ok(self.center + self.offset(self.angle_at(t)))
    }

    fn tangent_at(&self, t: f64) -> Result<Vector, GeometryError> {
        let t = parameter(t)?;
        let angle = self.angle_at(t);
        let direction = if self.sweep > 0.0 { 1.0 } else { -1.0 };
        Ok(Vector::new(
            direction * -angle.sin(),
            direction * angle.cos(),
        ))
    }

    fn length(&self) -> f64 {
        self.radius * self.sweep.abs()
    }
}

/// Bézier de grado n con n + 1 puntos de control. Se evalúa por De Casteljau,
/// que solo hace interpolaciones lineales y es estable con cualquier grado.
#[derive(Clone, Debug, PartialEq)]
pub struct Bezier {
    points: Vec<Point>,
}

impl Bezier {
    /// Rechaza menos de dos puntos, coordenadas no finitas y la curva de
    /// longitud nula.
    pub fn new(points: Vec<Point>) -> Result<Self, GeometryError> {
        if points.len() < 2 {
            return Err(GeometryError::TooFewSamples);
        }
        if !points.iter().copied().all(Point::is_finite) {
            return Err(GeometryError::Degenerate);
        }
        let curve = Self { points };
        if curve.length() == 0.0 {
            return Err(GeometryError::Degenerate);
        }
        Ok(curve)
    }

    /// La cúbica, con sus cuatro puntos de control.
    pub fn cubic(p0: Point, p1: Point, p2: Point, p3: Point) -> Result<Self, GeometryError> {
        Self::new(vec![p0, p1, p2, p3])
    }

    /// Los puntos de control, del primero al último.
    #[must_use]
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// Grado: uno menos que el número de puntos.
    #[must_use]
    pub fn degree(&self) -> usize {
        self.points.len() - 1
    }

    fn first(&self) -> Point {
        self.points[0]
    }

    fn last(&self) -> Point {
        self.points[self.points.len() - 1]
    }

    fn casteljau(points: &[Point], t: f64) -> Point {
        let mut work = points.to_vec();
        for level in (1..work.len()).rev() {
            for index in 0..level {
                work[index] = work[index] + (work[index + 1] - work[index]) * t;
            }
        }
        work[0]
    }

    /// Curvatura con signo en `t`: positiva si la curva gira a la izquierda
    /// (antihorario). κ = (x′y″ − y′x″)/|B′|³. Con derivada nula no hay
    /// tangente y es `NoTangent`.
    pub fn curvature_at(&self, t: f64) -> Result<f64, GeometryError> {
        let t = parameter(t)?;
        let first = self.derivative(t);
        let speed = first.norm();
        if speed == 0.0 || !speed.is_finite() {
            return Err(GeometryError::NoTangent);
        }
        let second = self.second_derivative(t);
        Ok((first.x * second.y - first.y * second.x) / (speed * speed * speed))
    }

    /// B″(t) = n(n−1)·Σ (P_(i+2) − 2P_(i+1) + P_i)·B_(i,n−2)(t). Nula si n < 2.
    fn second_derivative(&self, t: f64) -> Vector {
        let n = self.degree();
        if n < 2 {
            return Vector::new(0.0, 0.0);
        }
        let steps: Vec<Point> = self
            .points
            .windows(3)
            .map(|three| {
                Point::new(
                    three[2].x - 2.0 * three[1].x + three[0].x,
                    three[2].y - 2.0 * three[1].y + three[0].y,
                )
            })
            .collect();
        let at = Self::casteljau(&steps, t);
        let factor = (n * (n - 1)) as f64;
        Vector::new(factor * at.x, factor * at.y)
    }

    /// B′(t) = n·Σ (P_(i+1) − P_i)·B_(i,n−1)(t): la hodógrafa, otra Bézier.
    fn derivative(&self, t: f64) -> Vector {
        let n = self.degree() as f64;
        let legs: Vec<Point> = self
            .points
            .windows(2)
            .map(|pair| Point::new(pair[1].x - pair[0].x, pair[1].y - pair[0].y))
            .collect();
        let at = Self::casteljau(&legs, t);
        Vector::new(n * at.x, n * at.y)
    }
}

impl Curve for Bezier {
    fn point_at(&self, t: f64) -> Result<Point, GeometryError> {
        let t = parameter(t)?;
        if t <= 0.0 {
            return Ok(self.first());
        }
        if t >= 1.0 {
            return Ok(self.last());
        }
        Ok(Self::casteljau(&self.points, t))
    }

    fn tangent_at(&self, t: f64) -> Result<Vector, GeometryError> {
        let t = parameter(t)?;
        self.derivative(t).try_unit()
    }

    fn length(&self) -> f64 {
        // Simpson sobre |B′|. En una recta con puntos equiespaciados la
        // velocidad es constante y la cuadratura es exacta.
        const INTERVALS: usize = 64;
        let step = 1.0 / INTERVALS as f64;
        let mut sum = self.derivative(0.0).norm() + self.derivative(1.0).norm();
        for index in 1..INTERVALS {
            let weight = if index.is_multiple_of(2) { 2.0 } else { 4.0 };
            sum += weight * self.derivative(index as f64 * step).norm();
        }
        sum * step / 3.0
    }
}
