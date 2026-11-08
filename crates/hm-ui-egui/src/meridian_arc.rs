//! Arco de un tramo del contorno mecánico: el arco menor de radio |R| entre
//! sus dos nodos. El signo de R dice hacia dónde se curva: positivo a la
//! izquierda de la cuerda recorrida de `a` a `b` (en el plano z, r), negativo
//! a la derecha. Se recorre a velocidad angular constante, así que la
//! fracción `f` de un punto es también la de su ángulo.

/// El círculo del arco y lo que gira de `a` a `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Arc {
    pub center: [f64; 2],
    pub a: [f64; 2],
    /// Ángulo que gira de `a` a `b`, en radianes, con signo (antihorario
    /// positivo). Nunca pasa de media vuelta.
    pub sweep: f64,
}

impl Arc {
    /// El arco de radio `radius` (con signo) entre `a` y `b`. `None` si no
    /// cabe: |R| menor que media cuerda, R nula o los dos puntos iguales.
    pub(super) fn new(a: [f64; 2], b: [f64; 2], radius: f64) -> Option<Self> {
        let chord = [b[0] - a[0], b[1] - a[1]];
        let length = chord[0].hypot(chord[1]);
        let half = length / 2.0;
        let r = radius.abs();
        // Un poco de holgura para que media vuelta exacta quepa.
        if !(radius.is_finite() && r > 0.0 && length > 1.0e-12 && r >= half * (1.0 - 1.0e-12)) {
            return None;
        }
        let left = [-chord[1] / length, chord[0] / length];
        let rise = (r * r - half * half).max(0.0).sqrt();
        // El arco menor se curva hacia un lado: el centro queda al otro.
        let side = radius.signum();
        let middle = [a[0] + chord[0] / 2.0, a[1] + chord[1] / 2.0];
        let center = [
            middle[0] - side * rise * left[0],
            middle[1] - side * rise * left[1],
        ];
        let sweep = -side * 2.0 * (half / r).min(1.0).asin();
        Some(Self { center, a, sweep })
    }

    /// El punto en la fracción `f` del arco, de 0 (`a`) a 1 (`b`).
    pub(super) fn at(&self, f: f64) -> [f64; 2] {
        let (sin, cos) = (self.sweep * f).sin_cos();
        let d = [self.a[0] - self.center[0], self.a[1] - self.center[1]];
        [
            self.center[0] + d[0] * cos - d[1] * sin,
            self.center[1] + d[0] * sin + d[1] * cos,
        ]
    }
}

impl Arc {
    /// La dirección unitaria de avance en la fracción `f`, de `a` hacia `b`.
    pub(super) fn direction(&self, f: f64) -> [f64; 2] {
        let p = self.at(f);
        let d = [p[0] - self.center[0], p[1] - self.center[1]];
        let n = d[0].hypot(d[1]);
        let s = self.sweep.signum();
        [-s * d[1] / n, s * d[0] / n]
    }
}

/// Lo menos que gira un arco tangente, en radianes: su nodo lejano no se
/// junta con el de la tangencia.
const MIN_SWEEP: f64 = 0.01;

/// El círculo de un arco que sale de `anchor` en la dirección `dir`
/// (tangente al tramo vecino) con radio `radius`, con el signo de [`Arc`]
/// visto desde `anchor`: positivo, se curva a la derecha de `dir` (el arco
/// queda a la izquierda de su cuerda). El nodo del otro extremo solo puede
/// estar sobre él, entre un giro mínimo y media vuelta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Tangent {
    pub center: [f64; 2],
    anchor: [f64; 2],
    /// +1 si gira en sentido antihorario, -1 si en el horario.
    turn: f64,
}

impl Tangent {
    /// `None` si la dirección es nula o el radio no vale.
    pub(super) fn new(anchor: [f64; 2], dir: [f64; 2], radius: f64) -> Option<Self> {
        let n = dir[0].hypot(dir[1]);
        if !(radius.is_finite() && radius != 0.0 && n > 1.0e-12) {
            return None;
        }
        let left = [-dir[1] / n, dir[0] / n];
        let center = [anchor[0] - radius * left[0], anchor[1] - radius * left[1]];
        Some(Self {
            center,
            anchor,
            turn: -radius.signum(),
        })
    }

    /// Lo que gira desde `anchor` hasta el punto del círculo en la dirección
    /// de `p`, en el sentido del arco, entre 0 y una vuelta.
    fn angle_of(&self, p: [f64; 2]) -> f64 {
        let a = [
            self.anchor[0] - self.center[0],
            self.anchor[1] - self.center[1],
        ];
        let b = [p[0] - self.center[0], p[1] - self.center[1]];
        let cross = a[0] * b[1] - a[1] * b[0];
        let dot = a[0] * b[0] + a[1] * b[1];
        (self.turn * cross.atan2(dot)).rem_euclid(std::f64::consts::TAU)
    }

