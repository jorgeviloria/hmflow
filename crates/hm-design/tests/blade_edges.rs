//! Bordes del álabe, entrada: romo, elipse y afilado (con asimetría), cotas por
//! pared, límites y cp,min (Gülich, ec. 7.1a).

use hm_design::blade_edges::{
    BladeEdges, CP_MIN_COEFFICIENT, LeadingDims, LeadingKind, NOSE_DEFAULT_RATIO,
    NOSE_MAX_FRACTION, NoseProfile, TAIL_DEFAULT_RATIO, TailProfile, TrailingDims, TrailingKind,
    WEDGE_TIP_RATIO, cp_min,
};
use hm_design::blade_thickness::ThicknessWall;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

/// Espesor nominal de las pruebas: 6 mm.
const E: f64 = 0.006;
/// Longitud de nariz de las pruebas: Lp = 5·e = 30 mm.
const LP: f64 = 0.030;

fn ellipse() -> NoseProfile {
    NoseProfile {
        kind: LeadingKind::Ellipse,
        dims: LeadingDims {
            lp: LP,
            tip: 0.0,
            asymmetry: 0.0,
        },
    }
}

fn wedge(tip: f64, asymmetry: f64) -> NoseProfile {
    NoseProfile {
        kind: LeadingKind::Wedge,
        dims: LeadingDims {
            lp: LP,
            tip,
            asymmetry,
        },
    }
}

/// Espesor total de la nariz a la distancia `s` de la punta.
fn total(profile: &NoseProfile, s: f64) -> f64 {
    let (p, q) = profile.half_thicknesses(s, E);
    p + q
}

#[test]
fn cp_min_matches_the_two_values_printed_in_the_book() {
    // Gülich, ec. 7.1a, p. 414: 2,05 con el semicírculo (e/Lp = 2) y 0,155
    // con la elipse de e/Lp = 0,2.
    close(cp_min(2.0), 2.05, 0.005);
    close(cp_min(0.2), 0.155, 0.001);
    close(CP_MIN_COEFFICIENT, 0.373, 0.0);
    // Sin borde (e/Lp = 0) no hay pico de succión.
    close(cp_min(0.0), 0.0, 0.0);
}

#[test]
fn the_ellipse_is_a_half_ellipse_with_semi_axes_lp_and_half_e() {
    let profile = ellipse();
    for i in 0..=20 {
        let s = LP * f64::from(i) / 20.0;
        let (p, q) = profile.half_thicknesses(s, E);
        assert_eq!(p, q, "la elipse es simétrica (s = {s})");
        // (s, t/2) está en la elipse de semiejes Lp y e/2 con centro en
        // (Lp, 0): ((s − Lp)/Lp)² + (t/e)² = 1.
        let t = p + q;
        let on_ellipse = ((s - LP) / LP).powi(2) + (t / E).powi(2);
        close(on_ellipse, 1.0, 1e-12);
    }
    // En la punta el espesor es cero.
    close(total(&profile, 0.0), 0.0, 1e-15);
}

#[test]
fn the_ellipse_reaches_full_thickness_with_zero_slope_at_lp() {
    let profile = ellipse();
    close(total(&profile, LP), E, 1e-15);
    // Derivada numérica centrada en Lp: del lado de fuera vale e, del de
    // dentro baja como (Δ/Lp)², así que la pendiente es casi nula.
    let d = 1e-6;
    let slope = (total(&profile, LP + d) - total(&profile, LP - d)) / (2.0 * d);
    close(slope, 0.0, 1e-4);
}

#[test]
fn the_wedge_is_linear_from_the_tip_to_e() {
    let tip = 0.1 * E;
    let profile = wedge(tip, 0.0);
    close(total(&profile, 0.0), tip, 1e-15);
    close(total(&profile, LP), E, 1e-15);
    for i in 0..=10 {
        let u = f64::from(i) / 10.0;
        close(total(&profile, u * LP), tip + (E - tip) * u, 1e-15);
    }
    // Sin punta, parte de cero.
    close(total(&wedge(0.0, 0.0), 0.0), 0.0, 1e-15);
}

