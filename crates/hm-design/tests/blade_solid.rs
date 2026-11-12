//! Las dos caras de cada línea de corriente del álabe con espesor: a un espesor
//! de distancia, sobre el cubo y la tapa, con el canto de salida sobre la
//! salida y los errores que dicen la línea y el radio.

use hm_design::blade_camber::{BladeCamber, CAMBER_STEPS, blade_camber};
use hm_design::blade_edges::{BladeEdges, LeadingDims, LeadingKind};
use hm_design::blade_setup::{BladeSetup, BladeShape, CamberBows};
use hm_design::blade_solid::{
    BladeSolid, BladeSolidError, NOSE_STEPS, READING_STATIONS, SolidLine, SolidPoint, blade_solid,
    blade_solid_for_design,
};
use hm_design::blade_thickness::{
    BladeThickness, ThicknessCurve, ThicknessCurves, ThicknessNode, thickness_curves_for_design,
};
use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, ImpellerGeometry, geometry};
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
    ImpellerGeometry,
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
    (result, impeller, model, lines, triangles)
}

struct Case {
    result: DesignResult,
    impeller: ImpellerGeometry,
    model: MeridionalModel,
    lines: Vec<Streamline>,
    triangles: Vec<InletTriangle>,
    camber: BladeCamber,
    curves: ThicknessCurves,
}

fn case(shape: BladeShape, count: u8) -> Case {
    let (result, impeller, model, lines, triangles) = b1(count);
    let beta2b = result.selection.beta2_deg.to_radians();
    let camber = blade_camber(shape, &lines, &triangles, beta2b, &CamberBows::default()).unwrap();
    let curves = thickness_curves_for_design(&result, &camber, &BladeThickness::default()).unwrap();
    Case {
        result,
        impeller,
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    for line in &solid.lines {
        let half = line.pressure.len() / 2;
        assert!(line.pressure[half].theta < line.middle[half].theta);
        assert!(line.middle[half].theta < line.suction[half].theta);
    }
}

#[test]
fn every_face_has_j_plus_one_points() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    assert_eq!(solid.lines.len(), 5);
    for line in &solid.lines {
        for face in [&line.pressure, &line.middle, &line.suction] {
            assert_eq!(face.len(), CAMBER_STEPS + 1);
        }
        assert_eq!(line.true_thickness.len(), CAMBER_STEPS + 1);
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
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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
        let solid = blade_solid(
            &case.camber,
            &case.lines,
            &case.curves,
            &BladeEdges::default(),
            true,
            case.z(),
        )
        .unwrap_or_else(|error| panic!("{shape:?}: {error}"));
        assert_eq!(solid.lines.len(), case.camber.lines.len());
    }
}

/// Con Z = 1 y 60 mm de espesor, B1 da `FacesCross`; con Z = 40 y 10 mm,
/// `NeighboursTouch` (los números del plan, comprobados con B1).
#[test]
fn faces_that_cross_are_an_error() {
    let case = b1_case();
    let error = blade_solid(
        &case.camber,
        &case.lines,
        &flat(60.0),
        &BladeEdges::default(),
        true,
        1,
    )
    .unwrap_err();
    assert!(
        matches!(error, BladeSolidError::FacesCross { .. }),
        "{error:?}"
    );
}

#[test]
fn neighbours_that_touch_are_an_error() {
    let case = b1_case();
    let error = blade_solid(
        &case.camber,
        &case.lines,
        &flat(10.0),
        &BladeEdges::default(),
        true,
        40,
    )
    .unwrap_err();
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
    let error = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap_err();
    assert!(
        matches!(error, BladeSolidError::InclinedOutlet { line, .. } if line == last),
        "{error:?}"
    );
}