    /// El punto del círculo tras girar `angle` desde `anchor`.
    fn at_angle(&self, angle: f64) -> [f64; 2] {
        let (sin, cos) = (self.turn * angle).sin_cos();
        let d = [
            self.anchor[0] - self.center[0],
            self.anchor[1] - self.center[1],
        ];
        [
            self.center[0] + d[0] * cos - d[1] * sin,
            self.center[1] + d[0] * sin + d[1] * cos,
        ]
    }

    /// Un giro llevado al tramo que vale: lo que pasa de media vuelta va al
    /// extremo más cercano.
    fn clamp(angle: f64) -> f64 {
        use std::f64::consts::PI;
        if angle <= PI {
            angle.max(MIN_SWEEP)
        } else if angle < 1.5 * PI {
            PI
        } else {
            MIN_SWEEP
        }
    }

    /// El punto del arco más próximo a la dirección de `p`.
    pub(super) fn project(&self, p: [f64; 2]) -> [f64; 2] {
        self.at_angle(Self::clamp(self.angle_of(p)))
    }

    /// El punto del arco donde la z (`axis` 0) o la r (`axis` 1) vale
    /// `value`; de los dos que puede haber, el más cercano a `near`. `None`
    /// si el arco no llega.
    pub(super) fn crossing(&self, axis: usize, value: f64, near: [f64; 2]) -> Option<[f64; 2]> {
        let radius = (self.anchor[0] - self.center[0]).hypot(self.anchor[1] - self.center[1]);
        let d = value - self.center[axis];
        if d.abs() > radius {
            return None;
        }
        let other = (radius * radius - d * d).sqrt();
        [-1.0, 1.0]
            .into_iter()
            .map(|side| {
                let mut p = [0.0; 2];
                p[axis] = value;
                p[1 - axis] = self.center[1 - axis] + side * other;
                p
            })
            .filter(|p| {
                let angle = self.angle_of(*p);
                (MIN_SWEEP * 0.999..=std::f64::consts::PI + 1.0e-9).contains(&angle)
            })
            .min_by(|p, q| {
                let dp = (p[0] - near[0]).hypot(p[1] - near[1]);
                let dq = (q[0] - near[0]).hypot(q[1] - near[1]);
                dp.total_cmp(&dq)
            })
    }
}

