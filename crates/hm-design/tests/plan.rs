//! Vista en planta: el único sitio del que leen el visor y la exportación.

use hm_design::geometry::geometry;
use hm_design::plan::{PlanModel, plan_model};
use hm_design::{CasingFit, DesignInput, DesignOptions, design, design_with_options};
use hm_units::parse_quantity;

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

fn free_plan() -> (hm_design::DesignResult, PlanModel) {
    let result = design(&input()).unwrap();
    let shape = geometry(&input(), &result).unwrap();
    let plan = plan_model(&result, &shape).unwrap();
    (result, plan)
}

fn names(plan: &PlanModel) -> Vec<&str> {
    plan.curves
        .iter()
        .map(|curve| curve.name.as_str())
        .collect()
}

#[test]
fn the_plan_has_the_circles_one_blade_per_channel_and_the_volute() {
    let (result, plan) = free_plan();
    let blades = result.selection.blade_count as usize;
    let mut expected = vec!["D1".to_string(), "D2".to_string(), "Lengua".to_string()];
    expected.extend((1..=blades).map(|blade| format!("Álabe {blade}")));
    expected.push("Voluta".to_string());
    assert_eq!(names(&plan), expected);
}

#[test]
fn a_circle_is_closed_and_has_its_radius_in_millimetres() {
    let (result, plan) = free_plan();
    let d2 = plan.curve("D2").unwrap();
    let radius = result.d2.si / 2.0 * 1000.0;
    for point in &d2.points {
        assert!(
            (point[0].hypot(point[1]) - radius).abs() < 1e-9,
            "{point:?}"
        );
    }
    let (first, last) = (d2.points[0], d2.points[d2.points.len() - 1]);
    assert!((first[0] - last[0]).abs() < 1e-9 && (first[1] - last[1]).abs() < 1e-9);
}

#[test]
fn the_blades_are_the_mean_line_turned_by_one_pitch_each() {
    let (result, plan) = free_plan();
    let shape = geometry(&input(), &result).unwrap();
    let blades = f64::from(result.selection.blade_count);
    let first = plan.curve("Álabe 1").unwrap();
    let second = plan.curve("Álabe 2").unwrap();
    assert_eq!(first.points.len(), shape.impeller.blade.len());
    for ((a, b), point) in first
        .points
        .iter()
        .zip(&second.points)
        .zip(&shape.impeller.blade)
    {
        let radius = point.r * 1000.0;
        assert!((a[0].hypot(a[1]) - radius).abs() < 1e-9);
        // El segundo álabe es el primero girado un paso.
        let turn = b[1].atan2(b[0]) - a[1].atan2(a[0]);
        let pitch = std::f64::consts::TAU / blades;
        let turn = turn.rem_euclid(std::f64::consts::TAU);
        assert!((turn - pitch).abs() < 1e-9, "{turn} frente a {pitch}");
    }
}

#[test]
fn with_a_casing_there_is_no_volute_to_draw() {
    let casing = CasingFit {
        d2: 0.210,
        neck_diameter: 0.090,
        eye_diameter_max: 0.075,
        bore_diameter: 0.020,
        hub_diameter: 0.030,
        b3: 0.030,
        cutwater_radius: 0.111,
        channels: 1,
        wrap_deg: None,
        throat_area: None,
        leakage: None,
    };
    let options = DesignOptions {
        casing: Some(casing),
        ..DesignOptions::default()
    };
    let result = design_with_options(&input(), &options).unwrap();
    let shape = geometry(&input(), &result).unwrap();
    let plan = plan_model(&result, &shape).unwrap();
    assert!(plan.curve("Voluta").is_none());
    assert!(plan.curve("Lengua").is_some());
}

#[test]
fn the_volute_starts_at_the_base_radius_and_grows() {
    let (result, plan) = free_plan();
    let volute = plan.curve("Voluta").unwrap();
    let start = volute.points[0];
    assert!((start[0] - result.volute.base_radius.si * 1000.0).abs() < 1e-9);
    assert!(start[1].abs() < 1e-9);
    let end = volute.points[volute.points.len() - 1];
    assert!(end[0].hypot(end[1]) > start[0].hypot(start[1]));
}

#[test]
fn the_fingerprint_is_stable_and_sees_any_change() {
    let (_, plan) = free_plan();
    let (_, again) = free_plan();
    assert_eq!(plan.fingerprint(), again.fingerprint());

    let mut moved = plan.clone();
    moved.curves[3].points[5][0] += 1.0e-9;
    assert_ne!(moved.fingerprint(), plan.fingerprint());

    let mut renamed = plan.clone();
    renamed.curves[0].name = "D0".to_string();
    assert_ne!(renamed.fingerprint(), plan.fingerprint());

    // Mover puntos entre curvas vecinas también cambia la huella.
    let mut regrouped = plan.clone();
    let point = regrouped.curves[0].points.pop().unwrap();
    regrouped.curves[1].points.insert(0, point);
    assert_ne!(regrouped.fingerprint(), plan.fingerprint());
}

#[test]
fn the_fingerprint_reads_as_sixteen_hexadecimal_digits() {
    let (_, plan) = free_plan();
    let text = plan.fingerprint_text();
    assert_eq!(text.len(), 16);
    assert!(text.chars().all(|c| c.is_ascii_hexdigit()), "{text}");
}

fn model(curves: &[(&str, &[[f64; 2]])]) -> PlanModel {
    PlanModel {
        curves: curves
            .iter()
            .map(|(name, points)| hm_design::plan::PlanCurve {
                name: (*name).to_string(),
                points: points.to_vec(),
            })
            .collect(),
    }
}

#[test]
fn the_fingerprint_sees_the_second_coordinate_and_where_a_name_ends() {
    let (_, plan) = free_plan();
    let mut moved = plan.clone();
    moved.curves[3].points[5][1] += 1.0e-9;
    assert_ne!(moved.fingerprint(), plan.fingerprint());

    let joined = model(&[("ab", &[]), ("c", &[])]);
    let split = model(&[("a", &[]), ("bc", &[])]);
    assert_ne!(joined.fingerprint(), split.fingerprint());

    let one = model(&[("a", &[[1.0, 2.0]]), ("b", &[])]);
    let other = model(&[("a", &[]), ("b", &[[1.0, 2.0]])]);
    assert_ne!(one.fingerprint(), other.fingerprint());
}

#[test]
fn a_small_fingerprint_keeps_its_leading_zeros() {
    let small = (0..10_000)
        .map(|index| model(&[(&format!("curva {index}"), &[])]))
        .find(|plan| plan.fingerprint() < 1 << 60)
        .unwrap();
    assert_eq!(small.fingerprint_text().len(), 16);
    assert!(small.fingerprint_text().starts_with('0'));
}

#[test]
fn a_circle_has_one_point_per_segment_plus_the_closing_one() {
    let (_, plan) = free_plan();
    for name in ["D1", "D2", "Lengua"] {
        assert_eq!(plan.curve(name).unwrap().points.len(), 121, "{name}");
    }
}

#[test]
fn the_volute_ends_on_its_last_section() {
    let (result, plan) = free_plan();
    let model = result.model_volute.as_ref().unwrap();
    let last = model.sections.last().unwrap();
    let volute = plan.curve("Voluta").unwrap();
    let end = volute.points[volute.points.len() - 1];
    let angle = last.angle_deg.to_radians();
    let radius = last.outer_radius.si * 1000.0;
    assert!((end[0] - radius * angle.cos()).abs() < 1e-9, "{end:?}");
    assert!((end[1] - radius * angle.sin()).abs() < 1e-9, "{end:?}");
}
