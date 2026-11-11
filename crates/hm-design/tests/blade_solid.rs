//! Las dos caras de cada línea de corriente del álabe con espesor: a un espesor
//! de distancia, sobre el cubo y la tapa, con el canto de salida sobre la
//! salida y los errores que dicen la línea y el radio.

use hm_design::blade_camber::{BladeCamber, CAMBER_STEPS, blade_camber};
use hm_design::blade_setup::{BladeShape, CamberBows};
use hm_design::blade_solid::{BladeSolidError, SolidPoint, blade_solid};
use hm_design::blade_thickness::{
    BladeThickness, ThicknessCurve, ThicknessCurves, ThicknessNode, thickness_curves_for_design,
};
use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, geometry};
use hm_design::inlet_triangles::{InletTriangle, inlet_triangles};
use hm_design::meridian::{MeridionalModel, meridional_model};
use hm_design::streamlines::{Streamline, streamlines};
use hm_design::{DesignInput, DesignResult, default_mechanical_efficiency, design};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

/// El punto de máximo rendimiento de B1, como en `tests/blade_camber.rs`.
fn b1_input() -> DesignInput {
    let q = parse_quantity("1060 gpm").unwrap();
    let n = parse_quantity("1750 rpm").unwrap();
    DesignInput {
        h: parse_quantity("83 m").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: default_mechanical_efficiency(&q, &n).unwrap(),
        npsha: None,
        nu: None,
        q,
        n,
    }
}

/// B1 con `count` líneas de corriente: resultado, meridiano, líneas y
/// triángulos.
fn b1(
    count: u8,
) -> (
    DesignResult,
    MeridionalModel,
    Vec<Streamline>,
    Vec<InletTriangle>,
) {
    let input = b1_input();
    let result = design(&input).unwrap();
    let impeller = geometry(&input, &result).unwrap().impeller;
    let model = meridional_model(&result).unwrap();
    let lines = streamlines(&model, count).unwrap();
    let triangles = inlet_triangles(&result, &impeller, &lines, DEFAULT_INCIDENCE_DEG).unwrap();
    (result, model, lines, triangles)
}

struct Case {
    result: DesignResult,
    model: MeridionalModel,
    lines: Vec<Streamline>,
    triangles: Vec<InletTriangle>,
    camber: BladeCamber,
    curves: ThicknessCurves,
}

fn case(shape: BladeShape, count: u8) -> Case {
    let (result, model, lines, triangles) = b1(count);
    let beta2b = result.selection.beta2_deg.to_radians();
    let camber = blade_camber(shape, &lines, &triangles, beta2b, &CamberBows::default()).unwrap();
    let curves = thickness_curves_for_design(&result, &camber, &BladeThickness::default()).unwrap();
    Case {
        result,
        model,
        lines,
        triangles,
        camber,
        curves,
    }
}

fn b1_case() -> Case {
    case(BladeShape::Free3D, 5)
}

impl Case {
    fn z(&self) -> u32 {
        self.result.selection.blade_count
    }
}

/// Una curva de espesor plana, de `mm` en las dos paredes.
fn flat(mm: f64) -> ThicknessCurves {
    let curve = || {
        ThicknessCurve::new(vec![
            ThicknessNode {
                x: 0.0,
                e: mm / 1000.0,
            },
            ThicknessNode {
                x: 1.0,
                e: mm / 1000.0,
            },
        ])
        .unwrap()
    };
    ThicknessCurves {
        hub: curve(),
        shroud: curve(),
    }
}

