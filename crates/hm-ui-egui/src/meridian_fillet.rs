//! Redondeo de una esquina del contorno mecánico: el arco de radio R tangente
//! a los dos tramos que llegan al vértice. Un tramo es una recta o un trozo
//! de la Bézier hidráulica; recta contra recta sale con una fórmula cerrada,
//! recta contra curva se busca por bisección. Dos curvas no hacen esquina.

/// Un tramo visto desde el vértice.
pub(super) enum Side<'a> {
    /// Recta hasta este otro extremo.
    Line([f64; 2]),
    /// Curva con `s` de 0 (el vértice) a 1 (el otro extremo).
    Curve(&'a dyn Fn(f64) -> [f64; 2]),
}

/// El empalme: el centro, los puntos de tangencia y, en cada tramo, la
/// fracción que recorta desde el vértice (`s`, como en [`Side`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fillet {
    pub center: [f64; 2],
    pub radius: f64,
    /// Tangencia sobre el tramo de entrada.
    pub a: [f64; 2],
    /// Tangencia sobre el tramo de salida.
    pub b: [f64; 2],
    pub sa: f64,
    pub sb: f64,
}

/// Pasos con que se busca la primera tangencia sobre una curva.
const SCAN: usize = 400;

/// Paso con que se deriva la curva.
const H: f64 = 1e-5;

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: [f64; 2], b: [f64; 2], k: f64) -> [f64; 2] {
    [a[0] + b[0] * k, a[1] + b[1] * k]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn norm(a: [f64; 2]) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: [f64; 2]) -> Option<[f64; 2]> {
    let n = norm(a);
    (n > 1e-12).then(|| [a[0] / n, a[1] / n])
}

fn perp(a: [f64; 2]) -> [f64; 2] {
    [-a[1], a[0]]
}

/// La tangente unitaria de la curva en `s`, en el sentido de `s` creciente.
fn tangent(curve: &dyn Fn(f64) -> [f64; 2], s: f64) -> Option<[f64; 2]> {
    let (low, high) = ((s - H).max(0.0), (s + H).min(1.0));
    unit(sub(curve(high), curve(low)))
}

/// El arco de radio `radius` en la esquina `vertex` entre el tramo de
/// entrada `a` y el de salida `b`; `None` si no hay esquina (dos curvas, o
/// tramos alineados) o si el radio no cabe en los tramos.
pub(super) fn fillet(vertex: [f64; 2], a: &Side, b: &Side, radius: f64) -> Option<Fillet> {
    if !(radius > 0.0 && radius.is_finite()) {
        return None;
    }
    match (a, b) {
        (Side::Line(far_a), Side::Line(far_b)) => lines(vertex, *far_a, *far_b, radius),
        (Side::Line(far), Side::Curve(curve)) => line_curve(vertex, *far, *curve, radius),
        (Side::Curve(curve), Side::Line(far)) => {
            line_curve(vertex, *far, *curve, radius).map(|f| Fillet {
                a: f.b,
                b: f.a,
                sa: f.sb,
                sb: f.sa,
                ..f
            })
        }
        (Side::Curve(_), Side::Curve(_)) => None,
    }
}

fn lines(vertex: [f64; 2], far_a: [f64; 2], far_b: [f64; 2], radius: f64) -> Option<Fillet> {
    let (la, lb) = (norm(sub(far_a, vertex)), norm(sub(far_b, vertex)));
    let (da, db) = (unit(sub(far_a, vertex))?, unit(sub(far_b, vertex))?);
    let angle = dot(da, db).clamp(-1.0, 1.0).acos();
    let half = angle / 2.0;
    if half.sin() < 1e-6 || half.tan() < 1e-6 || (std::f64::consts::PI - angle) < 1e-6 {
        return None;
    }
    let back = radius / half.tan();
    if back > la || back > lb {
        return None;
    }
    let bisector = unit(add(da, db, 1.0))?;
    Some(Fillet {
        center: add(vertex, bisector, radius / half.sin()),
        radius,
        a: add(vertex, da, back),
        b: add(vertex, db, back),
        sa: back / la,
        sb: back / lb,
    })
}

/// La recta va del vértice a `far`; la curva sale del vértice. Devuelve
/// `a` sobre la recta y `b` sobre la curva.
fn line_curve(
    vertex: [f64; 2],
    far: [f64; 2],
    curve: &dyn Fn(f64) -> [f64; 2],
    radius: f64,
) -> Option<Fillet> {
    let length = norm(sub(far, vertex));
    let dl = unit(sub(far, vertex))?;
    let dc = tangent(curve, 0.0)?;
    if cross(dl, dc).abs() < 1e-6 && dot(dl, dc) < 0.0 {
        // La curva sigue a la recta: no hay esquina.
        return None;
    }
    // Normales hacia dentro de la esquina.
    let mut nl = perp(dl);
    if dot(nl, dc) < 0.0 {
        nl = [-nl[0], -nl[1]];
    }
    let sign = if dot(perp(dc), dl) >= 0.0 { 1.0 } else { -1.0 };
    let center_at = |s: f64| -> Option<[f64; 2]> {
        let t = tangent(curve, s)?;
        let n = perp(t);
        Some(add(curve(s), [n[0] * sign, n[1] * sign], radius))
    };
    // Distancia con signo del centro a la recta, menos R: negativa junto al
    // vértice y cero donde el círculo toca también la recta.
    let gap = |s: f64| center_at(s).map(|c| dot(sub(c, vertex), nl) - radius);
    let mut low = 0.0;
    let mut found = None;
    for k in 1..=SCAN {
        let s = k as f64 / SCAN as f64;
        if gap(s)? >= 0.0 {
            found = Some(s);
            break;
        }
        low = s;
    }
    let mut high = found?;
    for _ in 0..60 {
        let mid = 0.5 * (low + high);
        if gap(mid)? >= 0.0 {
            high = mid;
        } else {
            low = mid;
        }
    }
    let s = high;
    let center = center_at(s)?;
    let along = dot(sub(center, vertex), dl);
    if along <= 0.0 || along > length {
        return None;
    }
    Some(Fillet {
        center,
        radius,
        a: add(vertex, dl, along),
        b: curve(s),
        sa: along / length,
        sb: s,
    })
}

