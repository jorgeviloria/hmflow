//! Línea media del álabe sobre cada línea de corriente (punto 2 del álabe):
//! anclaje con el álabe 2D de hoy, formas cerradas con solución analítica y el
//! caso B1.

use hm_design::blade_camber::{BladeCamber, CAMBER_STEPS, CamberError, CamberOrigin, blade_camber};
use hm_design::blade_setup::{BladeShape, CamberBows};
use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, camber_line_shaped, geometry};
use hm_design::inlet_triangles::{Blade2dAngle, InletTriangle, inlet_triangles};
use hm_design::meridian::meridional_model;
use hm_design::streamlines::{Streamline, streamlines};
use hm_design::{DesignInput, DesignResult, default_mechanical_efficiency, design};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

fn close_rel(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        ((actual - expected) / expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, error relativo {} > {tolerance}",
        ((actual - expected) / expected).abs()
    );
}

/// Una línea de corriente sintética: `points_mm` en mm y el corte con el borde
/// de ataque a `m_le_mm` de longitud desde su primer punto.
fn line(psi: f64, points_mm: &[[f64; 2]], m_le_mm: f64) -> Streamline {
    let mut walked = 0.0;
    let mut leading_edge = points_mm[0];
    for pair in points_mm.windows(2) {
        let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        if walked + length >= m_le_mm {
            let t = (m_le_mm - walked) / length;
            leading_edge = [
                pair[0][0] + t * (pair[1][0] - pair[0][0]),
                pair[0][1] + t * (pair[1][1] - pair[0][1]),
            ];
            break;
        }
        walked += length;
        leading_edge = pair[1];
    }
    Streamline {
        psi,
        points: points_mm.to_vec(),
        leading_edge,
        m_leading_edge: m_le_mm,
        leading_edge_area: 0.0,
    }
}

/// Línea radial de `r1` a `r2` (m), sin tramo antes del corte.
fn radial(psi: f64, r1: f64, r2: f64) -> Streamline {
    line(psi, &[[0.0, r1 * 1000.0], [0.0, r2 * 1000.0]], 0.0)
}

/// Línea axial de longitud `length` (m) a radio `r` (m).
fn axial(psi: f64, r: f64, length: f64) -> Streamline {
    line(
        psi,
        &[[0.0, r * 1000.0], [length * 1000.0, r * 1000.0]],
        0.0,
    )
}

/// Cono de `r1` a `r2` (m) con la línea a `gamma` (rad) del eje: sen γ = dr/dm.
fn cone(psi: f64, r1: f64, r2: f64, gamma: f64) -> Streamline {
    let length = (r2 - r1) / gamma.sin();
    line(
        psi,
        &[
            [0.0, r1 * 1000.0],
            [length * gamma.cos() * 1000.0, r2 * 1000.0],
        ],
        0.0,
    )
}

/// Un triángulo de entrada con ese β1B y el resto a 1.0.
fn triangle(psi: f64, beta1b: f64) -> InletTriangle {
    InletTriangle {
        psi,
        r1: 1.0,
        u1: 1.0,
        c1m: 1.0,
        tau1: 1.0,
        beta1_flow: 1.0,
        beta1b,
        beta_2d: Blade2dAngle::NoBlade,
    }
}

const NO_BOWS: CamberBows = CamberBows {
    hub: (0.0, 0.0),
    shroud: (0.0, 0.0),
};

fn free(
    lines: &[Streamline],
    betas1: &[f64],
    beta2b: f64,
    bows: &CamberBows,
) -> Result<BladeCamber, CamberError> {
    let triangles: Vec<InletTriangle> = lines
        .iter()
        .zip(betas1)
        .map(|(line, beta)| triangle(line.psi, *beta))
        .collect();
    blade_camber(BladeShape::Free3D, lines, &triangles, beta2b, bows)
}

/// El punto de máximo rendimiento de B1, como en `tests/case_b1.rs`.
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

/// B1 con `count` líneas de corriente: resultado, líneas y triángulos.
fn b1(count: u8) -> (DesignResult, Vec<Streamline>, Vec<InletTriangle>) {
    let input = b1_input();
    let result = design(&input).unwrap();
    let impeller = geometry(&input, &result).unwrap().impeller;
    let model = meridional_model(&result).unwrap();
    let lines = streamlines(&model, count).unwrap();
    let triangles = inlet_triangles(&result, &impeller, &lines, DEFAULT_INCIDENCE_DEG).unwrap();
    (result, lines, triangles)
}