/// (x, y, z) cartesianas del punto, m.
fn cartesian(point: &SolidPoint) -> [f64; 3] {
    [
        point.r * point.theta.cos(),
        point.r * point.theta.sin(),
        point.z,
    ]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Distancia de `point` a la polilínea 3D.
fn distance_to_polyline(polyline: &[[f64; 3]], point: [f64; 3]) -> f64 {
    polyline
        .windows(2)
        .map(|pair| {
            let d = [
                pair[1][0] - pair[0][0],
                pair[1][1] - pair[0][1],
                pair[1][2] - pair[0][2],
            ];
            let squared = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let from = [
                point[0] - pair[0][0],
                point[1] - pair[0][1],
                point[2] - pair[0][2],
            ];
            let t = if squared > 0.0 {
                ((from[0] * d[0] + from[1] * d[1] + from[2] * d[2]) / squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            distance([from[0], from[1], from[2]], [t * d[0], t * d[1], t * d[2]])
        })
        .fold(f64::MAX, f64::min)
}

/// La x de la línea media en esa m′, interpolada entre sus nodos.
fn x_at(camber: &BladeCamber, line: usize, conformal: f64) -> f64 {
    let points = &camber.lines[line].points;
    let segment = points
        .partition_point(|p| p.m_conformal <= conformal)
        .saturating_sub(1)
        .min(points.len() - 2);
    let (a, b) = (&points[segment], &points[segment + 1]);
    let t = ((conformal - a.m_conformal) / (b.m_conformal - a.m_conformal)).clamp(0.0, 1.0);
    a.x + t * (b.x - a.x)
}

#[test]
fn the_faces_are_one_thickness_apart() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    let last = solid.lines.len() - 1;
    for index in [0, last / 2, last] {
        let line = &solid.lines[index];
        let suction: Vec<[f64; 3]> = line.suction.iter().map(cartesian).collect();
        let mut checked = 0;
        for point in &line.pressure {
            let x = x_at(&case.camber, index, point.m_conformal);
            if !(0.1..=0.9).contains(&x) {
                continue;
            }
            let expected = case.curves.at(line.psi, x);
            let measured = distance_to_polyline(&suction, cartesian(point));
            close(measured, expected, 5.0e-5);
            checked += 1;
        }
        assert!(
            checked > 100,
            "línea {index}: solo {checked} puntos medidos"
        );
    }
}

/// Distancia de (z, r) en m a una polilínea en mm.
fn distance_to_wall(wall: &[[f64; 2]], point: &SolidPoint) -> f64 {
    let p = [point.z * 1000.0, point.r * 1000.0];
    wall.windows(2)
        .map(|pair| {
            let (dz, dr) = (pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]);
            let squared = dz * dz + dr * dr;
            let t = if squared > 0.0 {
                (((p[0] - pair[0][0]) * dz + (p[1] - pair[0][1]) * dr) / squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p[0] - pair[0][0] - t * dz).hypot(p[1] - pair[0][1] - t * dr)
        })
        .fold(f64::MAX, f64::min)
        / 1000.0
}

/// Lo más que la propia línea de corriente (una poligonal de cuerdas) se aparta
/// de la pared, mirando los vértices y los puntos intermedios de cada tramo, m.
fn streamline_deviation(line: &Streamline, wall: &[[f64; 2]]) -> f64 {
    let mut worst = 0.0_f64;
    for pair in line.points.windows(2) {
        for k in 0..=10 {
            let t = f64::from(k) / 10.0;
            let at = [
                pair[0][0] + t * (pair[1][0] - pair[0][0]),
                pair[0][1] + t * (pair[1][1] - pair[0][1]),
            ];
            let point = SolidPoint {
                z: at[0] / 1000.0,
                r: at[1] / 1000.0,
                theta: 0.0,
                m_conformal: 0.0,
            };
            worst = worst.max(distance_to_wall(wall, &point));
        }
    }
    worst
}

/// Las caras del cubo y de la tapa quedan sobre su línea de corriente, que es
/// la pared. La poligonal de B1 se aparta de la curva de la pared hasta 12 µm
/// (es de cuerdas), así que el límite es ese, medido, y no el 1e-6 m del plan:
/// las caras no añaden nada a lo que ya se aparta la línea.
#[test]
fn the_faces_of_hub_and_shroud_lie_on_their_walls() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    let rear = &case.model.curve("Tapa trasera").unwrap().points;
    let front = &case.model.curve("Tapa delantera").unwrap().points;
    for (line, stream, wall) in [
        (solid.lines.first().unwrap(), &case.lines[0], rear),
        (
            solid.lines.last().unwrap(),
            case.lines.last().unwrap(),
            front,
        ),
    ] {
        let baseline = streamline_deviation(stream, wall);
        assert!(baseline < 2.0e-5, "la línea de corriente: {baseline} m");
        // Desde el nodo 0: aguas arriba de él la línea de corriente puede no
        // existir (en B1, la de la tapa empieza en el borde de ataque) y el
        // camino es su primer tramo prolongado en recta, fuera de la pared.
        for point in line
            .pressure
            .iter()
            .chain(&line.middle)
            .chain(&line.suction)
            .filter(|point| point.m_conformal >= 0.0)
        {
            let off = distance_to_wall(wall, point);
            assert!(
                off <= baseline + 1.0e-7,
                "ψ = {}: a {off} m de su pared, la línea a {baseline} m",
                line.psi
            );
        }
    }
}

#[test]
fn the_exit_edge_is_on_the_outlet_and_measures_e_over_sin_beta2() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    for (index, line) in solid.lines.iter().enumerate() {
        let outlet = case.camber.lines[index].points.last().unwrap();
        let [pressure, middle, suction] = line.trailing_edge();
        for point in [pressure, middle, suction] {
            close(point.r, outlet.r, 1.0e-9);
        }
        let expected = case.curves.at(line.psi, 1.0) / outlet.beta.sin();
        close(
            outlet.r * (suction.theta - pressure.theta),
            expected,
            5.0e-5,
        );
    }
}