/// El arco de `a` a `b`, por el lado corto, con `count + 1` puntos.
pub(super) fn arc(fillet: &Fillet, count: usize) -> Vec<[f64; 2]> {
    let angle = |p: [f64; 2]| (p[1] - fillet.center[1]).atan2(p[0] - fillet.center[0]);
    let start = angle(fillet.a);
    let mut sweep = angle(fillet.b) - start;
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    (0..=count)
        .map(|k| {
            let phi = start + sweep * k as f64 / count as f64;
            [
                fillet.center[0] + fillet.radius * phi.cos(),
                fillet.center[1] + fillet.radius * phi.sin(),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn distance_to_line(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
        cross(sub(b, a), sub(p, a)).abs() / norm(sub(b, a))
    }

    #[test]
    fn a_square_corner_gets_the_textbook_arc() {
        // Baja por z = 0 y gira hacia z > 0: esquina en (0, 0).
        let f = fillet(
            [0.0, 0.0],
            &Side::Line([0.0, 20.0]),
            &Side::Line([30.0, 0.0]),
            5.0,
        )
        .unwrap();
        assert!(close(f.center[0], 5.0) && close(f.center[1], 5.0), "{f:?}");
        assert!(close(f.a[1], 5.0) && close(f.a[0], 0.0));
        assert!(close(f.b[0], 5.0) && close(f.b[1], 0.0));
        assert!(close(f.sa, 0.25) && close(f.sb, 5.0 / 30.0));
    }

    #[test]
    fn an_open_corner_is_tangent_to_both_lines() {
        let (v, p, q) = ([10.0, 10.0], [0.0, 40.0], [50.0, 15.0]);
        let f = fillet(v, &Side::Line(p), &Side::Line(q), 8.0).unwrap();
        assert!(close(distance_to_line(f.center, v, p), 8.0));
        assert!(close(distance_to_line(f.center, v, q), 8.0));
        assert!(close(norm(sub(f.center, f.a)), 8.0));
        assert!(close(norm(sub(f.center, f.b)), 8.0));
    }

    #[test]
    fn a_radius_too_big_for_the_lines_does_not_fit() {
        let none = fillet(
            [0.0, 0.0],
            &Side::Line([0.0, 4.0]),
            &Side::Line([30.0, 0.0]),
            5.0,
        );
        assert_eq!(none, None);
    }

    #[test]
    fn aligned_lines_have_no_corner() {
        let none = fillet(
            [0.0, 0.0],
            &Side::Line([-10.0, 0.0]),
            &Side::Line([10.0, 0.0]),
            5.0,
        );
        assert_eq!(none, None);
    }

    #[test]
    fn a_line_and_a_curve_are_both_tangent() {
        // Un cuarto de circunferencia de radio 40 centrada en (40, 0), que
        // sale vertical de (0, 0), y la recta horizontal hacia z < 0: una
        // esquina de 90° como la del círculo blanco.
        let quarter = |s: f64| {
            let phi = std::f64::consts::PI - s * std::f64::consts::FRAC_PI_2;
            [40.0 + 40.0 * phi.cos(), 40.0 * phi.sin()]
        };
        let v = quarter(0.0);
        let f = fillet(v, &Side::Line([-30.0, 0.0]), &Side::Curve(&quarter), 5.0).unwrap();
        // Tangente a la recta r = 0.
        assert!(close(f.center[1], 5.0), "{f:?}");
        assert!(close(f.a[1], 0.0));
        // Tangente a la circunferencia, por fuera: centros a 40 + 5.
        assert!(close(norm(sub(f.center, [40.0, 0.0])), 45.0), "{f:?}");
        assert!(close(norm(sub(f.center, f.b)), 5.0));
        // Y la curva al revés da lo mismo con los lados cambiados.
        let g = fillet(v, &Side::Curve(&quarter), &Side::Line([-30.0, 0.0]), 5.0).unwrap();
        assert_eq!((g.a, g.b, g.sa, g.sb), (f.b, f.a, f.sb, f.sa));
    }

    #[test]
    fn two_curves_make_no_corner() {
        let c = |s: f64| [s, s * s];
        assert_eq!(
            fillet([0.0, 0.0], &Side::Curve(&c), &Side::Curve(&c), 1.0),
            None
        );
    }

    #[test]
    fn the_arc_goes_the_short_way() {
        let f = fillet(
            [0.0, 0.0],
            &Side::Line([0.0, 20.0]),
            &Side::Line([30.0, 0.0]),
            5.0,
        )
        .unwrap();
        let points = arc(&f, 10);
        assert_eq!(points.len(), 11);
        assert!(close(points[0][0], f.a[0]) && close(points[0][1], f.a[1]));
        assert!(close(points[10][0], f.b[0]) && close(points[10][1], f.b[1]));
        // El punto medio queda hacia la esquina, no al otro lado del centro.
        assert!(points[5][0] < 5.0 && points[5][1] < 5.0);
    }
}