fn b1_camber(count: u8, bows: &CamberBows) -> (Vec<Streamline>, Vec<InletTriangle>, BladeCamber) {
    let (result, lines, triangles) = b1(count);
    let beta2b = result.selection.beta2_deg.to_radians();
    let camber = blade_camber(BladeShape::Free3D, &lines, &triangles, beta2b, bows).unwrap();
    (lines, triangles, camber)
}

#[test]
fn a_radial_line_is_the_2d_blade_of_today() {
    let (r1, r2) = (0.08, 0.21);
    let (b1, b2) = (25_f64.to_radians(), 16_f64.to_radians());
    for bow in [(0.0, 0.0), (3.0, -2.0)] {
        let flat = camber_line_shaped((r1, r2), (b1, b2), (1.0, 1.0), 1.0, bow, CAMBER_STEPS);
        let camber = blade_camber(
            BladeShape::Free3D,
            &[radial(0.0, r1, r2), radial(1.0, r1, r2)],
            &[triangle(0.0, b1), triangle(1.0, b1)],
            b2,
            &CamberBows {
                hub: bow,
                shroud: bow,
            },
        )
        .unwrap();
        for (point, old) in camber.lines[0].points.iter().zip(&flat) {
            close(point.theta, old.theta, 1e-12);
            close(point.beta, old.beta, 1e-12);
        }
    }
}

#[test]
fn a_log_spiral_wraps_ln_ratio_over_tan_beta() {
    let (r1, r2) = (0.1, 0.2);
    let beta = 20_f64.to_radians();
    let camber = free(
        &[radial(0.0, r1, r2), radial(1.0, r1, r2)],
        &[beta, beta],
        beta,
        &NO_BOWS,
    )
    .unwrap();
    close_rel(camber.lines[0].wrap, (r2 / r1).ln() / beta.tan(), 1e-5);
}

#[test]
fn an_axial_line_is_a_helix() {
    let (r, length) = (0.1, 0.05);
    let beta = 20_f64.to_radians();
    let camber = free(
        &[axial(0.0, r, length), axial(1.0, r, length)],
        &[beta, beta],
        beta,
        &NO_BOWS,
    )
    .unwrap();
    let helix = &camber.lines[0];
    close(helix.wrap, length / (r * beta.tan()), 1e-12);
    close(helix.length, length.hypot(r * helix.wrap), 1e-12);
    for point in &helix.points {
        close(point.r, r, 1e-15);
    }
}

#[test]
fn an_axial_line_with_linear_beta_has_its_closed_form() {
    let (r, length) = (0.1, 0.05);
    let (b1, b2) = (25_f64.to_radians(), 16_f64.to_radians());
    let camber = free(
        &[axial(0.0, r, length), axial(1.0, r, length)],
        &[b1, b1],
        b2,
        &NO_BOWS,
    )
    .unwrap();
    let expected = length * (b2.sin() / b1.sin()).ln() / (r * (b2 - b1));
    close_rel(camber.lines[0].wrap, expected, 1e-5);
}

#[test]
fn a_cone_scales_the_wrap_by_sin_gamma() {
    let (r1, r2) = (0.08, 0.2);
    let gamma = 40_f64.to_radians();
    let beta = 20_f64.to_radians();
    let camber = free(
        &[cone(0.0, r1, r2, gamma), cone(1.0, r1, r2, gamma)],
        &[beta, beta],
        beta,
        &NO_BOWS,
    )
    .unwrap();
    let cone_line = &camber.lines[0];
    close_rel(
        cone_line.wrap,
        (r2 / r1).ln() / (gamma.sin() * beta.tan()),
        1e-5,
    );
    close_rel(
        cone_line.points.last().unwrap().m_conformal,
        (r2 / r1).ln() / gamma.sin(),
        1e-5,
    );
}

#[test]
fn the_conformal_slope_is_tan_beta() {
    let (r1, r2) = (0.08, 0.2);
    let gamma = 40_f64.to_radians();
    let (b1, b2) = (25_f64.to_radians(), 16_f64.to_radians());
    let camber = free(
        &[cone(0.0, r1, r2, gamma), cone(1.0, r1, r2, gamma)],
        &[b1, b1],
        b2,
        &NO_BOWS,
    )
    .unwrap();
    let points = &camber.lines[0].points;
    for j in 1..points.len() - 1 {
        let slope = (points[j + 1].m_conformal - points[j - 1].m_conformal)
            / (points[j + 1].theta - points[j - 1].theta);
        close_rel(slope, points[j].beta.tan(), 1e-3);
    }
}