#[test]
fn the_leading_edge_is_square_to_the_camber_line() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    for (index, line) in solid.lines.iter().enumerate() {
        let nodes = &case.camber.lines[index].points;
        let (dm, dt) = (
            nodes[1].m_conformal - nodes[0].m_conformal,
            nodes[1].theta - nodes[0].theta,
        );
        let length = dm.hypot(dt);
        let t = [dm / length, dt / length];
        let [pressure, middle, suction] = line.leading_edge();
        let across = [
            pressure.m_conformal - suction.m_conformal,
            pressure.theta - suction.theta,
        ];
        close(across[0] * t[0] + across[1] * t[1], 0.0, 1.0e-9);
        close(middle.theta, nodes[0].theta, 1.0e-12);
        close(middle.m_conformal, nodes[0].m_conformal, 1.0e-12);
        close(middle.z, nodes[0].z, 1.0e-9);
        close(middle.r, nodes[0].r, 1.0e-9);
    }
}

#[test]
fn the_pressure_face_is_on_the_side_of_rotation() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    for line in &solid.lines {
        let half = line.pressure.len() / 2;
        assert!(line.pressure[half].theta < line.middle[half].theta);
        assert!(line.middle[half].theta < line.suction[half].theta);
    }
}

#[test]
fn every_face_has_j_plus_one_points() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    assert_eq!(solid.lines.len(), 5);
    for line in &solid.lines {
        for face in [&line.pressure, &line.middle, &line.suction] {
            assert_eq!(face.len(), CAMBER_STEPS + 1);
        }
        assert!(line.true_thickness.is_empty());
        for point in line
            .pressure
            .iter()
            .chain(&line.middle)
            .chain(&line.suction)
        {
            assert!(
                [point.z, point.r, point.theta, point.m_conformal]
                    .iter()
                    .all(|v| v.is_finite())
            );
        }
    }
}

#[test]
fn the_edge_faces_are_the_first_and_last_stations() {
    let case = b1_case();
    let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap();
    for line in &solid.lines {
        let last = line.pressure.len() - 1;
        assert_eq!(
            line.leading_edge(),
            [line.pressure[0], line.middle[0], line.suction[0]]
        );
        assert_eq!(
            line.trailing_edge(),
            [line.pressure[last], line.middle[last], line.suction[last]]
        );
    }
}

#[test]
fn ruled_and_free_blades_both_build() {
    for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
        let case = case(shape, 5);
        let solid = blade_solid(&case.camber, &case.lines, &case.curves, case.z())
            .unwrap_or_else(|error| panic!("{shape:?}: {error}"));
        assert_eq!(solid.lines.len(), case.camber.lines.len());
    }
}

/// Con Z = 1 y 60 mm de espesor, B1 da `FacesCross`; con Z = 40 y 10 mm,
/// `NeighboursTouch` (los números del plan, comprobados con B1).
#[test]
fn faces_that_cross_are_an_error() {
    let case = b1_case();
    let error = blade_solid(&case.camber, &case.lines, &flat(60.0), 1).unwrap_err();
    assert!(
        matches!(error, BladeSolidError::FacesCross { .. }),
        "{error:?}"
    );
}