#[test]
fn lines_that_do_not_match_are_an_error() {
    let case = b1_case();
    let error = blade_solid(
        &case.camber,
        &case.lines[..3],
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap_err();
    assert_eq!(error, BladeSolidError::LineMismatch);

    let mut moved = case.lines.clone();
    moved[2].psi += 0.1;
    let error = blade_solid(
        &case.camber,
        &moved,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap_err();
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
    let solid = blade_solid(
        &camber,
        &trimmed,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
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

/// El `BladeSetup` de B1 con esa forma y esas líneas de corriente.
fn setup(shape: BladeShape, streamlines: u8) -> BladeSetup {
    BladeSetup {
        shape,
        streamlines,
        ..BladeSetup::default()
    }
}

#[test]
fn the_true_thickness_is_close_to_the_curve_and_never_above_it() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    let readings = &solid.readings;
    let last = solid.lines.len() - 1;
    for (index, real) in [
        (0, readings.true_thickness_hub),
        (last, readings.true_thickness_shroud),
    ] {
        let psi = solid.lines[index].psi;
        for (x, real) in READING_STATIONS.iter().zip(real) {
            let e = case.curves.at(psi, *x);
            assert!(
                real >= 0.7 * e && real <= e + 1.0e-9,
                "línea {index}, x = {x}: real {real} frente a e = {e}"
            );
        }
    }
}

#[test]
fn the_true_thickness_of_every_node_is_filled() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    for line in &solid.lines {
        assert_eq!(line.true_thickness.len(), CAMBER_STEPS + 1);
        assert!(
            line.true_thickness
                .iter()
                .all(|e| e.is_finite() && *e > 0.0)
        );
    }
}

#[test]
fn the_inlet_throat_is_shorter_than_the_pitch() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    let pitch = 2.0 * std::f64::consts::PI / f64::from(case.z());
    for (line, throat) in solid.lines.iter().zip(&solid.readings.throats) {
        let throat = throat.as_ref().expect("Z > 1 tiene garganta");
        let r_le = line.pressure[0].r;
        assert!(
            throat.width > 0.0 && throat.width < pitch * r_le,
            "ψ = {}: garganta {} frente al paso {}",
            line.psi,
            throat.width,
            pitch * r_le
        );
        close(throat.from.theta - line.pressure[0].theta, pitch, 1.0e-12);
    }
}

#[test]
fn the_throat_ends_on_the_suction_face() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    for (line, throat) in solid.lines.iter().zip(&solid.readings.throats) {
        let throat = throat.as_ref().unwrap();
        let suction: Vec<[f64; 3]> = line.suction.iter().map(cartesian).collect();
        let to = cartesian(&throat.to);
        assert!(distance_to_polyline(&suction, to) <= 1.0e-9);
        // El ancho es la distancia de `from` a `to`.
        close(distance(cartesian(&throat.from), to), throat.width, 1.0e-9);
    }
}

#[test]
fn one_blade_has_no_throat() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        1,
    )
    .unwrap();
    assert!(solid.readings.throats.iter().all(Option::is_none));
    assert_eq!(solid.readings.throats.len(), solid.lines.len());
}

#[test]
fn three_streamlines_build_and_read() {
    let case = case(BladeShape::Free3D, 3);
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    assert_eq!(solid.lines.len(), 3);
    let readings = &solid.readings;
    assert!(
        readings
            .true_thickness_hub
            .iter()
            .chain(&readings.true_thickness_shroud)
            .chain(&readings.exit_edges)
            .chain([&readings.exit_blockage])
            .all(|v| v.is_finite() && *v > 0.0)
    );
    assert_eq!(readings.throats.len(), 3);
}

#[test]
fn the_exit_edge_reading_is_e_over_sin_beta2() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    for (index, edge) in solid.readings.exit_edges.iter().enumerate() {
        let outlet = case.camber.lines[index].points.last().unwrap();
        let expected = case.curves.at(outlet_psi(&case, index), 1.0) / outlet.beta.sin();
        close(*edge, expected, 5.0e-5);
    }
}

fn outlet_psi(case: &Case, index: usize) -> f64 {
    case.camber.lines[index].psi
}

#[test]
fn the_exit_blockage_matches_the_1d() {
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    let mid = hm_design::blade_thickness::mid_line(&case.camber).unwrap();
    let outlet = mid.points.last().unwrap();
    let expected = hm_design::blade_blockage(
        case.z(),
        case.curves.at(mid.psi, 1.0),
        2.0 * outlet.r,
        outlet.beta,
    );
    let got = solid.readings.exit_blockage;
    assert!(
        ((got - expected) / expected).abs() < 1.0e-3,
        "τ2 = {got} frente a {expected}"
    );
}

