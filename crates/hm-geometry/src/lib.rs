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

/// Bézier cúbica con los cuatro puntos de control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CubicBezier {
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
}

impl CubicBezier {
    /// Rechaza coordenadas no finitas y la curva de longitud nula.
    pub fn new(p0: Point, p1: Point, p2: Point, p3: Point) -> Result<Self, GeometryError> {
        if ![p0, p1, p2, p3].into_iter().all(Point::is_finite) {
            return Err(GeometryError::Degenerate);
        }
        let curve = Self { p0, p1, p2, p3 };
        if curve.length() == 0.0 {
            return Err(GeometryError::Degenerate);
        }
        Ok(curve)
    }

    #[must_use]
    pub fn p0(self) -> Point {
        self.p0
    }

    #[must_use]
    pub fn p3(self) -> Point {
        self.p3
    }

    fn derivative(&self, t: f64) -> Vector {
        let u = 1.0 - t;
        (self.p1 - self.p0) * (3.0 * u * u)
            + (self.p2 - self.p1) * (6.0 * u * t)
            + (self.p3 - self.p2) * (3.0 * t * t)
    }

    fn at(&self, t: f64) -> Point {
        let u = 1.0 - t;
        let uu = u * u;
        let tt = t * t;
        let weight0 = uu * u;
        let weight1 = 3.0 * uu * t;
        let weight2 = 3.0 * u * tt;
        let weight3 = tt * t;
        Point::new(
            weight0 * self.p0.x + weight1 * self.p1.x + weight2 * self.p2.x + weight3 * self.p3.x,
            weight0 * self.p0.y + weight1 * self.p1.y + weight2 * self.p2.y + weight3 * self.p3.y,
        )
    }
}

impl Curve for CubicBezier {
    fn point_at(&self, t: f64) -> Result<Point, GeometryError> {
        let t = parameter(t)?;
        if t <= 0.0 {
            return Ok(self.p0);
        }
        if t >= 1.0 {
            return Ok(self.p3);
        }
        Ok(self.at(t))
    }

    fn tangent_at(&self, t: f64) -> Result<Vector, GeometryError> {
        let t = parameter(t)?;
        self.derivative(t).try_unit()
    }

    fn length(&self) -> f64 {
        // Simpson sobre |B′|. En una recta la velocidad es constante y la
        // cuadratura es exacta.
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