#[test]
fn symmetric_nose_gives_equal_halves() {
    for profile in [ellipse(), wedge(0.1 * E, 0.0)] {
        for i in 0..=10 {
            let (p, q) = profile.half_thicknesses(LP * f64::from(i) / 10.0, E);
            assert_eq!(p, q);
        }
    }
}

#[test]
fn asymmetry_one_puts_the_pressure_face_on_the_camber_at_the_tip() {
    // Con punta cero las dos caras parten de la línea media.
    let sharp = wedge(0.0, 1.0);
    let (p0, q0) = sharp.half_thicknesses(0.0, E);
    close(p0, 0.0, 1e-18);
    close(q0, 0.0, 1e-18);

    // Con punta > 0, la cara de presión pasa por la línea media (fig. 7.10e)
    // y todo el espesor de la punta cuelga de la succión.
    let tip = 0.1 * E;
    let profile = wedge(tip, 1.0);
    let (p, q) = profile.half_thicknesses(0.0, E);
    close(p, 0.0, 1e-18);
    close(q, tip, 1e-18);

    // La suma de las dos mitades es siempre t, y la presión nunca queda
    // por debajo de la línea media.
    for (profile, tip) in [(profile, tip), (sharp, 0.0)] {
        for i in 0..=20 {
            let u = f64::from(i) / 20.0;
            let (p, q) = profile.half_thicknesses(u * LP, E);
            close(p + q, tip + (E - tip) * u, 1e-15);
            assert!(p >= 0.0, "h_p ≥ 0 (u = {u})");
            assert!(q >= p, "la succión lleva el reparto (u = {u})");
        }
    }
}

#[test]
fn asymmetry_vanishes_with_zero_slope_at_lp() {
    let profile = wedge(0.1 * E, 1.0);
    let (p, q) = profile.half_thicknesses(LP, E);
    close(p, E / 2.0, 1e-15);
    close(q, E / 2.0, 1e-15);
    // El reparto q − p es 2c = a·t·(1 − u)²: tangente nula en Lp.
    let split = |s: f64| {
        let (p, q) = profile.half_thicknesses(s, E);
        q - p
    };
    let d = 1e-6;
    close((split(LP + d) - split(LP - d)) / (2.0 * d), 0.0, 1e-5);
    // Y a medio camino queda (1 − u)² = 1/4 del espesor local.
    close(split(LP / 2.0), total(&profile, LP / 2.0) * 0.25, 1e-15);
}

#[test]
fn blunt_and_beyond_lp_give_half_the_thickness() {
    let blunt = NoseProfile {
        kind: LeadingKind::Blunt,
        dims: LeadingDims {
            lp: LP,
            tip: 0.0,
            asymmetry: 1.0,
        },
    };
    // Con romo ninguna cota importa, en ninguna s.
    for s in [-1.0, 0.0, LP / 2.0, LP, 1.0] {
        assert_eq!(blunt.half_thicknesses(s, E), (E / 2.0, E / 2.0));
    }
    assert_eq!(blunt.ratio(E), None);
    // Pasado Lp (y antes de la punta), el cuerpo.
    for profile in [ellipse(), wedge(0.1 * E, 1.0)] {
        for s in [LP * 1.0001, 2.0 * LP, 1.0, -1e-9] {
            assert_eq!(profile.half_thicknesses(s, E), (E / 2.0, E / 2.0));
        }
    }
}

