//! Curva de espesor e(x) del álabe: nodos, interpolación monótona, estados
//! «Gülich» y «editada», cubo = tapa y ediciones de nodos.

use hm_design::blade_camber::{BladeCamber, CamberLine, CamberOrigin, camber_for_design};
use hm_design::blade_setup::{BladeSetup, BladeShape};
use hm_design::blade_thickness::{
    BladeThickness, EditedThickness, GULICH_TAPER_MIN_X, THICKNESS_NODE_GAP, ThicknessCurve,
    ThicknessCurves, ThicknessError, ThicknessNode, ThicknessWall, mid_line, node_floor,
    thickness_curves_for_design,
};
use hm_design::geometry::{DEFAULT_INCIDENCE_DEG, geometry};
use hm_design::mechanics::{CAST_MIN_THICKNESS, blade_thicknesses};
use hm_design::meridian::meridional_model;
use hm_design::{DesignInput, default_mechanical_efficiency, design};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

/// Un nodo con el espesor en mm.
fn node(x: f64, e_mm: f64) -> ThicknessNode {
    ThicknessNode {
        x,
        e: e_mm / 1000.0,
    }
}

fn nodes(pairs: &[(f64, f64)]) -> Vec<ThicknessNode> {
    pairs.iter().map(|&(x, e)| node(x, e)).collect()
}

fn curve(pairs: &[(f64, f64)]) -> ThicknessCurve {
    ThicknessCurve::new(nodes(pairs)).unwrap()
}

/// Las curvas que se ven en «Gülich» para e = 6 mm en una línea de 0,2 m.
fn gulich_curves() -> ThicknessCurves {
    let gulich = ThicknessCurve::gulich(0.006, 0.003, 0.2);
    ThicknessCurves {
        hub: gulich.clone(),
        shroud: gulich,
    }
}

fn edited(hub: &[(f64, f64)], shroud: &[(f64, f64)], same: bool) -> BladeThickness {
    BladeThickness {
        same,
        edited: Some(EditedThickness {
            hub: nodes(hub),
            shroud: nodes(shroud),
        }),
    }
}

#[test]
fn the_curve_passes_through_its_nodes() {
    let pairs = [(0.0, 6.0), (0.3, 6.0), (0.5, 9.0), (0.8, 4.0), (1.0, 3.0)];
    let curve = curve(&pairs);
    for (x, e_mm) in pairs {
        close(curve.at(x), e_mm / 1000.0, 1e-15);
    }
}

#[test]
fn the_interpolation_never_overshoots_the_nodes() {
    let pairs = [(0.0, 6.0), (0.3, 6.0), (0.5, 9.0), (0.8, 4.0), (1.0, 3.0)];
    let curve = curve(&pairs);
    for i in 0..=1000 {
        let x = f64::from(i) / 1000.0;
        let segment = pairs.windows(2).find(|w| x <= w[1].0).unwrap();
        let (low, high) = (
            segment[0].1.min(segment[1].1) / 1000.0,
            segment[0].1.max(segment[1].1) / 1000.0,
        );
        let e = curve.at(x);
        assert!(
            e >= low - 1e-12 && e <= high + 1e-12,
            "x = {x}: {e} fuera de [{low}, {high}]"
        );
    }
}

#[test]
fn a_run_between_equal_nodes_is_flat() {
    let curve = curve(&[(0.0, 6.0), (0.7, 6.0), (1.0, 3.0)]);
    for i in 0..=700 {
        let x = f64::from(i) / 1000.0;
        assert_eq!(curve.at(x), 0.006, "x = {x}");
    }
}

#[test]
fn two_nodes_give_a_straight_line() {
    let curve = curve(&[(0.0, 6.0), (1.0, 3.0)]);
    close(curve.at(0.5), 0.0045, 1e-15);
    close(curve.at(0.25), 0.00525, 1e-15);
}

#[test]
fn the_gulich_curve_tapers_four_thicknesses_before_the_exit() {
    let curve = ThicknessCurve::gulich(0.006, 0.003, 0.2);
    let nodes = curve.nodes();
    assert_eq!(nodes.len(), 3);
    close(nodes[0].x, 0.0, 0.0);
    close(nodes[0].e, 0.006, 1e-15);
    close(nodes[1].x, 0.88, 1e-12);
    close(nodes[1].e, 0.006, 1e-15);
    close(nodes[2].x, 1.0, 0.0);
    close(nodes[2].e, 0.003, 1e-15);
}

