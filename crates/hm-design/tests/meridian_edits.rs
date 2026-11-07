//! Edición del meridiano con enlace tangente: cada punto de control interior
//! se desliza por su recta, y los extremos los fija el 1D.

use hm_design::meridian::{
    MeridianEdits, MeridionalModel, meridional_model, meridional_model_with,
};
use hm_design::{DesignInput, DesignResult, design};
use hm_units::parse_quantity;

fn result() -> DesignResult {
    let q = |text: &str| parse_quantity(text).unwrap();
    design(&DesignInput {
        q: q("120 m3/h"),
        h: q("45 m"),
        n: q("2900 rpm"),
        rho: q("998.2 kg/m3"),
        eta_m: 0.95,
        npsha: None,
        nu: None,
    })
    .unwrap()
}

fn edited(edits: &MeridianEdits) -> MeridionalModel {
    meridional_model_with(&result(), edits).unwrap()
}

fn points<'a>(model: &'a MeridionalModel, name: &str) -> &'a [[f64; 2]] {
    &model.curve(name).unwrap().points
}

#[test]
fn without_edits_the_section_is_the_one_of_f2() {
    let auto = edited(&MeridianEdits::default());
    assert_eq!(auto, meridional_model(&result()).unwrap());
    // Huella de F2 antes de añadir la edición (commit 32867ef).
    assert_eq!(auto.fingerprint_text(), "78161660e6771965");
}

#[test]
fn the_params_report_the_arms_used() {
    let auto = edited(&MeridianEdits::default());
    let p = &auto.params;
    assert!((p.front_axial_arm - (p.z_e - p.g1)).abs() < 1e-12);
    let r = result();
    assert!((p.rear_axial_arm - (r.b2.si * 1000.0 + p.z_e)).abs() < 1e-9);
    assert!((p.rear_radial_arm - p.inlet_width / 2.0).abs() < 1e-9);
}

#[test]
fn a_front_arm_moves_only_the_front_shroud_and_keeps_its_ends() {
    let auto = edited(&MeridianEdits::default());
    let shorter = MeridianEdits {
        front_radial_arm: Some(auto.params.front_radial_arm * 0.6),
        ..MeridianEdits::default()
    };
    let model = edited(&shorter);
    assert_ne!(
        points(&model, "Tapa delantera"),
        points(&auto, "Tapa delantera")
    );
    assert_eq!(
        points(&model, "Tapa trasera"),
        points(&auto, "Tapa trasera")
    );
    let (front, before) = (
        points(&model, "Tapa delantera"),
        points(&auto, "Tapa delantera"),
    );
    assert_eq!(front[0], before[0]);
    assert_eq!(front[front.len() - 1], before[before.len() - 1]);
    assert_eq!(
        model.params.front_radial_arm,
        auto.params.front_radial_arm * 0.6
    );
}

#[test]
fn the_reported_radius_follows_an_edited_arm() {
    let auto = edited(&MeridianEdits::default());
    let model = edited(&MeridianEdits {
        front_radial_arm: Some(auto.params.front_radial_arm * 0.6),
        ..MeridianEdits::default()
    });
    // Brazo radial más corto: la curva sale más cerrada de g1.
    assert!(model.params.r_ds < auto.params.r_ds);
    let r = result();
    let rise = (r.d2.si - r.d1.si) * 1000.0 / 2.0;
    let a = model.params.front_axial_arm;
    let expected = 1.5 * a * a / (rise - model.params.front_radial_arm);
    assert!((model.params.r_ds - expected).abs() < 1e-9);
}