#[test]
fn defaults_follow_the_thickness() {
    close(NOSE_DEFAULT_RATIO, 5.0, 0.0);
    close(WEDGE_TIP_RATIO, 0.1, 0.0);
    let ellipse = LeadingDims::default_for(LeadingKind::Ellipse, E);
    close(ellipse.lp, 5.0 * E, 1e-18);
    close(ellipse.asymmetry, 0.0, 0.0);
    // e/Lp = 0,2 (Gülich, ec. 7.1a).
    let profile = NoseProfile {
        kind: LeadingKind::Ellipse,
        dims: ellipse,
    };
    close(profile.ratio(E).unwrap(), 0.2, 1e-12);

    let wedge = LeadingDims::default_for(LeadingKind::Wedge, E);
    close(wedge.lp, 5.0 * E, 1e-18);
    close(wedge.tip, 0.1 * E, 1e-18);
    close(wedge.asymmetry, 0.0, 0.0);

    // Con otro espesor, otras cotas: siguen a e.
    close(
        LeadingDims::default_for(LeadingKind::Ellipse, 0.004).lp,
        0.020,
        1e-18,
    );
}

#[test]
fn clamping_keeps_dims_inside_the_limits() {
    let length = 0.1;
    let dims = |lp: f64, tip: f64, asymmetry: f64| LeadingDims { lp, tip, asymmetry };

    // Lp < e/2 → e/2 (el semicírculo).
    let short = dims(0.5 * E / 2.0, 0.0, 0.0).clamped(E, length).unwrap();
    close(short.lp, E / 2.0, 1e-18);
    // Lp > 0,3·L → 0,3·L.
    let long = dims(0.5 * length, 0.0, 0.0).clamped(E, length).unwrap();
    close(NOSE_MAX_FRACTION, 0.3, 0.0);
    close(long.lp, NOSE_MAX_FRACTION * length, 1e-18);
    // Dentro de los límites no se toca.
    let ok = dims(LP, 0.5 * E, 0.4);
    assert_eq!(ok.clamped(E, length), Some(ok));
    // Punta entre 0 y e.
    let tip = dims(LP, 2.0 * E, 0.0).clamped(E, length).unwrap();
    close(tip.tip, E, 1e-18);
    let tip = dims(LP, -E, 0.0).clamped(E, length).unwrap();
    close(tip.tip, 0.0, 0.0);
    // Asimetría entre 0 y 1.
    let a = dims(LP, 0.0, 1.3).clamped(E, length).unwrap();
    close(a.asymmetry, 1.0, 0.0);
    let a = dims(LP, 0.0, -0.2).clamped(E, length).unwrap();
    close(a.asymmetry, 0.0, 0.0);
    // No finito → None.
    assert_eq!(dims(f64::NAN, 0.0, 0.0).clamped(E, length), None);
    assert_eq!(dims(LP, f64::INFINITY, 0.0).clamped(E, length), None);
    assert_eq!(dims(LP, 0.0, f64::NAN).clamped(E, length), None);
    assert_eq!(ok.clamped(f64::NAN, length), None);
    assert_eq!(ok.clamped(E, f64::NAN), None);
}

#[test]
fn inner_lines_blend_hub_and_shroud_by_value() {
    let edges = BladeEdges {
        leading: LeadingKind::Wedge,
        leading_hub: LeadingDims {
            lp: 0.020,
            tip: 0.0004,
            asymmetry: 0.2,
        },
        leading_shroud: LeadingDims {
            lp: 0.040,
            tip: 0.0008,
            asymmetry: 0.6,
        },
        ..BladeEdges::default()
    };
    let blend = edges.profile_at(0.25, false);
    assert_eq!(blend.kind, LeadingKind::Wedge);
    close(blend.dims.lp, 0.75 * 0.020 + 0.25 * 0.040, 1e-15);
    close(blend.dims.tip, 0.75 * 0.0004 + 0.25 * 0.0008, 1e-15);
    close(blend.dims.asymmetry, 0.75 * 0.2 + 0.25 * 0.6, 1e-15);

    // En las paredes, la cota de la pared.
    assert_eq!(edges.profile_at(0.0, false).dims, edges.leading_hub);
    assert_eq!(edges.profile_at(1.0, false).dims, edges.leading_shroud);
    // Con «cubo = tapa» manda el cubo, sea cual sea ψ.
    assert_eq!(edges.profile_at(0.25, true).dims, edges.leading_hub);
    assert_eq!(edges.profile_at(1.0, true).dims, edges.leading_hub);
    // ψ se acota a [0, 1].
    assert_eq!(edges.profile_at(-0.5, false).dims, edges.leading_hub);
    assert_eq!(edges.profile_at(1.5, false).dims, edges.leading_shroud);
}