#[test]
fn the_gulich_taper_never_starts_before_mid_blade() {
    for length in [0.04, 0.0, f64::NAN] {
        let curve = ThicknessCurve::gulich(0.006, 0.003, length);
        close(curve.nodes()[1].x, GULICH_TAPER_MIN_X, 0.0);
        close(curve.nodes()[1].x, 0.5, 0.0);
    }
}

#[test]
fn bad_nodes_are_errors() {
    let bad = |pairs: &[(f64, f64)]| ThicknessCurve::new(nodes(pairs)).unwrap_err();
    assert_eq!(bad(&[(0.0, 6.0)]), ThicknessError::NodeCount(1));
    let nine: Vec<(f64, f64)> = (0..9).map(|i| (f64::from(i) / 8.0, 6.0)).collect();
    assert_eq!(bad(&nine), ThicknessError::NodeCount(9));
    assert_eq!(bad(&[(0.1, 6.0), (1.0, 6.0)]), ThicknessError::Ends);
    assert_eq!(bad(&[(0.0, 6.0), (0.9, 6.0)]), ThicknessError::Ends);
    assert_eq!(
        bad(&[(0.0, 6.0), (0.5, 6.0), (0.5, 5.0), (1.0, 4.0)]),
        ThicknessError::NotIncreasing { index: 2 }
    );
    assert_eq!(
        bad(&[(0.0, 6.0), (0.6, 6.0), (0.5, 5.0), (1.0, 4.0)]),
        ThicknessError::NotIncreasing { index: 2 }
    );
    assert_eq!(
        bad(&[(0.0, 6.0), (0.5, f64::NAN), (1.0, 4.0)]),
        ThicknessError::NotFinite
    );
    assert_eq!(
        bad(&[(0.0, 6.0), (f64::INFINITY, 6.0), (1.0, 4.0)]),
        ThicknessError::NotFinite
    );
}

#[test]
fn the_exit_node_may_go_down_to_half_the_casting_minimum() {
    close(node_floor(0.0), CAST_MIN_THICKNESS, 0.0);
    close(node_floor(0.999), CAST_MIN_THICKNESS, 0.0);
    close(node_floor(1.0), CAST_MIN_THICKNESS / 2.0, 0.0);
    assert!(ThicknessCurve::new(nodes(&[(0.0, 6.0), (1.0, 1.5)])).is_ok());
    assert!(matches!(
        ThicknessCurve::new(nodes(&[(0.0, 6.0), (1.0, 1.4)])),
        Err(ThicknessError::UnderFloor { x, .. }) if x == 1.0
    ));
    assert!(matches!(
        ThicknessCurve::new(nodes(&[(0.0, 6.0), (0.5, 2.9), (1.0, 2.0)])),
        Err(ThicknessError::UnderFloor { x, .. }) if x == 0.5
    ));
}

#[test]
fn a_gulich_exit_node_under_3_mm_survives_the_first_edit() {
    // e = 3,21 mm de B1: el nodo de salida de «Gülich» vale 1,605 mm, por
    // debajo de 3 mm pero sobre su mínimo de 1,5 mm.
    let gulich = ThicknessCurve::gulich(0.00321, 0.001_605, 0.2);
    let base = ThicknessCurves {
        hub: gulich.clone(),
        shroud: gulich,
    };
    let setup = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 0, 0.0, 0.004);
    assert!(setup.is_edited());
    let curves = setup.curves(0.00321, 0.001_605, 0.2).unwrap();
    let last = *curves.hub.nodes().last().unwrap();
    assert_eq!(
        last,
        ThicknessNode {
            x: 1.0,
            e: 0.001_605
        }
    );
    close(curves.hub.nodes()[0].e, 0.004, 1e-15);
}