#[test]
fn a_thin_station_raises_a_notice() {
    // Una curva plana de 3 mm, el mínimo, en una reglada: el espesor real
    // queda por debajo en alguna estación.
    let case = case(BladeShape::Ruled3D, 5);
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &flat(3.0),
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    assert!(!solid.notices.is_empty(), "sin avisos");
    for notice in &solid.notices {
        assert!(
            notice.message.contains("(0 es el cubo)"),
            "{}",
            notice.message
        );
        assert!(notice.message.contains("el espesor real en x = "));
        assert!(notice.message.contains("mínimo de fundición de "));
        assert!(!notice.message.contains('.'), "{}", notice.message);
        assert!(notice.line < solid.lines.len());
    }
    // Con la curva de Gülich de B1, que está lejos del mínimo, no hay ninguno.
    let thick = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    assert!(thick.notices.is_empty(), "{:?}", thick.notices);
}

#[test]
fn the_notice_names_the_line_the_station_and_the_minimum() {
    // Un nodo de x = 1 no baja de 1,5 mm: el mínimo se dice sin ceros de sobra.
    let case = b1_case();
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &flat(3.0),
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    let messages: Vec<&str> = solid.notices.iter().map(|n| n.message.as_str()).collect();
    assert!(
        messages
            .iter()
            .all(|m| m.ends_with("de 3 mm") || m.ends_with("de 1,5 mm")),
        "{messages:?}"
    );
}

#[test]
fn a_2d_shape_has_no_solid_and_says_why() {
    let case = b1_case();
    let error = blade_solid_for_design(
        &case.result,
        &case.impeller,
        &case.model,
        &setup(BladeShape::Free2D, 5),
        DEFAULT_INCIDENCE_DEG,
    )
    .unwrap_err();
    assert_eq!(
        error,
        BladeSolidError::NoCamber(
            "la forma del álabe es 2D: su espesor llega en la etapa 3".into()
        )
    );
}

#[test]
fn a_failed_camber_has_no_solid_and_says_why() {
    let case = b1_case();
    // Con estas manijas β baja de 5° en el cubo: no hay línea media.
    let bows = CamberBows {
        hub: (-30.0, -30.0),
        shroud: (-30.0, -30.0),
    };
    let wrong = BladeSetup {
        bows,
        ..setup(BladeShape::Free3D, 5)
    };
    // El mismo error que da la línea media, con su texto.
    let reason = hm_design::blade_camber::camber_for_design(
        &case.result,
        &case.impeller,
        &case.model,
        &wrong,
        DEFAULT_INCIDENCE_DEG,
    )
    .expect_err("estas manijas no dan línea media")
    .to_string();
    let error = blade_solid_for_design(
        &case.result,
        &case.impeller,
        &case.model,
        &wrong,
        DEFAULT_INCIDENCE_DEG,
    )
    .unwrap_err();
    let BladeSolidError::NoCamber(text) = error else {
        panic!("se esperaba NoCamber: {error:?}");
    };
    // El texto de la línea media, tal cual y sin el prefijo de `DesignError`.
    assert!(reason.ends_with(&text), "{reason} frente a {text}");
    assert!(
        text.starts_with("línea de corriente 0 (0 es el cubo): β = "),
        "{text}"
    );
}

#[test]
fn a_design_builds_the_same_solid_as_its_parts() {
    let case = b1_case();
    let for_design = blade_solid_for_design(
        &case.result,
        &case.impeller,
        &case.model,
        &setup(BladeShape::Free3D, 5),
        DEFAULT_INCIDENCE_DEG,
    )
    .unwrap();
    let by_parts = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        &BladeEdges::default(),
        true,
        case.z(),
    )
    .unwrap();
    assert_eq!(for_design, by_parts);
}

// ---------------------------------------------------------------------------
// La nariz de la entrada (etapa 2).
// ---------------------------------------------------------------------------

/// Los bordes con ese tipo y esas cotas, iguales en cubo y tapa.
fn edges(kind: LeadingKind, dims: LeadingDims) -> BladeEdges {
    BladeEdges {
        leading: kind,
        leading_hub: dims,
        leading_shroud: dims,
        ..BladeEdges::default()
    }
}

fn dims(lp: f64, tip: f64, asymmetry: f64) -> LeadingDims {
    LeadingDims { lp, tip, asymmetry }
}

/// El sólido de B1 con esos bordes y «cubo = tapa».
fn solid_with(case: &Case, edges: &BladeEdges) -> Result<BladeSolid, BladeSolidError> {
    blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        edges,
        true,
        case.z(),
    )
}