#[test]
fn changing_the_kind_keeps_the_other_wall_and_resets_dims() {
    let (e_hub, e_shroud) = (0.006, 0.004);
    let edges = BladeEdges::default().with_leading_kind(LeadingKind::Ellipse, e_hub, e_shroud);
    assert_eq!(edges.leading, LeadingKind::Ellipse);
    // Al elegir tipo, las cotas arrancan con default_for en cada pared con su
    // e(0).
    assert_eq!(
        edges.leading_hub,
        LeadingDims::default_for(LeadingKind::Ellipse, e_hub)
    );
    assert_eq!(
        edges.leading_shroud,
        LeadingDims::default_for(LeadingKind::Ellipse, e_shroud)
    );

    // Editar una pared deja la otra como estaba.
    let hub_dims = LeadingDims {
        lp: 0.025,
        tip: 0.0,
        asymmetry: 0.0,
    };
    let edited = edges.with_leading_dims(ThicknessWall::Hub, hub_dims, false);
    assert_eq!(edited.leading_hub, hub_dims);
    assert_eq!(edited.leading_shroud, edges.leading_shroud);
    assert_eq!(edited.leading, LeadingKind::Ellipse);

    let shroud_dims = LeadingDims {
        lp: 0.018,
        tip: 0.0,
        asymmetry: 0.0,
    };
    let edited = edited.with_leading_dims(ThicknessWall::Shroud, shroud_dims, false);
    assert_eq!(edited.leading_hub, hub_dims);
    assert_eq!(edited.leading_shroud, shroud_dims);

    // Con «cubo = tapa» una cota se escribe en las dos paredes y `wall` se
    // ignora, como en la curva e(x).
    let same = edited.with_leading_dims(ThicknessWall::Shroud, hub_dims, true);
    assert_eq!(same.leading_hub, hub_dims);
    assert_eq!(same.leading_shroud, hub_dims);

    // Cambiar de tipo (elipse → afilado) rehace las cotas de las dos paredes.
    let wedge = edited.with_leading_kind(LeadingKind::Wedge, e_hub, e_shroud);
    assert_eq!(wedge.leading, LeadingKind::Wedge);
    assert_eq!(
        wedge.leading_hub,
        LeadingDims::default_for(LeadingKind::Wedge, e_hub)
    );
    assert_eq!(
        wedge.leading_shroud,
        LeadingDims::default_for(LeadingKind::Wedge, e_shroud)
    );
    // Y volver a romo no deja cotas que importen.
    let blunt = wedge.with_leading_kind(LeadingKind::Blunt, e_hub, e_shroud);
    assert_eq!(blunt, BladeEdges::default());
}

#[test]
fn turning_same_off_copies_the_hub_and_on_keeps_the_hub() {
    let edges = BladeEdges {
        leading: LeadingKind::Ellipse,
        leading_hub: LeadingDims {
            lp: 0.024,
            tip: 0.0,
            asymmetry: 0.0,
        },
        leading_shroud: LeadingDims {
            lp: 0.036,
            tip: 0.0,
            asymmetry: 0.0,
        },
        ..BladeEdges::default()
    };
    // Apagarlo: la tapa arranca como copia del cubo.
    let off = edges.with_same(false);
    assert_eq!(off.leading_hub, edges.leading_hub);
    assert_eq!(off.leading_shroud, edges.leading_hub);
    // Encenderlo: manda el cubo y la tapa lo copia (sus cotas se pierden).
    let on = edges.with_same(true);
    assert_eq!(on.leading_hub, edges.leading_hub);
    assert_eq!(on.leading_shroud, edges.leading_hub);
    assert_eq!(on.leading, LeadingKind::Ellipse);
}