#[test]
fn inner_lines_blend_hub_and_shroud_by_value() {
    let hub = curve(&[(0.0, 6.0), (0.6, 6.0), (1.0, 3.0)]);
    let shroud = curve(&[(0.0, 5.0), (0.2, 7.0), (0.5, 6.0), (0.8, 4.0), (1.0, 3.0)]);
    let curves = ThicknessCurves {
        hub: hub.clone(),
        shroud: shroud.clone(),
    };
    for i in 0..=10 {
        let x = f64::from(i) / 10.0;
        close(
            curves.at(0.25, x),
            0.75 * hub.at(x) + 0.25 * shroud.at(x),
            1e-15,
        );
    }
    // ψ se acota a [0, 1].
    close(curves.at(-1.0, 0.3), hub.at(0.3), 1e-15);
    close(curves.at(2.0, 0.3), shroud.at(0.3), 1e-15);
}

#[test]
fn the_default_follows_the_design() {
    let setup = BladeThickness::default();
    assert!(setup.same);
    assert!(!setup.is_edited());
    assert_eq!(setup.edges(), None);
    let curves = setup.curves(0.006, 0.003, 0.2).unwrap();
    let gulich = ThicknessCurve::gulich(0.006, 0.003, 0.2);
    assert_eq!(curves.hub, gulich);
    assert_eq!(curves.shroud, gulich);
}

#[test]
fn moving_a_node_makes_it_edited_and_keeps_the_others() {
    let base = gulich_curves();
    let setup = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, 0.7, 0.005);
    assert!(setup.is_edited());
    let curves = setup.curves(0.006, 0.003, 0.2).unwrap();
    let nodes = curves.hub.nodes();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[0], ThicknessNode { x: 0.0, e: 0.006 });
    close(nodes[1].x, 0.7, 1e-15);
    close(nodes[1].e, 0.005, 1e-15);
    assert_eq!(nodes[2], ThicknessNode { x: 1.0, e: 0.003 });
    // Ya editada, deja de seguir al diseño: otro e no la cambia.
    let other = setup.curves(0.009, 0.0045, 0.3).unwrap();
    assert_eq!(other.hub.nodes()[1].x, 0.7);
}

#[test]
fn with_same_a_move_writes_both_walls_and_without_it_only_one() {
    let base = gulich_curves();
    let joined =
        BladeThickness::default().with_node_moved(&base, ThicknessWall::Shroud, 1, 0.7, 0.005);
    let edited = joined.edited.as_ref().unwrap();
    assert_eq!(edited.hub, edited.shroud);
    close(edited.hub[1].x, 0.7, 1e-15);

    let apart = BladeThickness {
        same: false,
        edited: None,
    }
    .with_node_moved(&base, ThicknessWall::Shroud, 1, 0.7, 0.005);
    let edited = apart.edited.as_ref().unwrap();
    assert_ne!(edited.hub, edited.shroud);
    close(edited.hub[1].x, 0.88, 1e-12);
    close(edited.shroud[1].x, 0.7, 1e-15);
}

#[test]
fn an_inner_node_cannot_cross_its_neighbours() {
    let base = gulich_curves();
    let setup =
        BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, 0.99, 0.006);
    close(
        setup.edited.unwrap().hub[1].x,
        1.0 - THICKNESS_NODE_GAP,
        1e-15,
    );

    let setup =
        BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, -0.4, 0.006);
    close(setup.edited.unwrap().hub[1].x, THICKNESS_NODE_GAP, 1e-15);
}

#[test]
fn end_nodes_only_move_in_thickness() {
    let base = gulich_curves();
    let first = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 0, 0.3, 0.007);
    let edited = first.edited.unwrap();
    assert_eq!(edited.hub[0], ThicknessNode { x: 0.0, e: 0.007 });
    let last = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 2, 0.4, 0.002);
    let edited = last.edited.unwrap();
    assert_eq!(
        *edited.hub.last().unwrap(),
        ThicknessNode { x: 1.0, e: 0.002 }
    );
}

#[test]
fn a_node_cannot_go_under_its_floor() {
    let base = gulich_curves();
    let inner = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, 0.7, 0.001);
    close(inner.edited.unwrap().hub[1].e, 0.003, 0.0);
    let last = BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 2, 1.0, 0.001);
    close(last.edited.unwrap().hub[2].e, 0.0015, 0.0);
    // Un valor que no es un número no cambia nada.
    let nan =
        BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, 0.7, f64::NAN);
    assert!(!nan.is_edited());
    let nan =
        BladeThickness::default().with_node_moved(&base, ThicknessWall::Hub, 1, f64::NAN, 0.005);
    assert!(!nan.is_edited());
}