/// El radio, con signo, del círculo que pasa por `a`, `p` y `b`; `None` si
/// están alineados. Sirve para proponer el arco que más se parece a la curva.
pub(super) fn through(a: [f64; 2], p: [f64; 2], b: [f64; 2]) -> Option<f64> {
    let (ab, ap, pb) = (
        [b[0] - a[0], b[1] - a[1]],
        [p[0] - a[0], p[1] - a[1]],
        [b[0] - p[0], b[1] - p[1]],
    );
    let cross = ab[0] * ap[1] - ab[1] * ap[0];
    if cross.abs() < 1.0e-12 {
        return None;
    }
    let lengths = ab[0].hypot(ab[1]) * ap[0].hypot(ap[1]) * pb[0].hypot(pb[1]);
    // R = abc / (4·área), y el área es |cross| / 2.
    Some(lengths / (2.0 * cross))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(p: [f64; 2], q: [f64; 2]) -> bool {
        (p[0] - q[0]).abs() < 1.0e-9 && (p[1] - q[1]).abs() < 1.0e-9
    }

    #[test]
    fn the_arc_joins_its_two_ends_with_its_radius() {
        let (a, b) = ([0.0, 0.0], [6.0, 0.0]);
        let arc = Arc::new(a, b, 5.0).unwrap();
        assert!(same(arc.at(0.0), a));
        assert!(same(arc.at(1.0), b));
        for k in 0..=10 {
            let p = arc.at(f64::from(k) / 10.0);
            let r = (p[0] - arc.center[0]).hypot(p[1] - arc.center[1]);
            assert!((r - 5.0).abs() < 1.0e-9, "{p:?}");
        }
    }

    #[test]
    fn the_sign_says_to_which_side_it_bulges() {
        let (a, b) = ([0.0, 0.0], [6.0, 0.0]);
        // De a hacia b la izquierda es r positiva.
        assert!(Arc::new(a, b, 5.0).unwrap().at(0.5)[1] > 0.0);
        assert!(Arc::new(a, b, -5.0).unwrap().at(0.5)[1] < 0.0);
        // Es el arco menor: la flecha es R - √(R² - 3²) = 1.
        assert!((Arc::new(a, b, 5.0).unwrap().at(0.5)[1] - 1.0).abs() < 1.0e-9);
    }

    #[test]
    fn half_a_turn_fits_and_less_than_half_the_chord_does_not() {
        let (a, b) = ([0.0, 0.0], [6.0, 0.0]);
        let half = Arc::new(a, b, 3.0).unwrap();
        assert!(same(half.at(0.5), [3.0, 3.0]));
        assert!(Arc::new(a, b, 2.9).is_none());
        assert!(Arc::new(a, b, 0.0).is_none());
        assert!(Arc::new(a, a, 5.0).is_none());
    }

    #[test]
    fn walking_it_backwards_with_the_opposite_sign_is_the_same_arc() {
        let (a, b) = ([1.0, 2.0], [7.0, 5.0]);
        let forward = Arc::new(a, b, 8.0).unwrap();
        let backward = Arc::new(b, a, -8.0).unwrap();
        for k in 0..=10 {
            let f = f64::from(k) / 10.0;
            assert!(same(forward.at(f), backward.at(1.0 - f)));
        }
    }

    #[test]
    fn the_circle_through_three_points_gives_the_signed_radius() {
        let (a, b) = ([0.0, 0.0], [6.0, 0.0]);
        let arc = Arc::new(a, b, 5.0).unwrap();
        let r = through(a, arc.at(0.3), b).unwrap();
        assert!((r - 5.0).abs() < 1.0e-9, "{r}");
        let below = Arc::new(a, b, -5.0).unwrap();
        assert!((through(a, below.at(0.5), b).unwrap() + 5.0).abs() < 1.0e-9);
        assert!(through(a, [3.0, 0.0], b).is_none());
    }

    #[test]
    fn the_direction_follows_the_arc_from_a_to_b() {
        let arc = Arc::new([0.0, 0.0], [6.0, 0.0], 3.0).unwrap();
        // Media vuelta por arriba: sale hacia +r y llega hacia -r.
        assert!(same(arc.direction(0.0), [0.0, 1.0]));
        assert!(same(arc.direction(1.0), [0.0, -1.0]));
        let below = Arc::new([0.0, 0.0], [6.0, 0.0], -3.0).unwrap();
        assert!(same(below.direction(0.0), [0.0, -1.0]));
    }

    #[test]
    fn a_tangent_arc_leaves_along_the_direction_and_keeps_its_radius() {
        // Sale de (0, 0) hacia +z con R 5: el centro queda debajo (a la
        // derecha de la dirección) y gira en el sentido horario.
        let tangent = Tangent::new([0.0, 0.0], [1.0, 0.0], 5.0).unwrap();
        assert!(same(tangent.center, [0.0, -5.0]));
        let far = tangent.project([10.0, -5.0]);
        assert!(same(far, [5.0, -5.0]));
        // El arco de ese radio entre los dos nodos es el mismo círculo y
        // sale en la dirección dada.
        let arc = Arc::new([0.0, 0.0], far, 5.0).unwrap();
        assert!(same(arc.center, tangent.center));
        assert!(same(arc.direction(0.0), [1.0, 0.0]));
        // Con R negativa se curva hacia el otro lado.
        let up = Tangent::new([0.0, 0.0], [1.0, 0.0], -5.0).unwrap();
        assert!(same(up.center, [0.0, 5.0]));
    }

    #[test]
    fn the_far_node_stays_between_a_small_turn_and_half_a_turn() {
        let tangent = Tangent::new([0.0, 0.0], [1.0, 0.0], 5.0).unwrap();
        // Más allá de media vuelta se queda en media vuelta.
        assert!(same(tangent.project([-3.0, -9.0]), [0.0, -10.0]));
        // Detrás de la tangencia se queda junto a ella, sin tocarla.
        let near = tangent.project([-3.0, -1.0]);
        assert!(near[0] > 0.0 && near[0] < 0.1, "{near:?}");
    }

    #[test]
    fn a_written_z_or_r_lands_on_the_arc() {
        let tangent = Tangent::new([0.0, 0.0], [1.0, 0.0], 5.0).unwrap();
        let p = tangent.crossing(0, 3.0, [3.0, -1.0]).unwrap();
        assert!(same(p, [3.0, -1.0]));
        // Con z = 3 hay dos puntos del arco: va al más cercano.
        let q = tangent.crossing(0, 3.0, [3.0, -9.0]).unwrap();
        assert!(same(q, [3.0, -9.0]));
        // Con z = -3 los dos pasan de media vuelta.
        assert!(tangent.crossing(0, -3.0, [0.0, 0.0]).is_none());
        let r = tangent.crossing(1, -5.0, [0.0, 0.0]).unwrap();
        assert!(same(r, [5.0, -5.0]));
        assert!(tangent.crossing(0, 6.0, [0.0, 0.0]).is_none());
        assert!(tangent.crossing(0, -1.0, [0.0, 0.0]).is_none());
    }
}
