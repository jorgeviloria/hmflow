//! La sección `[blade]` del proyecto y el `BladeSetup` del diseño.

use hm_design::blade_setup::{self, BladeSetup, BladeShape, CamberBows};
use hm_design::blade_thickness::{
    self, BladeThickness, EditedThickness, THICKNESS_NODES, ThicknessNode, node_floor,
};
use hm_engine::{
    Case, CurvePoints, Density, DensitySource, Duty, Entered, Fluid, FluidKind, Intake, PumpType,
    analyze, blade_section_of, blade_setup_of, new_project, parse_temperature, recalc,
};
use hm_project::{Blade, Project};
use hm_units::parse_quantity;

#[test]
fn the_file_ids_are_the_shapes_of_the_design_crate() {
    assert_eq!(
        hm_project::BLADE_SHAPES,
        BladeShape::ALL.map(BladeShape::id)
    );
    assert_eq!(
        hm_project::STREAMLINE_COUNTS,
        blade_setup::STREAMLINE_COUNTS
    );
    assert_eq!(
        hm_project::DEFAULT_STREAMLINES,
        blade_setup::DEFAULT_STREAMLINES
    );
    assert_eq!(hm_project::DEFAULT_BLADE_SHAPE, BladeShape::default().id());
}

#[test]
fn no_section_is_the_default_setup_and_the_default_setup_is_no_section() {
    assert_eq!(blade_setup_of(None), Ok(BladeSetup::default()));
    assert_eq!(blade_section_of(&BladeSetup::default()), None);
    assert_eq!(
        blade_setup_of(Some(&Blade::default())),
        Ok(BladeSetup::default())
    );
}

#[test]
fn a_setup_round_trips_through_the_section() {
    for streamlines in [3, 7, 9] {
        let setup = BladeSetup {
            shape: BladeShape::Free2D,
            streamlines,
            ..BladeSetup::default()
        };
        let section = blade_section_of(&setup).expect("no es el de por defecto");
        assert_eq!(section.shape, "free_2d");
        assert_eq!(section.streamlines, streamlines);
        assert_eq!(blade_setup_of(Some(&section)), Ok(setup));
    }
    // Una forma que aún no está disponible también se guarda y se lee.
    let setup = BladeSetup {
        shape: BladeShape::Ruled3D,
        streamlines: 5,
        ..BladeSetup::default()
    };
    let section = blade_section_of(&setup).unwrap();
    assert_eq!(section.shape, "ruled_3d");
    assert_eq!(blade_setup_of(Some(&section)), Ok(setup));
}

#[test]
fn a_hand_built_section_with_bad_values_is_an_error() {
    let shape = blade_setup_of(Some(&Blade {
        shape: "foo".into(),
        streamlines: 5,
        ..Blade::default()
    }))
    .unwrap_err();
    assert!(
        shape.contains("forma de álabe desconocida «foo»"),
        "{shape}"
    );
    let count = blade_setup_of(Some(&Blade {
        shape: "free_2d".into(),
        streamlines: 4,
        ..Blade::default()
    }))
    .unwrap_err();
    assert!(count.contains("debe ser 3, 5, 7 o 9"), "{count}");
}

