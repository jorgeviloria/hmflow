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
        let center = [middle[0] - side * rise * left[0], middle[1] - side * rise * left[1]];
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
}