#[test]
fn tangency_is_kept_at_both_ends_of_the_bend() {
    let auto = edited(&MeridianEdits::default());
    let model = edited(&MeridianEdits {
        front_axial_arm: Some(auto.params.front_axial_arm * 0.5),
        rear_radial_arm: Some(auto.params.rear_radial_arm * 1.5),
        ..MeridianEdits::default()
    });
    let front = points(&model, "Tapa delantera");
    // Sale de g1 en axial y llega a d2 en radial.
    let start = front
        .iter()
        .position(|p| (p[0] - model.params.g1).abs() < 1e-9)
        .unwrap();
    // La tangente es exacta, pero un segmento muestreado de longitud Δs se
    // inclina del orden de Δs/R respecto a ella.
    let (a, b) = (front[start], front[start + 1]);
    let step = (b[0] - a[0]).hypot(b[1] - a[1]);
    assert!(
        (b[1] - a[1]).abs() / step < 1.5 * step / model.params.r_ds,
        "{a:?} {b:?}"
    );
    let (y, z) = (front[front.len() - 2], front[front.len() - 1]);
    let step = (z[0] - y[0]).hypot(z[1] - y[1]);
    assert!(
        (z[0] - y[0]).abs() / step < 1.5 * step / model.params.r_ds,
        "{y:?} {z:?}"
    );
}

#[test]
fn an_arm_out_of_its_line_is_refused_by_name() {
    let auto = edited(&MeridianEdits::default());
    for (edits, name) in [
        (
            MeridianEdits {
                front_axial_arm: Some(auto.params.front_axial_arm * 1.01),
                ..MeridianEdits::default()
            },
            "brazo axial de la tapa delantera",
        ),
        (
            MeridianEdits {
                front_radial_arm: Some(0.0),
                ..MeridianEdits::default()
            },
            "brazo radial de la tapa delantera",
        ),
        (
            MeridianEdits {
                rear_axial_arm: Some(-1.0),
                ..MeridianEdits::default()
            },
            "brazo axial de la tapa trasera",
        ),
        (
            MeridianEdits {
                rear_radial_arm: Some(f64::NAN),
                ..MeridianEdits::default()
            },
            "brazo radial de la tapa trasera",
        ),
    ] {
        let error = meridional_model_with(&result(), &edits)
            .unwrap_err()
            .to_string();
        assert!(error.contains(name), "{error}");
    }
}

#[test]
fn a_rear_arm_moves_only_the_rear_shroud_and_keeps_the_flat_disc() {
    let auto = edited(&MeridianEdits::default());
    let model = edited(&MeridianEdits {
        rear_radial_arm: Some(auto.params.rear_radial_arm * 1.5),
        rear_axial_arm: Some(auto.params.rear_axial_arm * 0.7),
        ..MeridianEdits::default()
    });
    assert_eq!(
        points(&model, "Tapa delantera"),
        points(&auto, "Tapa delantera")
    );
    let (rear, before) = (
        points(&model, "Tapa trasera"),
        points(&auto, "Tapa trasera"),
    );
    assert_ne!(rear, before);
    assert_eq!(rear[0], before[0]);
    assert_eq!(rear[rear.len() - 1], before[before.len() - 1]);
    // En una cúbica la velocidad en los extremos es 3·brazo: el primer paso
    // de las 100 muestras de la curva del cubo mide unos 3·brazo axial/100 y
    // el último, hasta la unión con el disco en d1/2, 3·brazo radial/100.
    let r1 = points(&auto, "Borde de ataque")[0][1];
    let join = rear.iter().position(|p| p[1] == r1).unwrap();
    let step = |a: [f64; 2], b: [f64; 2]| (b[0] - a[0]).hypot(b[1] - a[1]);
    let first = step(rear[0], rear[1]);
    let last = step(rear[join - 1], rear[join]);
    let expected_first = 3.0 * model.params.rear_axial_arm / 100.0;
    let expected_last = 3.0 * model.params.rear_radial_arm / 100.0;
    assert!(
        (first - expected_first).abs() / expected_first < 0.02,
        "{first} {expected_first}"
    );
    assert!(
        (last - expected_last).abs() / expected_last < 0.02,
        "{last} {expected_last}"
    );
    // El disco sigue plano desde d1/2.
    let disc = rear[rear.len() - 1][0];
    assert!(rear.iter().filter(|p| p[1] >= r1).all(|p| p[0] == disc));
}