/// Largo de una polilínea 3D, m.
fn path_length(points: &[[f64; 3]]) -> f64 {
    points.windows(2).map(|p| distance(p[0], p[1])).sum()
}

/// La distancia de cada fila de la nariz a la punta, medida por la línea
/// media de la nariz (cuerdas entre filas), m.
fn nose_distances(line: &SolidLine) -> Vec<f64> {
    let middle: Vec<[f64; 3]> = line.nose.iter().map(|row| cartesian(&row[1])).collect();
    let mut out = vec![0.0];
    for pair in middle.windows(2) {
        out.push(out.last().unwrap() + distance(pair[0], pair[1]));
    }
    out
}

/// Largo de la línea media de la línea `index`, m: la polilínea de sus nodos.
fn camber_length(case: &Case, index: usize) -> f64 {
    let nodes: Vec<[f64; 3]> = case.camber.lines[index]
        .points
        .iter()
        .map(|p| [p.r * p.theta.cos(), p.r * p.theta.sin(), p.z])
        .collect();
    path_length(&nodes)
}

/// El espesor medido entre las dos caras de cada fila de la nariz frente al
/// `expected(u, e, índice de línea)`, con u = s/Lp, en las líneas dadas. El
/// error máximo, m.
fn nose_thickness_error(
    case: &Case,
    edges: &BladeEdges,
    same: bool,
    lines: &[usize],
    expected: impl Fn(f64, f64, &LeadingDims) -> f64,
) -> f64 {
    let solid = blade_solid(
        &case.camber,
        &case.lines,
        &case.curves,
        edges,
        same,
        case.z(),
    )
    .unwrap();
    let mut worst: f64 = 0.0;
    for &index in lines {
        let line = &solid.lines[index];
        assert_eq!(line.nose.len(), NOSE_STEPS + 1);
        let lp = edges.profile_at(line.psi, same).dims;
        let s = nose_distances(line);
        for (k, row) in line.nose.iter().enumerate().skip(1) {
            let e = case
                .curves
                .at(line.psi, x_at(&case.camber, index, row[1].m_conformal));
            let measured = distance(cartesian(&row[0]), cartesian(&row[2]));
            let want = expected(s[k] / lp.lp, e, &lp);
            worst = worst.max((measured - want).abs());
        }
    }
    worst
}

fn hub_mid_shroud(case: &Case) -> [usize; 3] {
    [0, case.camber.lines.len() / 2, case.camber.lines.len() - 1]
}

#[test]
fn blunt_leaves_the_solid_bit_identical() {
    for shape in [BladeShape::Free3D, BladeShape::Ruled3D] {
        let case = case(shape, 5);
        let reference = solid_with(&case, &BladeEdges::default()).unwrap();
        // Con romo ninguna cota importa, ni el interruptor.
        let noisy = edges(LeadingKind::Blunt, dims(0.05, 0.003, 1.0));
        assert_eq!(solid_with(&case, &noisy).unwrap(), reference, "{shape:?}");
        let split = blade_solid(
            &case.camber,
            &case.lines,
            &case.curves,
            &noisy,
            false,
            case.z(),
        )
        .unwrap();
        assert_eq!(split, reference, "{shape:?}");
        for line in &reference.lines {
            assert!(line.nose.is_empty());
            assert_eq!(line.leading_tip(), line.middle[0]);
            assert_eq!(line.pressure.len(), CAMBER_STEPS + 1);
        }
        assert!(
            reference
                .notices
                .iter()
                .all(|n| !n.message.contains("nariz"))
        );
    }
}

#[test]
fn the_nose_thickness_measured_between_faces_matches_the_profile() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let edges = BladeEdges {
        leading: LeadingKind::Ellipse,
        leading_hub: LeadingDims::default_for(LeadingKind::Ellipse, e),
        leading_shroud: LeadingDims::default_for(LeadingKind::Ellipse, case.curves.shroud.at(0.0)),
        ..BladeEdges::default()
    };
    // e/Lp = 0,2 en cubo, línea central y tapa.
    close(e / edges.leading_hub.lp, 0.2, 1e-12);
    let worst = nose_thickness_error(&case, &edges, false, &hub_mid_shroud(&case), |u, e, _| {
        e * (1.0 - (1.0 - u) * (1.0 - u)).max(0.0).sqrt()
    });
    assert!(worst <= 5.0e-5, "error máximo {} mm", worst * 1000.0);
}

