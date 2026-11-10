//! Triángulo de entrada por línea de corriente: u1 = ω·r del corte con el
//! borde de ataque, c1m común y β1B = β′1 + i1 con el bloqueo iterado
//! (Gülich 2020, tabla 3.1 y §7.2.1, p. 411), y el ángulo del álabe 2D de hoy
//! visto sobre cada línea.

use std::f64::consts::PI;

use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, ImpellerGeometry, geometry};
use hm_design::inlet_triangles::{AXIAL_BETA_2D_DEG, Blade2dAngle, InletTriangle, inlet_triangles};
use hm_design::meridian::{
    EdgeMode, MeridianEdits, MeridionalModel, meridional_model, meridional_model_with,
};
use hm_design::streamlines::{Streamline, streamlines};
use hm_design::{DesignInput, DesignResult, default_mechanical_efficiency, design};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

/// El mismo caso de partida que `tests/meridian.rs`.
fn input() -> DesignInput {
    let q = |text: &str| parse_quantity(text).unwrap();
    DesignInput {
        q: q("120 m3/h"),
        h: q("45 m"),
        n: q("2900 rpm"),
        rho: q("998.2 kg/m3"),
        eta_m: 0.95,
        npsha: None,
        nu: None,
    }
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

struct Case {
    result: DesignResult,
    impeller: ImpellerGeometry,
    model: MeridionalModel,
}

fn case_of(input: &DesignInput) -> Case {
    let result = design(input).unwrap();
    let impeller = geometry(input, &result).unwrap().impeller;
    let model = meridional_model(&result).unwrap();
    Case {
        result,
        impeller,
        model,
    }
}

impl Case {
    fn triangles(&self, lines: &[Streamline], incidence_deg: f64) -> Vec<InletTriangle> {
        inlet_triangles(&self.result, &self.impeller, lines, incidence_deg).unwrap()
    }

    /// d1/2, en mm.
    fn r1_mm(&self) -> f64 {
        self.result.d1.si * 1000.0 / 2.0
    }
}

/// Sección de paso de las líneas sintéticas, mm².
const SYNTHETIC_AREA: f64 = 20_000.0;

/// Una línea de dos puntos, en mm, que corta el borde en `cut`.
fn synthetic(points: [[f64; 2]; 2], cut: [f64; 2]) -> Streamline {
    Streamline {
        leading_edge_area: SYNTHETIC_AREA,
        psi: 0.5,
        points: points.to_vec(),
        leading_edge: cut,
        m_leading_edge: (cut[0] - points[0][0]).hypot(cut[1] - points[0][1]),
    }
}

#[test]
fn the_shroud_line_matches_the_1d_with_a_radial_edge_in_the_eye() {
    let input = input();
    let result = design(&input).unwrap();
    let impeller = geometry(&input, &result).unwrap().impeller;
    let edits = MeridianEdits {
        edge_shroud: Some(0.0),
        edge_hub: Some(0.0),
        edge_mode: EdgeMode::Line,
        ..Default::default()
    };
    let model = meridional_model_with(&result, &edits).unwrap();
    let lines = streamlines(&model, 5).unwrap();
    let triangles = inlet_triangles(&result, &impeller, &lines, DEFAULT_INCIDENCE_DEG).unwrap();
    let shroud = triangles.last().unwrap();

    // Precondiciones del spec: el borde radial en el ojo da la sección y el
    // radio del 1D, y con ellos el c1m del 1D, en todas las líneas.
    let d1 = result.d1.si * 1000.0;
    let dh = model.params.d1i;
    let annulus = PI * (d1 * d1 - dh * dh) / 4.0;
    close(model.params.edge_area / annulus, 1.0, 1.0e-9);
    close(shroud.r1, result.d1.si / 2.0, 1.0e-9);
    for (triangle, line) in triangles.iter().zip(&lines) {
        close(line.leading_edge_area / annulus, 1.0, 1.0e-9);
        close(triangle.c1m / result.selection.cm0, 1.0, 1.0e-9);
    }

    close(shroud.psi, 1.0, 0.0);
    close(shroud.beta1b, result.beta1b, 1.0e-6);
}

/// Con c1m común, β1B baja cuando sube el radio del corte. En el meridiano por
/// defecto de B1 los radios no crecen con ψ (la línea 1 corta por debajo del
/// cubo), así que el orden se comprueba con r1 y no con ψ.
#[test]
fn beta1b_falls_as_the_cut_radius_grows_in_b1() {
    let case = case_of(&b1_input());
    let lines = streamlines(&case.model, 5).unwrap();
    let mut triangles = case.triangles(&lines, DEFAULT_INCIDENCE_DEG);
    assert_eq!(triangles.len(), 5);
    for (triangle, line) in triangles.iter().zip(&lines) {
        close(triangle.psi, line.psi, 0.0);
        close(triangle.r1, line.leading_edge[1] / 1000.0, 1.0e-15);
    }
    triangles.sort_by(|a, b| a.r1.total_cmp(&b.r1));
    for pair in triangles.windows(2) {
        assert!(pair[0].r1 < pair[1].r1, "radios repetidos: {pair:?}");
        assert!(
            pair[0].beta1b > pair[1].beta1b,
            "β1B no baja al subir r1: {pair:?}"
        );
    }
}

/// c1m es el caudal del rodete entre la sección de paso de la estación de
/// cada corte (Gülich, ec. T3.1.2), no entre la superficie del borde.
#[test]
fn c1m_is_the_flow_over_the_section_at_each_cut() {
    let case = case_of(&b1_input());
    let lines = streamlines(&case.model, 7).unwrap();
    let triangles = case.triangles(&lines, DEFAULT_INCIDENCE_DEG);
    let omega = 2.0 * case.result.u1.si / case.result.d1.si;
    for (triangle, line) in triangles.iter().zip(&lines) {
        close(
            triangle.c1m,
            case.result.qth.si / (line.leading_edge_area * 1.0e-6),
            1.0e-12,
        );
        close(triangle.u1, omega * triangle.r1, 1.0e-12);
        assert!(triangle.tau1 >= 1.0, "τ1 = {}", triangle.tau1);
    }
}

/// En B1 el corte de la tapa cae en el plano de entrada, donde la sección es
/// la corona del ojo: su c1m es el cm0 del 1D aunque el borde esté inclinado.
/// Los demás cortes quedan cerca: cm casi no cambia en la curva del canal.
#[test]
fn the_shroud_cut_of_b1_sees_the_cm0_of_the_1d() {
    let case = case_of(&b1_input());
    let lines = streamlines(&case.model, 5).unwrap();
    let triangles = case.triangles(&lines, DEFAULT_INCIDENCE_DEG);
    let cm0 = case.result.selection.cm0;
    close(triangles[4].c1m / cm0, 1.0, 1.0e-9);
    for triangle in &triangles {
        close(triangle.c1m / cm0, 1.0, 0.05);
    }
    // Con la superficie del borde, c1m saldría un 19 % bajo.
    let over_the_edge = case.result.qth.si / (case.model.params.edge_area * 1.0e-6);
    assert!(over_the_edge < 0.85 * cm0, "{over_the_edge}");
}

#[test]
fn a_line_without_a_section_is_an_error() {
    let case = case_of(&b1_input());
    let r = case.r1_mm();
    let mut line = synthetic([[10.0, r - 5.0], [10.0, r + 5.0]], [10.0, r]);
    line.leading_edge_area = 0.0;
    let error =
        inlet_triangles(&case.result, &case.impeller, &[line], DEFAULT_INCIDENCE_DEG).unwrap_err();
    assert!(error.to_string().contains("sección de paso"), "{error}");
}

#[test]
fn the_flow_angle_is_beta1b_minus_the_incidence() {
    let case = case_of(&b1_input());
    let lines = streamlines(&case.model, 5).unwrap();
    let incidence_deg = 3.0_f64;
    for triangle in case.triangles(&lines, incidence_deg) {
        close(
            triangle.beta1_flow,
            triangle.beta1b - incidence_deg.to_radians(),
            1.0e-12,
        );
        // β′1 = arctan(τ1·c1m / u1), sin prerrotación.
        close(
            triangle.beta1_flow,
            (triangle.tau1 * triangle.c1m / triangle.u1).atan(),
            1.0e-9,
        );
    }
}

#[test]
fn the_2d_angle_on_a_radial_segment_is_the_plan_angle() {
    let case = case_of(&b1_input());
    let (inner, outer) = (&case.impeller.blade[3], &case.impeller.blade[4]);
    let r = (inner.r + outer.r) / 2.0 * 1000.0;
    assert!(r >= case.r1_mm());
    let line = synthetic([[10.0, r - 5.0], [10.0, r + 5.0]], [10.0, r]);
    let triangle = case.triangles(&[line], DEFAULT_INCIDENCE_DEG)[0].clone();
    let Blade2dAngle::Angle(beta_2d) = triangle.beta_2d else {
        panic!("se esperaba un ángulo, no {:?}", triangle.beta_2d);
    };
    close(beta_2d, (inner.beta + outer.beta) / 2.0, 1.0e-12);
    close(triangle.mismatch().unwrap(), beta_2d - triangle.beta1b, 0.0);
}

#[test]
fn the_2d_angle_at_d1_is_the_blade_inlet_angle() {
    let case = case_of(&b1_input());
    let r = case.r1_mm();
    let line = synthetic([[10.0, r - 5.0], [10.0, r + 5.0]], [10.0, r]);
    let triangle = case.triangles(&[line], DEFAULT_INCIDENCE_DEG)[0].clone();
    let Blade2dAngle::Angle(beta_2d) = triangle.beta_2d else {
        panic!("en d1/2 hay álabe, no {:?}", triangle.beta_2d);
    };
    close(beta_2d, case.impeller.blade[0].beta, 1.0e-9);
}

#[test]
fn a_cut_below_d1_has_no_2d_blade() {
    let case = case_of(&b1_input());
    let r = 0.8 * case.r1_mm();
    let line = synthetic([[10.0, r - 5.0], [10.0, r + 5.0]], [10.0, r]);
    let triangle = case.triangles(&[line], DEFAULT_INCIDENCE_DEG)[0].clone();
    assert_eq!(triangle.beta_2d, Blade2dAngle::NoBlade);
    assert_eq!(triangle.mismatch(), None);
}

#[test]
fn an_axial_segment_reports_the_capped_angle() {
    let case = case_of(&b1_input());
    let r = 1.1 * case.r1_mm();
    let line = synthetic([[0.0, r], [20.0, r]], [10.0, r]);
    let triangle = case.triangles(&[line], DEFAULT_INCIDENCE_DEG)[0].clone();
    assert_eq!(
        triangle.beta_2d,
        Blade2dAngle::Angle(AXIAL_BETA_2D_DEG.to_radians())
    );
}

#[test]
fn the_1d_result_is_untouched() {
    let input = b1_input();
    let before = design(&input).unwrap().beta1b;
    let case = case_of(&input);
    let lines = streamlines(&case.model, 9).unwrap();
    let _ = case.triangles(&lines, DEFAULT_INCIDENCE_DEG);
    let after = design(&input).unwrap().beta1b;
    assert_eq!(before.to_bits(), after.to_bits());
    assert_eq!(before.to_bits(), case.result.beta1b.to_bits());
}

// Review Focus 2: un corte en el eje no puede dar NaN ni infinito.
#[test]
fn a_cut_on_the_axis_gives_finite_numbers() {
    let case = case_of(&b1_input());
    let line = synthetic([[10.0, 0.0], [10.0, 20.0]], [10.0, 0.0]);
    let triangle = case.triangles(&[line], DEFAULT_INCIDENCE_DEG)[0].clone();
    assert_eq!(triangle.u1, 0.0);
    assert_eq!(triangle.tau1, 1.0);
    assert!(triangle.beta1b.is_finite() && triangle.beta1_flow.is_finite());
    assert!(triangle.beta1b <= (90.0 + DEFAULT_INCIDENCE_DEG).to_radians());
    assert_eq!(triangle.beta_2d, Blade2dAngle::NoBlade);
}

#[test]
fn no_lines_give_no_triangles() {
    let case = case_of(&b1_input());
    assert!(case.triangles(&[], DEFAULT_INCIDENCE_DEG).is_empty());
}