#[test]
fn the_default_is_blunt() {
    let edges = BladeEdges::default();
    assert_eq!(edges.leading, LeadingKind::Blunt);
    assert_eq!(edges.leading_hub, LeadingDims::default());
    assert_eq!(edges.leading_shroud, LeadingDims::default());
    assert_eq!(edges.leading_hub.lp, 0.0);
    // Y el álabe con borde romo, en cualquier línea, es el de la etapa 1.
    for psi in [0.0, 0.3, 1.0] {
        let profile = edges.profile_at(psi, false);
        assert_eq!(profile.kind, LeadingKind::Blunt);
        assert_eq!(profile.half_thicknesses(0.0, E), (E / 2.0, E / 2.0));
    }
}

#[test]
fn the_kinds_have_file_ids_that_round_trip() {
    assert_eq!(
        LeadingKind::ALL.map(LeadingKind::id),
        ["blunt", "ellipse", "wedge"]
    );
    for kind in LeadingKind::ALL {
        assert_eq!(LeadingKind::from_id(kind.id()), Some(kind));
    }
    assert_eq!(LeadingKind::from_id("parabola"), None);
    assert_eq!(LeadingKind::from_id(""), None);
}

// ---------------------------------------------------------------------------
// El borde de salida.
// ---------------------------------------------------------------------------

/// e en el cuerpo y e(1) de las pruebas de la salida: 6 mm y 3 mm.
const E1: f64 = 0.003;
/// Largo del limado: 4·e(1) = 12 mm.
const LENGTH: f64 = 0.012;

fn tail(kind: TrailingKind, length: f64, end: f64) -> TailProfile {
    TailProfile {
        kind,
        dims: TrailingDims { length, end },
    }
}

#[test]
fn filing_the_suction_side_moves_only_the_suction_face_to_end_minus_half_e() {
    // Limado por succión con end = 2 mm: en D2 la succión queda a end − e1/2
    // de la línea media, y la presión no se toca (Gülich, 4.5.2, fig. 3.6).
    let end = 0.002;
    let profile = tail(TrailingKind::FiledSuction, LENGTH, end);
    let (p, s) = profile.half_thicknesses(0.0, E, E1);
    assert_eq!(p, E / 2.0);
    close(s, end - E1 / 2.0, 1e-15);
    // El espesor total en D2 es `end`.
    close(p + s - E / 2.0 + E / 2.0, E / 2.0 + end - E1 / 2.0, 1e-15);
    // En el inicio del limado, el cuerpo; entre los dos, lineal.
    let (p, s) = profile.half_thicknesses(LENGTH, E, E1);
    assert_eq!((p, s), (E / 2.0, E / 2.0));
    for i in 0..=10 {
        let w = f64::from(i) / 10.0;
        let (p, s) = profile.half_thicknesses(w * LENGTH, E, E1);
        assert_eq!(p, E / 2.0);
        close(
            s,
            (end - E1 / 2.0) + w * (E / 2.0 - (end - E1 / 2.0)),
            1e-15,
        );
    }
    // Fuera del limado, el cuerpo: más adentro y más allá de D2.
    for v in [1.5 * LENGTH, 1.0, -1e-9] {
        assert_eq!(profile.half_thicknesses(v, E, E1), (E / 2.0, E / 2.0));
    }
}

#[test]
fn with_end_equal_to_half_e_the_filed_face_ends_on_the_camber() {
    // end = e1/2: la cara limada acaba sobre la línea media.
    for (kind, face) in [
        (TrailingKind::FiledSuction, 1),
        (TrailingKind::FiledPressure, 0),
    ] {
        let profile = tail(kind, LENGTH, E1 / 2.0);
        let (p, s) = profile.half_thicknesses(0.0, E, E1);
        let (filed, other) = if face == 1 { (s, p) } else { (p, s) };
        close(filed, 0.0, 1e-15);
        assert_eq!(other, E / 2.0);
        // Con menos, pasa de la línea media (altura negativa): el espesor sigue
        // siendo positivo porque la otra cara no se mueve.
        let less = tail(kind, LENGTH, E1 / 4.0);
        let (p, s) = less.half_thicknesses(0.0, E, E1);
        let (filed, other) = if face == 1 { (s, p) } else { (p, s) };
        assert!(filed < 0.0);
        assert!(filed + other > 0.0);
    }
}