#[test]
fn all_lines_end_at_the_same_angle_and_the_most_forward_edge_is_zero() {
    let (r1, r2) = (0.08, 0.21);
    let b2 = 16_f64.to_radians();
    let camber = free(
        &[radial(0.0, r1, r2), radial(1.0, r1, r2)],
        &[25_f64.to_radians(), 20_f64.to_radians()],
        b2,
        &NO_BOWS,
    )
    .unwrap();
    assert!(camber.lines[0].wrap != camber.lines[1].wrap);
    for line in &camber.lines {
        close(line.points.last().unwrap().theta, camber.wrap_max, 1e-12);
        close(line.points[0].theta, camber.wrap_max - line.wrap, 1e-12);
    }
    let first = camber
        .lines
        .iter()
        .map(|line| line.points[0].theta)
        .fold(f64::INFINITY, f64::min);
    close(first, 0.0, 1e-15);
}

#[test]
fn the_handles_interpolate_between_hub_and_shroud() {
    let (r1, r2) = (0.08, 0.21);
    let (b1, b2) = (25_f64.to_radians(), 16_f64.to_radians());
    let lines = [
        radial(0.0, r1, r2),
        radial(0.5, r1, r2),
        radial(1.0, r1, r2),
    ];
    let betas = [b1, b1, b1];
    let straight = free(&lines, &betas, b2, &NO_BOWS).unwrap();
    let hub_only = free(
        &lines,
        &betas,
        b2,
        &CamberBows {
            hub: (4.0, 0.0),
            shroud: (0.0, 0.0),
        },
    )
    .unwrap();
    let both = free(
        &lines,
        &betas,
        b2,
        &CamberBows {
            hub: (2.0, 0.0),
            shroud: (2.0, 0.0),
        },
    )
    .unwrap();

    // La línea del medio usa la media de cubo y tapa.
    close(hub_only.lines[1].wrap, both.lines[1].wrap, 1e-12);
    for (a, b) in hub_only.lines[1].points.iter().zip(&both.lines[1].points) {
        close(a.beta, b.beta, 1e-12);
    }
    // Mover solo el cubo no cambia la tapa.
    close(hub_only.lines[2].wrap, straight.lines[2].wrap, 1e-15);
    for (a, b) in hub_only.lines[2]
        .points
        .iter()
        .zip(&straight.lines[2].points)
    {
        close(a.beta, b.beta, 1e-15);
    }
    // Una desviación positiva sube β y baja el envolvimiento.
    assert!(hub_only.lines[0].wrap < straight.lines[0].wrap);
}

#[test]
fn every_line_has_the_grid_the_solid_needs() {
    let (r1, r2) = (0.08, 0.21);
    let b = 20_f64.to_radians();
    let camber = free(
        &[radial(0.0, r1, r2), radial(1.0, r1, r2)],
        &[b, b],
        b,
        &NO_BOWS,
    )
    .unwrap();
    for line in &camber.lines {
        assert_eq!(line.points.len(), CAMBER_STEPS + 1);
        assert_eq!(line.origin, CamberOrigin::Integrated);
        assert_eq!(line.edge_gap, 0.0);
        for (j, point) in line.points.iter().enumerate() {
            close(point.x, j as f64 / CAMBER_STEPS as f64, 1e-15);
        }
        assert_eq!(line.points[0].m, 0.0);
        assert!(line.points.windows(2).all(|pair| pair[1].m > pair[0].m));
    }
}

#[test]
fn b1_starts_at_its_cuts_and_meets_the_flow() {
    let (lines, triangles, camber) = b1_camber(5, &NO_BOWS);
    assert_eq!(camber.lines.len(), 5);
    for ((line, triangle), camber_line) in lines.iter().zip(&triangles).zip(&camber.lines) {
        let first = &camber_line.points[0];
        // El corte de cubo y tapa es el punto del modelo, que cae a menos de
        // 0,1 µm de la polilínea de estaciones; el nodo 0 está sobre ella.
        close(first.z, line.leading_edge[0] / 1000.0, 1e-7);
        close(first.r, line.leading_edge[1] / 1000.0, 1e-7);
        let last = camber_line.points.last().unwrap();
        let end = line.points.last().unwrap();
        close(last.z, end[0] / 1000.0, 1e-12);
        close(last.r, end[1] / 1000.0, 1e-12);
        close(first.beta, triangle.beta1b, 1e-12);
    }
    close(camber.lines[4].points[0].beta.to_degrees(), 20.34, 0.01);
}