#[test]
fn a_new_node_lies_on_the_curve_and_the_ninth_is_refused() {
    let base = ThicknessCurves {
        hub: curve(&[(0.0, 6.0), (0.5, 8.0), (1.0, 3.0)]),
        shroud: curve(&[(0.0, 6.0), (0.5, 8.0), (1.0, 3.0)]),
    };
    let setup = BladeThickness::default()
        .with_node_added(&base, ThicknessWall::Hub, 0.25)
        .unwrap();
    let curves = setup.curves(0.006, 0.003, 0.2).unwrap();
    assert_eq!(curves.hub.nodes().len(), 4);
    close(curves.hub.nodes()[1].x, 0.25, 0.0);
    close(curves.hub.nodes()[1].e, base.hub.at(0.25), 1e-15);
    // Con «cubo = tapa» se escribe en las dos.
    assert_eq!(curves.hub, curves.shroud);

    // Demasiado cerca de otro nodo, o en un extremo.
    assert!(
        setup
            .with_node_added(&curves, ThicknessWall::Hub, 0.255)
            .is_none()
    );
    assert!(
        setup
            .with_node_added(&curves, ThicknessWall::Hub, 0.0)
            .is_none()
    );
    assert!(
        setup
            .with_node_added(&curves, ThicknessWall::Hub, 1.0)
            .is_none()
    );

    // Hasta 8 nodos; el noveno se rechaza.
    let mut current = setup;
    let mut curves = curves;
    for x in [0.1, 0.35, 0.6, 0.8] {
        current = current
            .with_node_added(&curves, ThicknessWall::Hub, x)
            .unwrap();
        curves = current.curves(0.006, 0.003, 0.2).unwrap();
    }
    assert_eq!(curves.hub.nodes().len(), 8);
    assert!(
        current
            .with_node_added(&curves, ThicknessWall::Hub, 0.9)
            .is_none()
    );
}

#[test]
fn a_new_node_never_starts_under_its_floor() {
    // Entre el último nodo de 3 mm y el de 1,5 mm la curva baja de 3 mm; el
    // nodo nuevo se sube a su mínimo para que la curva siga siendo válida.
    let base = gulich_curves();
    let setup = BladeThickness::default()
        .with_node_added(&base, ThicknessWall::Hub, 0.97)
        .unwrap();
    assert!(setup.curves(0.006, 0.003, 0.2).is_ok());
}

#[test]
fn end_nodes_cannot_be_removed_and_two_always_remain() {
    let base = gulich_curves();
    let setup = BladeThickness::default();
    assert!(
        setup
            .with_node_removed(&base, ThicknessWall::Hub, 0)
            .is_none()
    );
    assert!(
        setup
            .with_node_removed(&base, ThicknessWall::Hub, 2)
            .is_none()
    );
    assert!(
        setup
            .with_node_removed(&base, ThicknessWall::Hub, 7)
            .is_none()
    );
    let removed = setup
        .with_node_removed(&base, ThicknessWall::Hub, 1)
        .unwrap();
    let curves = removed.curves(0.006, 0.003, 0.2).unwrap();
    assert_eq!(curves.hub.nodes().len(), 2);
    // Con dos nodos no se quita ninguno más.
    assert!(
        removed
            .with_node_removed(&curves, ThicknessWall::Hub, 0)
            .is_none()
    );
    assert!(
        removed
            .with_node_removed(&curves, ThicknessWall::Hub, 1)
            .is_none()
    );
}

#[test]
fn turning_same_off_copies_the_hub_and_on_drops_the_shroud() {
    let base = gulich_curves();
    // En «Gülich» apagar el interruptor no pasa a «editada».
    let apart = BladeThickness::default().with_same(&base, false);
    assert!(!apart.same);
    assert!(!apart.is_edited());

    // Editada con las dos curvas distintas: encender deja la del cubo.
    let different = edited(
        &[(0.0, 6.0), (0.6, 6.0), (1.0, 3.0)],
        &[(0.0, 5.0), (1.0, 2.0)],
        false,
    );
    let joined = different.with_same(&base, true);
    assert!(joined.same);
    let state = joined.edited.unwrap();
    assert_eq!(state.hub, state.shroud);
    close(state.shroud[1].x, 0.6, 0.0);

    // Apagar en «editada»: la tapa arranca como copia del cubo.
    let apart = joined_apart();
    let state = apart.edited.unwrap();
    assert!(!apart.same);
    assert_eq!(state.hub, state.shroud);
}