#[test]
fn the_wedge_thickness_matches_the_profile() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let wedge = LeadingDims::default_for(LeadingKind::Wedge, e);
    let edges = edges(LeadingKind::Wedge, wedge);
    let worst = nose_thickness_error(&case, &edges, true, &hub_mid_shroud(&case), |u, e, d| {
        d.tip + (e - d.tip) * u
    });
    assert!(worst <= 5.0e-5, "error máximo {} mm", worst * 1000.0);
}

/// Una curva e(x) con pendiente en los primeros 25 mm: 8 mm en la entrada,
/// 5 mm en x = 0,25 y 3 mm en la salida.
fn sloped_curves() -> ThicknessCurves {
    let curve = || {
        ThicknessCurve::new(vec![
            ThicknessNode { x: 0.0, e: 0.008 },
            ThicknessNode { x: 0.25, e: 0.005 },
            ThicknessNode { x: 1.0, e: 0.003 },
        ])
        .unwrap()
    };
    ThicknessCurves {
        hub: curve(),
        shroud: curve(),
    }
}

#[test]
fn the_nose_follows_an_edited_thickness_curve() {
    let mut case = b1_case();
    case.curves = sloped_curves();
    let edges = edges(LeadingKind::Ellipse, dims(0.020, 0.0, 0.0));
    // t(s) = e(x(s))·f(u), con la e de la x del pie de cada fila.
    let worst = nose_thickness_error(&case, &edges, true, &hub_mid_shroud(&case), |u, e, _| {
        e * (1.0 - (1.0 - u) * (1.0 - u)).max(0.0).sqrt()
    });
    assert!(worst <= 5.0e-5, "error máximo {} mm", worst * 1000.0);
    // Y la curva sí cambia en la nariz: no es e(0)·f(u).
    let solid = solid_with(&case, &edges).unwrap();
    let line = &solid.lines[0];
    let row = &line.nose[NOSE_STEPS / 2];
    let measured = distance(cartesian(&row[0]), cartesian(&row[2]));
    let flat_guess = {
        let u = nose_distances(line)[NOSE_STEPS / 2] / 0.020;
        0.008 * (1.0 - (1.0 - u) * (1.0 - u)).sqrt()
    };
    assert!(
        (measured - flat_guess).abs() > 1.0e-4,
        "la nariz no sigue la curva: {measured} frente a {flat_guess}"
    );
}

/// Ángulo (grados) entre dos segmentos del plano conforme (m′, θ).
fn angle_between(a: [&SolidPoint; 2], b: [&SolidPoint; 2]) -> f64 {
    let va = [a[1].m_conformal - a[0].m_conformal, a[1].theta - a[0].theta];
    let vb = [b[1].m_conformal - b[0].m_conformal, b[1].theta - b[0].theta];
    let cos = (va[0] * vb[0] + va[1] * vb[1]) / (va[0].hypot(va[1]) * vb[0].hypot(vb[1]));
    cos.clamp(-1.0, 1.0).acos().to_degrees()
}

#[test]
fn the_ellipse_joins_the_body_tangentially() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let edges = edges(
        LeadingKind::Ellipse,
        LeadingDims::default_for(LeadingKind::Ellipse, e),
    );
    let solid = solid_with(&case, &edges).unwrap();
    for (index, line) in solid.lines.iter().enumerate() {
        let (before, joint) = (&line.nose[NOSE_STEPS - 1], &line.nose[NOSE_STEPS]);
        for (side, (nose, body)) in [
            (
                [&before[0], &joint[0]],
                [&line.pressure[0], &line.pressure[1]],
            ),
            (
                [&before[2], &joint[2]],
                [&line.suction[0], &line.suction[1]],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let angle = angle_between(nose, body);
            assert!(angle <= 0.5, "línea {index}, cara {side}: {angle}°");
        }
    }
}

