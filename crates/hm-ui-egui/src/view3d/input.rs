//! De ratón y trackpad a gesto del visor, sin estado: lo que egui dice de
//! este cuadro entra y sale un gesto.

/// Lo que el visor hace en este cuadro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gesture {
    None,
    Orbit,
    Pan,
    Zoom,
    Fit,
}

/// Lo que egui dice del puntero en este cuadro, ya sobre el visor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PointerState {
    pub middle_down: bool,
    pub middle_double_click: bool,
    pub ctrl: bool,
    pub shift: bool,
    /// Rueda del ratón, en líneas.
    pub wheel_lines: f32,
    /// Dos dedos en el trackpad, en puntos.
    pub trackpad_scroll: [f32; 2],
    /// Pellizco: 1 si no hay.
    pub pinch: f32,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            middle_down: false,
            middle_double_click: false,
            ctrl: false,
            shift: false,
            wheel_lines: 0.0,
            trackpad_scroll: [0.0, 0.0],
            pinch: 1.0,
        }
    }
}

/// Un gesto por cuadro, con prioridad encuadrar > desplazar > orbitar >
/// zoom. Botón central orbita y con Ctrl desplaza; doble clic central
/// encuadra; rueda o pellizco hacen zoom; dos dedos orbitan y con Mayús
/// desplazan.
pub(crate) fn gesture(state: &PointerState) -> Gesture {
    let moved = |value: f32| value.is_finite() && value != 0.0;
    let trackpad = state.trackpad_scroll.iter().any(|&value| moved(value));
    let pinch = state.pinch.is_finite() && state.pinch > 0.0 && state.pinch != 1.0;
    if state.middle_double_click {
        Gesture::Fit
    } else if (state.middle_down && state.ctrl) || (trackpad && state.shift) {
        Gesture::Pan
    } else if state.middle_down || trackpad {
        Gesture::Orbit
    } else if moved(state.wheel_lines) || pinch {
        Gesture::Zoom
    } else {
        Gesture::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> PointerState {
        PointerState::default()
    }

    #[test]
    fn each_input_maps_to_one_gesture() {
        assert_eq!(gesture(&idle()), Gesture::None);
        let double = PointerState {
            middle_double_click: true,
            ..idle()
        };
        assert_eq!(gesture(&double), Gesture::Fit);
        let pan = PointerState {
            middle_down: true,
            ctrl: true,
            ..idle()
        };
        assert_eq!(gesture(&pan), Gesture::Pan);
        let orbit = PointerState {
            middle_down: true,
            ..idle()
        };
        assert_eq!(gesture(&orbit), Gesture::Orbit);
        let wheel = PointerState {
            wheel_lines: -2.0,
            ..idle()
        };
        assert_eq!(gesture(&wheel), Gesture::Zoom);
        let pinch = PointerState {
            pinch: 1.05,
            ..idle()
        };
        assert_eq!(gesture(&pinch), Gesture::Zoom);
        let trackpad = PointerState {
            trackpad_scroll: [3.0, -1.0],
            ..idle()
        };
        assert_eq!(gesture(&trackpad), Gesture::Orbit);
        let trackpad_pan = PointerState {
            trackpad_scroll: [3.0, -1.0],
            shift: true,
            ..idle()
        };
        assert_eq!(gesture(&trackpad_pan), Gesture::Pan);
        // Mayús sola, Ctrl solo o un pellizco NaN no hacen nada.
        let keys = PointerState {
            shift: true,
            ctrl: true,
            pinch: f32::NAN,
            ..idle()
        };
        assert_eq!(gesture(&keys), Gesture::None);
    }

    #[test]
    fn fit_beats_pan_beats_orbit_beats_zoom() {
        let everything = PointerState {
            middle_down: true,
            middle_double_click: true,
            ctrl: true,
            shift: true,
            wheel_lines: 1.0,
            trackpad_scroll: [1.0, 1.0],
            pinch: 0.9,
        };
        assert_eq!(gesture(&everything), Gesture::Fit);
        let pan = PointerState {
            middle_double_click: false,
            ..everything
        };
        assert_eq!(gesture(&pan), Gesture::Pan);
        let orbit = PointerState {
            ctrl: false,
            shift: false,
            ..pan
        };
        assert_eq!(gesture(&orbit), Gesture::Orbit);
        let zoom = PointerState {
            middle_down: false,
            trackpad_scroll: [0.0, 0.0],
            ..orbit
        };
        assert_eq!(gesture(&zoom), Gesture::Zoom);
    }
}