fn joined_apart() -> BladeThickness {
    let base = gulich_curves();
    edited(
        &[(0.0, 6.0), (0.6, 6.0), (1.0, 3.0)],
        &[(0.0, 6.0), (0.6, 6.0), (1.0, 3.0)],
        true,
    )
    .with_same(&base, false)
}

#[test]
fn reset_goes_back_to_gulich_and_keeps_the_switch() {
    let setup = edited(&[(0.0, 6.0), (1.0, 3.0)], &[(0.0, 5.0), (1.0, 2.0)], false);
    let reset = setup.reset();
    assert!(!reset.is_edited());
    assert!(!reset.same);
    assert_eq!(reset.edges(), None);
}

#[test]
fn edges_are_those_of_the_mid_line() {
    let setup = edited(&[(0.0, 6.0), (1.0, 3.0)], &[(0.0, 4.0), (1.0, 2.0)], false);
    let (inlet, outlet) = setup.edges().unwrap();
    close(inlet, 0.005, 1e-15);
    close(outlet, 0.0025, 1e-15);
}

// ---------------------------------------------------------------------------
// Un diseño real
// ---------------------------------------------------------------------------

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

#[test]
fn the_design_curves_use_the_mid_line_length() {
    let input = b1_input();
    let result = design(&input).unwrap();
    let impeller = geometry(&input, &result).unwrap().impeller;
    let model = meridional_model(&result).unwrap();
    let setup = BladeSetup {
        shape: BladeShape::Free3D,
        streamlines: 5,
        ..BladeSetup::default()
    };
    let camber = camber_for_design(&result, &impeller, &model, &setup, DEFAULT_INCIDENCE_DEG)
        .unwrap()
        .unwrap();
    assert_eq!(camber.lines.len(), 5);

    let mid = mid_line(&camber).unwrap();
    close(mid.psi, 0.5, 1e-12);
    assert!(std::ptr::eq(mid, &camber.lines[2]));

    let (_, e, e_trailing) = blade_thicknesses(
        result.selection.blade_count,
        result.u2.si,
        result.nq,
        result.d2.si,
    )
    .unwrap();
    let expected_xa = 1.0 - 4.0 * e / camber.lines[2].length;
    assert!(
        expected_xa > GULICH_TAPER_MIN_X,
        "el caso debe usar la fórmula"
    );

    let curves = thickness_curves_for_design(&result, &camber, &BladeThickness::default()).unwrap();
    for wall in [&curves.hub, &curves.shroud] {
        let nodes = wall.nodes();
        assert_eq!(nodes.len(), 3);
        close(nodes[0].e, e, 0.0);
        close(nodes[1].x, expected_xa, 1e-12);
        close(nodes[2].e, e_trailing, 0.0);
    }
}

#[test]
fn the_mid_line_is_the_one_nearest_to_psi_one_half() {
    let line = |psi: f64| CamberLine {
        psi,
        origin: CamberOrigin::Integrated,
        points: Vec::new(),
        wrap: 0.0,
        length: 0.0,
        edge_gap: 0.0,
    };
    let camber = BladeCamber {
        shape: BladeShape::Free3D,
        lines: vec![line(0.0), line(0.4), line(1.0)],
        wrap_max: 0.0,
    };
    close(mid_line(&camber).unwrap().psi, 0.4, 0.0);
    let empty = BladeCamber {
        shape: BladeShape::Free3D,
        lines: Vec::new(),
        wrap_max: 0.0,
    };
    assert!(mid_line(&empty).is_none());
}

/// El mensaje cuenta los nodos desde 1, como el cargador del proyecto
/// (`hm-project`): con x = [0; 0,5; 0,5; 1] el que sobra es el tercero.
#[test]
fn the_not_increasing_error_counts_nodes_from_one() {
    let node = |x: f64| ThicknessNode { x, e: 0.005 };
    let error = ThicknessCurve::new(vec![node(0.0), node(0.5), node(0.5), node(1.0)])
        .expect_err("x repetida");
    assert_eq!(error, ThicknessError::NotIncreasing { index: 2 });
    assert_eq!(
        error.to_string(),
        "el nodo 3 de la curva de espesor no está a la derecha del anterior"
    );
}