#[test]
fn b1_wraps_what_the_probe_measured() {
    let (_, _, camber) = b1_camber(5, &NO_BOWS);
    let wraps: Vec<f64> = camber.lines.iter().map(|l| l.wrap.to_degrees()).collect();
    println!("envolvimientos [°]: {wraps:?}");
    println!(
        "θ borde [°]: {:?}",
        camber
            .lines
            .iter()
            .map(|l| l.points[0].theta.to_degrees())
            .collect::<Vec<_>>()
    );
    for (actual, expected) in wraps.iter().zip([181.5, 188.5, 195.1, 198.6, 198.6]) {
        close(*actual, expected, 0.05);
    }
    close(camber.lines[0].points[0].theta.to_degrees(), 17.1, 0.1);
}

#[test]
fn hub_and_shroud_do_not_depend_on_the_line_count() {
    let (_, _, reference) = b1_camber(5, &NO_BOWS);
    for count in [3, 7, 9] {
        let (_, _, other) = b1_camber(count, &NO_BOWS);
        for (mine, theirs) in [
            (
                reference.lines.first().unwrap(),
                other.lines.first().unwrap(),
            ),
            (reference.lines.last().unwrap(), other.lines.last().unwrap()),
        ] {
            assert_eq!(mine.points.len(), theirs.points.len());
            // Todo lo propio de la línea: ε, forma, β, z y r. El desplazamiento
            // común Θ = máx ε sí depende de N, porque el máximo cae en una
            // intermedia (ψ = 0,75 con 5 líneas); por eso θ se compara contra
            // la salida de la propia línea.
            close(mine.wrap, theirs.wrap, 1e-12);
            close(mine.length, theirs.length, 1e-12);
            let (end_mine, end_theirs) = (
                mine.points.last().unwrap().theta,
                theirs.points.last().unwrap().theta,
            );
            for (a, b) in mine.points.iter().zip(&theirs.points) {
                close(a.theta - end_mine, b.theta - end_theirs, 1e-12);
                close(a.beta, b.beta, 1e-12);
                close(a.z, b.z, 1e-12);
                close(a.r, b.r, 1e-12);
            }
        }
    }
}

#[test]
fn errors_name_the_line() {
    let (r1, r2) = (0.08, 0.21);
    let b = 20_f64.to_radians();
    let two = [radial(0.0, r1, r2), radial(1.0, r1, r2)];

    // Una forma que no es 3D.
    let err = blade_camber(
        BladeShape::Free2D,
        &two,
        &[triangle(0.0, b), triangle(1.0, b)],
        b,
        &NO_BOWS,
    )
    .unwrap_err();
    assert_eq!(err, CamberError::NotThreeD);

    // Una sola línea, o distinto número de líneas y de triángulos.
    let err = free(&two[..1], &[b], b, &NO_BOWS).unwrap_err();
    assert_eq!(err, CamberError::LineMismatch);
    let err = blade_camber(
        BladeShape::Free3D,
        &two,
        &[triangle(0.0, b), triangle(1.0, b), triangle(1.0, b)],
        b,
        &NO_BOWS,
    )
    .unwrap_err();
    assert_eq!(err, CamberError::LineMismatch);

    // El corte cae en el último punto de la línea.
    let at_the_end = line(1.0, &[[0.0, 80.0], [0.0, 210.0]], 130.0);
    let err = free(&[two[0].clone(), at_the_end], &[b, b], b, &NO_BOWS).unwrap_err();
    assert_eq!(err, CamberError::NoLength { line: 1 });

    // Una línea que pasa por el eje.
    let on_axis = line(1.0, &[[0.0, 0.0], [0.0, 210.0]], 0.0);
    let err = free(&[two[0].clone(), on_axis], &[b, b], b, &NO_BOWS).unwrap_err();
    assert_eq!(err, CamberError::OnAxis { line: 1 });

    // Una desviación que saca β de 5° a 90°.
    let deep = CamberBows {
        hub: (-30.0, -30.0),
        shroud: (-30.0, -30.0),
    };
    let err = free(
        &two,
        &[12_f64.to_radians(), 12_f64.to_radians()],
        10_f64.to_radians(),
        &deep,
    )
    .unwrap_err();
    assert!(
        matches!(err, CamberError::AngleOutOfRange { line: 0, .. }),
        "{err:?}"
    );

    // β1B de 95°, sin manijas.
    let err = free(&two, &[95_f64.to_radians(), b], b, &NO_BOWS).unwrap_err();
    assert!(
        matches!(err, CamberError::AngleOutOfRange { line: 0, x, .. } if x == 0.0),
        "{err:?}"
    );
    assert!(err.to_string().contains("95"), "{err}");
}