#[test]
fn filing_the_pressure_side_leaves_the_suction_face() {
    let end = 0.002;
    let profile = tail(TrailingKind::FiledPressure, LENGTH, end);
    for i in 0..=10 {
        let w = f64::from(i) / 10.0;
        let (p, s) = profile.half_thicknesses(w * LENGTH, E, E1);
        assert_eq!(s, E / 2.0, "w = {w}");
        close(
            p,
            (end - E1 / 2.0) + w * (E / 2.0 - (end - E1 / 2.0)),
            1e-15,
        );
    }
    let (p, s) = profile.half_thicknesses(0.0, E, E1);
    close(p, end - E1 / 2.0, 1e-15);
    assert_eq!(s, E / 2.0);
}

#[test]
fn the_trailing_ellipse_goes_to_zero_thickness_at_the_exit() {
    let profile = tail(TrailingKind::Ellipse, LENGTH, 0.0);
    let (p, s) = profile.half_thicknesses(0.0, E, E1);
    assert_eq!((p, s), (0.0, 0.0));
    for i in 0..=20 {
        let v = LENGTH * f64::from(i) / 20.0;
        let (p, s) = profile.half_thicknesses(v, E, E1);
        assert_eq!(p, s);
        // La semielipse de semiejes `length` y e/2, con D2 en la punta.
        let on_ellipse = ((v - LENGTH) / LENGTH).powi(2) + (2.0 * p / E).powi(2);
        close(on_ellipse, 1.0, 1e-12);
    }
    // En el inicio, el cuerpo con tangente continua.
    let (p, _) = profile.half_thicknesses(LENGTH, E, E1);
    close(p, E / 2.0, 1e-15);
    let d = 1e-7;
    let slope = (profile.half_thicknesses(LENGTH + d, E, E1).0
        - profile.half_thicknesses(LENGTH - d, E, E1).0)
        / (2.0 * d);
    close(slope, 0.0, 1e-3);
}

#[test]
fn trailing_defaults_follow_the_thickness() {
    close(TAIL_DEFAULT_RATIO, 4.0, 0.0);
    for kind in [TrailingKind::FiledSuction, TrailingKind::FiledPressure] {
        let dims = TrailingDims::default_for(kind, E1);
        close(dims.length, 4.0 * E1, 1e-18);
        close(dims.end, E1 / 2.0, 1e-18);
    }
    let ellipse = TrailingDims::default_for(TrailingKind::Ellipse, E1);
    close(ellipse.length, 4.0 * E1, 1e-18);
    assert_eq!(ellipse.end, 0.0);
    assert_eq!(
        TrailingDims::default_for(TrailingKind::Cut, E1),
        TrailingDims::default()
    );
}

#[test]
fn trailing_clamping() {
    let floor = 0.0015;
    let length = 0.2;
    let dims = |length: f64, end: f64| TrailingDims { length, end };
    // Más de 0,3·L → 0,3·L.
    let long = dims(0.5 * length, 0.002)
        .clamped(E1, length, floor)
        .unwrap();
    close(long.length, 0.3 * length, 1e-18);
    // Muy corta: no baja de e1/2 (de hmflow).
    let short = dims(1e-9, 0.002).clamped(E1, length, floor).unwrap();
    close(short.length, E1 / 2.0, 1e-18);
    // end: por debajo del mínimo de fundición de x = 1, el mínimo; por encima
    // de e1, e1.
    let low = dims(LENGTH, 0.0001).clamped(E1, length, floor).unwrap();
    close(low.end, floor, 1e-18);
    let high = dims(LENGTH, 0.01).clamped(E1, length, floor).unwrap();
    close(high.end, E1, 1e-18);
    // Dentro de los límites no se toca.
    let ok = dims(LENGTH, 0.002);
    assert_eq!(ok.clamped(E1, length, floor), Some(ok));
    // No finito → None.
    assert_eq!(dims(f64::NAN, 0.002).clamped(E1, length, floor), None);
    assert_eq!(dims(LENGTH, f64::INFINITY).clamped(E1, length, floor), None);
    assert_eq!(ok.clamped(f64::NAN, length, floor), None);
    assert_eq!(ok.clamped(E1, f64::NAN, floor), None);
}