#[test]
fn the_wedge_join_has_the_wedge_angle() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let wedge = LeadingDims::default_for(LeadingKind::Wedge, e);
    let solid = solid_with(&case, &edges(LeadingKind::Wedge, wedge)).unwrap();
    // Continuidad exacta: la cara de la nariz acaba donde empieza el cuerpo.
    // El ángulo: cada cara se abre atan((e − punta)/2/Lp) respecto a la línea
    // media.
    let expected = (((e - wedge.tip) / 2.0) / wedge.lp).atan().to_degrees();
    for (index, line) in solid.lines.iter().enumerate() {
        let joint = &line.nose[NOSE_STEPS];
        assert_eq!(joint[0], line.pressure[0]);
        assert_eq!(joint[2], line.suction[0]);
        let before = &line.nose[NOSE_STEPS - 1];
        let angle = angle_between(
            [&before[0], &joint[0]],
            [&line.pressure[0], &line.pressure[1]],
        );
        assert!(
            (angle - expected).abs() <= 0.3,
            "línea {index}: {angle}° frente a {expected}°"
        );
    }
}

#[test]
fn asymmetry_one_keeps_the_pressure_face_on_the_camber_at_the_tip() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    // Punta 0,1·e: la presión pasa por la línea media y la succión lleva
    // todo el espesor de la punta.
    let tip = 0.1 * e;
    let solid = solid_with(&case, &edges(LeadingKind::Wedge, dims(5.0 * e, tip, 1.0))).unwrap();
    for (index, line) in solid.lines.iter().enumerate() {
        let node = &case.camber.lines[index].points[0];
        let camber = [node.r * node.theta.cos(), node.r * node.theta.sin(), node.z];
        let row = &line.nose[0];
        assert!(
            distance(cartesian(&row[0]), camber) <= 5.0e-5,
            "línea {index}: la presión se aparta {} mm de la línea media",
            distance(cartesian(&row[0]), camber) * 1000.0
        );
        close(distance(cartesian(&row[2]), camber), tip, 5.0e-5);
        // Y vuelve a la simetría en la unión: el espesor completo.
        let joint = &line.nose[NOSE_STEPS];
        close(
            distance(cartesian(&joint[0]), cartesian(&joint[2])),
            case.curves.at(line.psi, 0.0),
            5.0e-5,
        );
    }
}

#[test]
fn the_nose_rows_end_on_the_first_station_of_the_faces() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    for kind in [LeadingKind::Ellipse, LeadingKind::Wedge] {
        let solid = solid_with(&case, &edges(kind, LeadingDims::default_for(kind, e))).unwrap();
        for line in &solid.lines {
            assert_eq!(line.nose.len(), NOSE_STEPS + 1);
            assert_eq!(
                *line.nose.last().unwrap(),
                [line.pressure[0], line.middle[0], line.suction[0]],
                "{kind:?}"
            );
            // Las caras siguen en J + 1 puntos y llegan a la salida.
            assert_eq!(line.pressure.len(), CAMBER_STEPS + 1);
            assert_eq!(line.suction.len(), CAMBER_STEPS + 1);
            assert_eq!(line.middle.len(), CAMBER_STEPS + 1);
        }
    }
}

#[test]
fn the_ellipse_chord_stays_within_the_fine_tolerance() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let lp = 5.0 * e;
    let solid = solid_with(&case, &edges(LeadingKind::Ellipse, dims(lp, 0.0, 0.0))).unwrap();
    for (index, line) in solid.lines.iter().enumerate() {
        // El polígono en (s, h): s por la línea media, h la mitad del espesor.
        let s = nose_distances(line);
        let polygon: Vec<[f64; 2]> = line
            .nose
            .iter()
            .zip(&s)
            .map(|(row, s)| [*s, 0.5 * distance(cartesian(&row[0]), cartesian(&row[2]))])
            .collect();
        let semi_axis = *s.last().unwrap();
        let mut worst: f64 = 0.0;
        for i in 0..=400 {
            let phi = std::f64::consts::FRAC_PI_2 * f64::from(i) / 400.0;
            let point = [semi_axis * (1.0 - phi.cos()), 0.5 * e * phi.sin()];
            let nearest = polygon
                .windows(2)
                .map(|pair| {
                    let d = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
                    let from = [point[0] - pair[0][0], point[1] - pair[0][1]];
                    let t = ((from[0] * d[0] + from[1] * d[1]) / (d[0] * d[0] + d[1] * d[1]))
                        .clamp(0.0, 1.0);
                    (from[0] - t * d[0]).hypot(from[1] - t * d[1])
                })
                .fold(f64::MAX, f64::min);
            worst = worst.max(nearest);
        }
        assert!(
            worst <= 5.0e-5,
            "línea {index}: la cuerda se aparta {} mm de la elipse",
            worst * 1000.0
        );
    }
}

