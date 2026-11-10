//! Lo que se elige del álabe antes de trazarlo: su forma y el número de líneas
//! de corriente. Va arriba del paso «Álabe β(r)».

use eframe::egui;
use hm_design::blade_setup::{BladeFamily, BladeSetup, BladeShape, STREAMLINE_COUNTS};

/// Cambia la forma solo si está disponible. Devuelve si cambió.
pub(super) fn choose_shape(setup: &mut BladeSetup, shape: BladeShape) -> bool {
    if !shape.is_available() || setup.shape == shape {
        return false;
    }
    setup.shape = shape;
    true
}

/// Cambia N solo si está en [`STREAMLINE_COUNTS`]. Devuelve si cambió.
pub(super) fn choose_streamlines(setup: &mut BladeSetup, count: u8) -> bool {
    if !STREAMLINE_COUNTS.contains(&count) || setup.streamlines == count {
        return false;
    }
    setup.streamlines = count;
    true
}

/// «Llega en el punto 2» o «Llega en el punto 4»; `None` si está disponible.
pub(super) fn arrival_hint(shape: BladeShape) -> Option<String> {
    shape
        .arrives_in()
        .map(|point| format!("Llega en el punto {point}"))
}

/// El texto del desplegable cerrado: «2D · Libre».
fn shape_label(shape: BladeShape) -> String {
    format!("{} · {}", shape.family().name(), shape.name())
}

/// El N de `HMF_STREAMLINES`, si es uno de [`STREAMLINE_COUNTS`].
pub(super) fn streamlines_from_text(text: &str) -> Option<u8> {
    text.trim()
        .parse()
        .ok()
        .filter(|count| STREAMLINE_COUNTS.contains(count))
}

/// El desplegable de la forma y el selector de N, en una fila. Devuelve si
/// cambió algo.
pub(super) fn blade_setup_controls(ui: &mut egui::Ui, setup: &mut BladeSetup) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        egui::ComboBox::from_label("Forma del álabe")
            .selected_text(shape_label(setup.shape))
            .show_ui(ui, |ui| {
                for (index, family) in [BladeFamily::ThreeD, BladeFamily::TwoD]
                    .into_iter()
                    .enumerate()
                {
                    if index > 0 {
                        ui.separator();
                    }
                    ui.label(egui::RichText::new(family.name()).small().weak());
                    for shape in BladeShape::ALL {
                        if shape.family() != family {
                            continue;
                        }
                        let selected = setup.shape == shape;
                        let response = ui
                            .add_enabled_ui(shape.is_available(), |ui| {
                                ui.selectable_label(selected, shape.name())
                            })
                            .inner;
                        let response = match arrival_hint(shape) {
                            Some(hint) => response.on_disabled_hover_text(hint),
                            None => response,
                        };
                        if response.clicked() {
                            changed |= choose_shape(setup, shape);
                        }
                    }
                }
            });
        ui.add_space(12.0);
        ui.label("Líneas de corriente");
        for count in STREAMLINE_COUNTS {
            if ui
                .selectable_label(setup.streamlines == count, count.to_string())
                .clicked()
            {
                changed |= choose_streamlines(setup, count);
            }
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_shape_cannot_be_chosen() {
        for shape in BladeShape::ALL {
            if shape == BladeShape::Free2D {
                continue;
            }
            let mut setup = BladeSetup::default();
            assert!(!choose_shape(&mut setup, shape), "{shape:?}");
            assert_eq!(setup, BladeSetup::default(), "{shape:?}");
        }
    }

    #[test]
    fn an_available_shape_replaces_another() {
        let mut setup = BladeSetup {
            shape: BladeShape::Ruled3D,
            ..BladeSetup::default()
        };
        assert!(choose_shape(&mut setup, BladeShape::Free2D));
        assert_eq!(setup.shape, BladeShape::Free2D);
        // Elegir la que ya está no es un cambio.
        assert!(!choose_shape(&mut setup, BladeShape::Free2D));
    }

    #[test]
    fn only_the_four_counts_can_be_chosen() {
        for count in STREAMLINE_COUNTS {
            let mut setup = BladeSetup {
                streamlines: if count == 3 { 5 } else { 3 },
                ..BladeSetup::default()
            };
            assert!(choose_streamlines(&mut setup, count), "{count}");
            assert_eq!(setup.streamlines, count);
            assert!(!choose_streamlines(&mut setup, count), "{count} repetido");
        }
        for count in [4, 11, 0] {
            let mut setup = BladeSetup::default();
            assert!(!choose_streamlines(&mut setup, count), "{count}");
            assert_eq!(setup, BladeSetup::default(), "{count}");
        }
    }

    #[test]
    fn the_hint_names_the_point_where_the_shape_arrives() {
        assert_eq!(
            arrival_hint(BladeShape::Free3D).as_deref(),
            Some("Llega en el punto 2")
        );
        assert_eq!(
            arrival_hint(BladeShape::Circular2D).as_deref(),
            Some("Llega en el punto 4")
        );
        assert_eq!(arrival_hint(BladeShape::Free2D), None);
    }

    #[test]
    fn the_selected_text_carries_the_family_and_the_name() {
        assert_eq!(shape_label(BladeShape::Free2D), "2D · Libre");
        assert_eq!(shape_label(BladeShape::Ruled3D), "3D · Reglada");
    }

    #[test]
    fn the_capture_seed_only_takes_a_valid_count() {
        assert_eq!(streamlines_from_text(" 7 "), Some(7));
        assert_eq!(streamlines_from_text("4"), None);
        assert_eq!(streamlines_from_text("siete"), None);
    }
}