#[test]
fn changing_the_trailing_kind_keeps_the_leading_edges() {
    let (e_hub, e_shroud) = (0.006, 0.004);
    let leading = BladeEdges::default().with_leading_kind(LeadingKind::Wedge, e_hub, e_shroud);
    let edges = leading.with_trailing_kind(TrailingKind::FiledSuction, E1, 0.002);
    assert_eq!(edges.trailing, TrailingKind::FiledSuction);
    assert_eq!(edges.leading, LeadingKind::Wedge);
    assert_eq!(edges.leading_hub, leading.leading_hub);
    assert_eq!(edges.leading_shroud, leading.leading_shroud);
    // Las cotas de la salida arrancan por pared con su e(1).
    assert_eq!(
        edges.trailing_hub,
        TrailingDims::default_for(TrailingKind::FiledSuction, E1)
    );
    assert_eq!(
        edges.trailing_shroud,
        TrailingDims::default_for(TrailingKind::FiledSuction, 0.002)
    );
    // Y cambiar la entrada no toca la salida.
    let again = edges.with_leading_kind(LeadingKind::Ellipse, e_hub, e_shroud);
    assert_eq!(again.trailing, TrailingKind::FiledSuction);
    assert_eq!(again.trailing_hub, edges.trailing_hub);
    // Editar una pared de la salida deja la otra; con `same`, las dos.
    let hub_dims = TrailingDims {
        length: 0.02,
        end: 0.002,
    };
    let edited = edges.with_trailing_dims(ThicknessWall::Hub, hub_dims, false);
    assert_eq!(edited.trailing_hub, hub_dims);
    assert_eq!(edited.trailing_shroud, edges.trailing_shroud);
    let same = edited.with_trailing_dims(ThicknessWall::Shroud, hub_dims, true);
    assert_eq!(same.trailing_shroud, hub_dims);
    // «Cubo = tapa» también gobierna la salida.
    let joined = edited.with_same(true);
    assert_eq!(joined.trailing_shroud, edited.trailing_hub);
    assert_eq!(joined.trailing, TrailingKind::FiledSuction);
    // Las cotas por pared mezclan por ψ, como la entrada.
    let blend = edited.tail_at(0.25, false);
    close(
        blend.dims.length,
        0.75 * edited.trailing_hub.length + 0.25 * edited.trailing_shroud.length,
        1e-15,
    );
    assert_eq!(edited.tail_at(0.25, true).dims, edited.trailing_hub);
}

#[test]
fn the_default_trailing_is_cut() {
    let edges = BladeEdges::default();
    assert_eq!(edges.trailing, TrailingKind::Cut);
    assert_eq!(edges.trailing_hub, TrailingDims::default());
    assert_eq!(edges.trailing_shroud, TrailingDims::default());
    // Con recortada, el cuerpo en cualquier punto.
    let cut = edges.tail_at(0.5, false);
    for v in [0.0, 0.001, 1.0] {
        assert_eq!(cut.half_thicknesses(v, E, E1), (E / 2.0, E / 2.0));
    }
    assert_eq!(
        TrailingKind::ALL.map(TrailingKind::id),
        ["cut", "filed_suction", "filed_pressure", "ellipse"]
    );
    for kind in TrailingKind::ALL {
        assert_eq!(TrailingKind::from_id(kind.id()), Some(kind));
    }
    assert_eq!(TrailingKind::from_id("round"), None);
}