#[test]
fn an_upstream_suction_face_still_gives_finite_points() {
    // La punta con asimetría 1: la cara de succión y la de presión se acercan
    // a la prolongación aguas arriba del borde de ataque.
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    for tip in [0.0, 0.1 * e, e] {
        let solid = solid_with(&case, &edges(LeadingKind::Wedge, dims(5.0 * e, tip, 1.0))).unwrap();
        for line in &solid.lines {
            let all = line
                .nose
                .iter()
                .flatten()
                .chain(&line.pressure)
                .chain(&line.middle)
                .chain(&line.suction);
            for point in all {
                assert!(
                    [point.z, point.r, point.theta, point.m_conformal]
                        .iter()
                        .all(|v| v.is_finite()),
                    "tip {tip}: {point:?}"
                );
            }
        }
    }
}

#[test]
fn lp_outside_the_limits_is_clamped_and_noticed() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let length = camber_length(&case, 0);
    // Lp = 0,5·L → 0,3·L.
    let solid = solid_with(
        &case,
        &edges(LeadingKind::Ellipse, dims(0.5 * length, 0.0, 0.0)),
    )
    .unwrap();
    let nose = nose_distances(&solid.lines[0]);
    close(*nose.last().unwrap(), 0.3 * length, 0.02 * length);
    let notice = solid
        .notices
        .iter()
        .find(|n| n.line == 0 && n.message.contains("se acotó"))
        .expect("el aviso de Lp acotada");
    assert!(
        notice.message.contains("0 es el cubo"),
        "{}",
        notice.message
    );
    assert!(notice.message.contains("Lp"), "{}", notice.message);
    // Lp = 0,001·e → e/2 (el semicírculo).
    let solid = solid_with(
        &case,
        &edges(LeadingKind::Ellipse, dims(0.001 * e, 0.0, 0.0)),
    )
    .unwrap();
    let nose = nose_distances(&solid.lines[0]);
    close(*nose.last().unwrap(), e / 2.0, 0.05 * e);
    assert!(
        solid
            .notices
            .iter()
            .any(|n| n.line == 0 && n.message.contains("se acotó")),
        "{:?}",
        solid.notices
    );
    // Una punta y una asimetría fuera de rango también se dicen.
    let solid = solid_with(
        &case,
        &edges(LeadingKind::Wedge, dims(5.0 * e, 2.0 * e, 1.5)),
    )
    .unwrap();
    let messages: Vec<&str> = solid.notices.iter().map(|n| n.message.as_str()).collect();
    assert!(messages.iter().any(|m| m.contains("punta")), "{messages:?}");
    assert!(
        messages.iter().any(|m| m.contains("asimetría")),
        "{messages:?}"
    );
}

#[test]
fn a_nose_that_cannot_fit_is_an_error() {
    let case = b1_case();
    let error = solid_with(
        &case,
        &edges(LeadingKind::Ellipse, dims(f64::NAN, 0.0, 0.0)),
    )
    .unwrap_err();
    let BladeSolidError::NoseOutOfRange { line, .. } = error else {
        panic!("{error:?}");
    };
    assert_eq!(line, 0);
    assert!(error.to_string().contains("0 es el cubo"), "{error}");
}

#[test]
fn a_long_nose_raises_a_notice() {
    let case = b1_case();
    let length = camber_length(&case, 0);
    let solid = solid_with(
        &case,
        &edges(LeadingKind::Ellipse, dims(0.25 * length, 0.0, 0.0)),
    )
    .unwrap();
    let notice = solid
        .notices
        .iter()
        .find(|n| n.line == 0 && n.message.contains("0,2·L"))
        .expect("el aviso de la nariz larga");
    assert!(
        notice.message.contains("0 es el cubo"),
        "{}",
        notice.message
    );
    assert!(
        solid
            .notices
            .iter()
            .all(|n| !n.message.contains("se acotó")),
        "{:?}",
        solid.notices
    );
    // Con la nariz por defecto (e/Lp = 0,2) no hay aviso.
    let e = case.curves.hub.at(0.0);
    let solid = solid_with(
        &case,
        &edges(
            LeadingKind::Ellipse,
            LeadingDims::default_for(LeadingKind::Ellipse, e),
        ),
    )
    .unwrap();
    assert!(
        solid.notices.iter().all(|n| !n.message.contains("nariz")),
        "{:?}",
        solid.notices
    );
}