#[test]
fn handles_cross_the_engine_both_ways() {
    let setup = BladeSetup {
        shape: BladeShape::Free3D,
        streamlines: 7,
        bows: CamberBows {
            hub: (3.5, -2.0),
            shroud: (0.0, 1.25),
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&setup).expect("no es el de por defecto");
    assert_eq!(section.bow_hub, [3.5, -2.0]);
    assert_eq!(section.bow_shroud, [0.0, 1.25]);
    assert_eq!(blade_setup_of(Some(&section)), Ok(setup));

    // Solo las manijas ya hacen la elección distinta de la de por defecto,
    // también con la forma 2D (se guardan para cuando se vuelva a 3D).
    let only_bows = BladeSetup {
        bows: CamberBows {
            hub: (1.0, 0.0),
            shroud: (0.0, 0.0),
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&only_bows).expect("las manijas se guardan");
    assert_eq!(section.shape, "free_2d");
    assert_eq!(blade_setup_of(Some(&section)), Ok(only_bows));
    assert_eq!(blade_section_of(&BladeSetup::default()), None);
}

#[test]
fn the_handle_range_is_the_same_in_project_and_design() {
    assert_eq!(
        hm_project::BLADE_BOW_RANGE_DEG,
        hm_design::BETA_BOW_RANGE_DEG
    );
}

#[test]
fn a_hand_made_section_out_of_range_is_rejected() {
    for section in [
        Blade {
            bow_hub: [45.0, 0.0],
            ..Blade::default()
        },
        Blade {
            bow_shroud: [0.0, f64::NAN],
            ..Blade::default()
        },
        Blade {
            bow_shroud: [f64::INFINITY, 0.0],
            ..Blade::default()
        },
    ] {
        let error = blade_setup_of(Some(&section)).unwrap_err();
        assert!(error.contains("bow_"), "{error}");
        assert!(error.contains("-30") && error.contains("30"), "{error}");
    }
}

fn entered(text: &str) -> Entered {
    Entered::parse(text).unwrap()
}

/// La bomba de referencia B1.
fn b1() -> Case {
    Case {
        pump_type: PumpType::EndSuction,
        duty: Duty {
            q: entered("1060 gpm"),
            h: entered("83 m"),
            n: entered("1750 rpm"),
        },
        fluid: Fluid {
            kind: FluidKind::Water,
            temperature: Some(parse_temperature("20 degC").unwrap()),
            density: Density {
                value: parse_quantity("998.2 kg/m3").unwrap(),
                source: DensitySource::Explicit {
                    text: "998.2 kg/m3".to_string(),
                },
            },
            viscosity: None,
        },
        intake: Intake::None,
        mechanical_efficiency: None,
        design: hm_design::DesignOptions::default(),
    }
}

fn nodes(pairs: &[(f64, f64)]) -> Vec<ThicknessNode> {
    pairs.iter().map(|&(x, e)| ThicknessNode { x, e }).collect()
}

/// Un proyecto de esquema 6: el B1 libre 3D, sin ningún campo de espesor.
#[test]
fn a_v6_project_gives_the_same_setup_and_the_same_result() {
    let mut v7 = new_project(
        "b1",
        &b1(),
        &analyze(&b1(), CurvePoints::new(11).unwrap()).unwrap(),
    )
    .unwrap();
    v7.blade = Some(Blade {
        shape: "free_3d".into(),
        ..Blade::default()
    });
    let text = v7.to_toml().unwrap();
    assert!(text.starts_with("schema = 7\n"), "{text}");
    assert!(!text.contains("thickness"), "{text}");
    let v6_text = text.replacen("schema = 7", "schema = 6", 1);
    let v6 = Project::from_toml(&v6_text).unwrap();
    assert_eq!(v6.schema, hm_project::SCHEMA_VERSION);

    let setup = blade_setup_of(v6.blade.as_ref()).unwrap();
    assert_eq!(setup.thickness, BladeThickness::default());
    assert_eq!(setup, blade_setup_of(v7.blade.as_ref()).unwrap());

    let from_v6 = recalc(v6, false).unwrap().project.to_toml().unwrap();
    let from_v7 = recalc(v7, false).unwrap().project.to_toml().unwrap();
    assert_eq!(from_v6, from_v7);
}

#[test]
fn thickness_goes_to_the_setup_in_metres_and_back() {
    let section = Blade {
        shape: "free_3d".into(),
        thickness_hub: vec![[0.0, 6.0], [0.8, 6.0], [1.0, 3.0]],
        ..Blade::default()
    };
    let setup = blade_setup_of(Some(&section)).unwrap();
    assert!(setup.thickness.same);
    let edited = setup.thickness.edited.as_ref().expect("editada");
    let expected = nodes(&[(0.0, 0.006), (0.8, 0.006), (1.0, 0.003)]);
    assert_eq!(edited.hub, expected);
    assert_eq!(edited.shroud, expected);
    assert_eq!(blade_section_of(&setup), Some(section));

    // Con las dos paredes distintas, las dos se guardan.
    let section = Blade {
        thickness_same: false,
        thickness_hub: vec![[0.0, 6.0], [1.0, 3.0]],
        thickness_shroud: vec![[0.0, 5.0], [0.4, 4.5], [1.0, 1.5]],
        ..Blade::default()
    };
    let setup = blade_setup_of(Some(&section)).unwrap();
    let edited = setup.thickness.edited.as_ref().unwrap();
    assert_eq!(
        edited.shroud,
        nodes(&[(0.0, 0.005), (0.4, 0.0045), (1.0, 0.0015)])
    );
    assert_eq!(blade_section_of(&setup), Some(section));

    // «Cubo = tapa» apagado y sin nodos: «Gülich», pero la elección se guarda.
    let split = BladeSetup {
        thickness: BladeThickness {
            same: false,
            edited: None,
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&split).expect("el interruptor se guarda");
    assert!(!section.thickness_same);
    assert!(section.thickness_hub.is_empty());
    assert_eq!(blade_setup_of(Some(&section)), Ok(split));
}

#[test]
fn with_same_only_the_hub_is_written() {
    let curve = nodes(&[(0.0, 0.0061234567), (0.7, 0.006), (1.0, 0.003)]);
    let setup = BladeSetup {
        thickness: BladeThickness {
            same: true,
            edited: Some(EditedThickness {
                hub: curve.clone(),
                shroud: curve,
            }),
        },
        ..BladeSetup::default()
    };
    let section = blade_section_of(&setup).unwrap();
    assert!(section.thickness_shroud.is_empty());
    // x y e se redondean a 6 decimales, en mm.
    assert_eq!(
        section.thickness_hub,
        vec![[0.0, 6.123457], [0.7, 6.0], [1.0, 3.0]]
    );
}

#[test]
fn a_hand_built_thickness_that_does_not_hold_is_an_error() {
    let cases = [
        // Tapa sin cubo.
        Blade {
            thickness_same: false,
            thickness_shroud: vec![[0.0, 6.0], [1.0, 3.0]],
            ..Blade::default()
        },
        // «Cubo = tapa» con dos curvas.
        Blade {
            thickness_hub: vec![[0.0, 6.0], [1.0, 3.0]],
            thickness_shroud: vec![[0.0, 6.0], [1.0, 3.0]],
            ..Blade::default()
        },
        // Con las paredes separadas y editada, faltan las dos.
        Blade {
            thickness_same: false,
            thickness_hub: vec![[0.0, 6.0], [1.0, 3.0]],
            ..Blade::default()
        },
    ];
    for section in cases {
        let error = blade_setup_of(Some(&section)).unwrap_err();
        assert!(error.contains("thickness_"), "{section:?}: {error}");
    }
    // Lo que no cumple la curva sale con el texto de `ThicknessCurve::new`.
    let under = Blade {
        thickness_hub: vec![[0.0, 2.9], [1.0, 3.0]],
        ..Blade::default()
    };
    let error = blade_setup_of(Some(&under)).unwrap_err();
    assert_eq!(
        error,
        blade_thickness::ThicknessCurve::new(nodes(&[(0.0, 0.0029), (1.0, 0.003)]))
            .unwrap_err()
            .to_string()
    );
    let unordered = Blade {
        thickness_hub: vec![[0.0, 6.0], [0.5, 6.0], [0.5, 5.0], [1.0, 3.0]],
        ..Blade::default()
    };
    assert!(blade_setup_of(Some(&unordered)).is_err());
}

#[test]
fn the_project_limits_match_the_design_ones() {
    assert_eq!(hm_project::BLADE_THICKNESS_NODES, THICKNESS_NODES);
    assert_eq!(
        hm_project::BLADE_THICKNESS_FLOOR_MM / 1000.0,
        node_floor(0.5)
    );
    assert_eq!(
        hm_project::BLADE_THICKNESS_EXIT_FLOOR_MM / 1000.0,
        node_floor(1.0)
    );
}