#[test]
fn neighbours_that_touch_are_an_error() {
    let case = b1_case();
    let error = blade_solid(&case.camber, &case.lines, &flat(10.0), 40).unwrap_err();
    assert!(
        matches!(error, BladeSolidError::NeighboursTouch { .. }),
        "{error:?}"
    );
}

#[test]
fn an_inclined_outlet_is_an_error() {
    let mut case = b1_case();
    let last = case.camber.lines.len() - 1;
    case.camber.lines[last].points.last_mut().unwrap().r -= 0.005;
    let error = blade_solid(&case.camber, &case.lines, &case.curves, case.z()).unwrap_err();
    assert!(
        matches!(error, BladeSolidError::InclinedOutlet { line, .. } if line == last),
        "{error:?}"
    );
}

#[test]
fn lines_that_do_not_match_are_an_error() {
    let case = b1_case();
    let error = blade_solid(&case.camber, &case.lines[..3], &case.curves, case.z()).unwrap_err();
    assert_eq!(error, BladeSolidError::LineMismatch);

    let mut moved = case.lines.clone();
    moved[2].psi += 0.1;
    let error = blade_solid(&case.camber, &moved, &case.curves, case.z()).unwrap_err();
    assert_eq!(error, BladeSolidError::LineMismatch);
}

#[test]
fn a_leading_edge_at_the_start_of_the_streamline_still_builds() {
    let case = b1_case();
    // Cada línea empieza en su corte con el borde de ataque: no queda línea de
    // corriente aguas arriba y el camino se prolonga en recta.
    let trimmed: Vec<Streamline> = case
        .lines
        .iter()
        .map(|line| {
            let mut walked = 0.0;
            let mut points = vec![line.leading_edge];
            for pair in line.points.windows(2) {
                let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                if walked >= line.m_leading_edge {
                    points.push(pair[0]);
                }
                walked += length;
            }
            points.push(*line.points.last().unwrap());
            Streamline {
                points,
                m_leading_edge: 0.0,
                ..line.clone()
            }
        })
        .collect();
    let beta2b = case.result.selection.beta2_deg.to_radians();
    let camber = blade_camber(
        BladeShape::Free3D,
        &trimmed,
        &case.triangles,
        beta2b,
        &CamberBows::default(),
    )
    .unwrap();
    let solid = blade_solid(&camber, &trimmed, &case.curves, case.z()).unwrap();
    for line in &solid.lines {
        for point in line
            .pressure
            .iter()
            .chain(&line.middle)
            .chain(&line.suction)
        {
            assert!(
                [point.z, point.r, point.theta, point.m_conformal]
                    .iter()
                    .all(|v| v.is_finite()),
                "{point:?}"
            );
        }
    }
}

#[test]
fn the_messages_name_the_line_and_the_radius_with_a_decimal_comma() {
    assert_eq!(
        BladeSolidError::FacesCross {
            line: 2,
            radius: 0.04512,
            at: [0.0, 0.0]
        }
        .to_string(),
        "línea de corriente 2 (0 es el cubo): las caras del álabe se cruzan en r = 45,1 mm: \
         el radio de curvatura de la línea media baja de medio espesor"
    );
    assert_eq!(
        BladeSolidError::NeighboursTouch {
            line: 0,
            radius: 0.1,
            at: [0.0, 0.0]
        }
        .to_string(),
        "línea de corriente 0 (0 es el cubo): los álabes vecinos se tocan en r = 100,0 mm"
    );
    assert_eq!(
        BladeSolidError::InclinedOutlet {
            line: 4,
            radius: 0.2045
        }
        .to_string(),
        "línea de corriente 4 (0 es el cubo): la salida del rodete no es radial (r = 204,5 mm): \
         el recorte del álabe en una salida inclinada no está resuelto"
    );
    assert_eq!(
        BladeSolidError::LineMismatch.to_string(),
        "las líneas de corriente no son las de la línea media"
    );
    assert_eq!(
        BladeSolidError::NoCamber("la forma no es 3D".into()).to_string(),
        "no hay línea media 3D y el álabe con espesor no se construye: la forma no es 3D"
    );
}