#[test]
fn the_throat_starts_at_the_tip_of_the_neighbour() {
    let case = b1_case();
    let e = case.curves.hub.at(0.0);
    let pitch = 2.0 * std::f64::consts::PI / f64::from(case.z());
    let solid = solid_with(
        &case,
        &edges(
            LeadingKind::Wedge,
            LeadingDims::default_for(LeadingKind::Wedge, e),
        ),
    )
    .unwrap();
    for (line, throat) in solid.lines.iter().zip(&solid.readings.throats) {
        let throat = throat.as_ref().unwrap();
        let tip = line.leading_tip();
        assert_eq!(tip, line.nose[0][1]);
        close(throat.from.theta - tip.theta, pitch, 1.0e-12);
        close(throat.from.r, tip.r, 1.0e-12);
        close(throat.from.m_conformal, tip.m_conformal, 1.0e-12);
        // La garganta acaba en la cara de succión entera (nariz incluida).
        let suction: Vec<[f64; 3]> = line
            .nose
            .iter()
            .map(|row| cartesian(&row[2]))
            .chain(line.suction.iter().map(cartesian))
            .collect();
        assert!(distance_to_polyline(&suction, cartesian(&throat.to)) <= 1.0e-9);
        close(
            distance(cartesian(&throat.from), cartesian(&throat.to)),
            throat.width,
            1.0e-9,
        );
        assert!(throat.width > 0.0);
    }
}

#[test]
fn an_ellipse_nose_fits_where_the_blunt_edge_crosses() {
    // Con 60 mm de espesor y Z = 1 la entrada romo cruza las caras en el
    // codo de la línea media (línea 0, m′ = 0). La elipse afina justo ahí y ya
    // no cruza: es un álabe válido, no un error callado.
    let case = b1_case();
    assert!(matches!(
        blade_solid(
            &case.camber,
            &case.lines,
            &flat(60.0),
            &BladeEdges::default(),
            true,
            1
        ),
        Err(BladeSolidError::FacesCross { line: 0, .. })
    ));
    let ellipse = edges(
        LeadingKind::Ellipse,
        LeadingDims::default_for(LeadingKind::Ellipse, 0.060),
    );
    assert!(blade_solid(&case.camber, &case.lines, &flat(60.0), &ellipse, true, 1).is_ok());
}

#[test]
fn the_faces_cross_error_still_fires_near_the_nose() {
    // Una horquilla de la línea media (θ vuelve hacia atrás) en el nodo 5, a
    // unos 11 mm de la punta y dentro de una nariz de 60 mm: las caras de la
    // nariz se cruzan, y el error dice la línea de corriente.
    let mut case = b1_case();
    let pivot = case.camber.lines[0].points[5].theta;
    for point in &mut case.camber.lines[0].points[6..] {
        point.theta = pivot - (point.theta - pivot);
    }
    let error = solid_with(&case, &edges(LeadingKind::Ellipse, dims(0.060, 0.0, 0.0))).unwrap_err();
    assert!(
        matches!(error, BladeSolidError::FacesCross { line: 0, .. }),
        "{error:?}"
    );
    assert!(error.to_string().contains("0 es el cubo"), "{error}");
}

#[test]
fn the_design_chain_passes_the_edges_and_the_switch() {
    let (result, impeller, model, _, _) = b1(5);
    let blunt = setup(BladeShape::Free3D, 5);
    let ellipse = BladeSetup {
        edges: BladeEdges::default().with_leading_kind(LeadingKind::Ellipse, 0.006, 0.006),
        ..blunt.clone()
    };
    let from = |setup: &BladeSetup| {
        blade_solid_for_design(&result, &impeller, &model, setup, DEFAULT_INCIDENCE_DEG).unwrap()
    };
    assert!(from(&blunt).lines.iter().all(|l| l.nose.is_empty()));
    assert!(
        from(&ellipse)
            .lines
            .iter()
            .all(|l| l.nose.len() == NOSE_STEPS + 1)
    );
}
