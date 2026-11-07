//! Pestaña Resumen: las filas de resultados y su formato.

use eframe::egui;
use egui::RichText;
use hm_engine::PARAM_SPECS;

use crate::model;
use crate::model::Outcome;

/// Fila del resumen: clave para el inspector, etiqueta, valor y fórmula.
pub(super) struct Row {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) value: String,
    pub(super) formula: Option<&'static str>,
}

fn format_si(value: f64, unit: &str) -> String {
    match unit {
        "m" if value.abs() < 1.0 => format!("{:.2} mm", value * 1000.0),
        "m" => format!("{value:.2} m"),
        "m2" => format!("{:.1} mm²", value * 1e6),
        "m3/s" => format!("{:.2} m³/h", value * 3600.0),
        "W" => format!("{:.3} kW", value / 1000.0),
        "N" => format!("{value:.0} N"),
        "N*m" => format!("{value:.2} N·m"),
        "Pa" => format!("{:.1} MPa", value / 1e6),
        "rad" => format!("{:.2}°", value.to_degrees()),
        "m/s" => format!("{value:.2} m/s"),
        "1" => format!("{value:.4}"),
        other => format!("{value:.4} {other}"),
    }
}

pub(super) fn rows(outcome: &Outcome) -> Vec<(&'static str, Vec<Row>)> {
    let result = &outcome.result;
    let selection = &result.selection;
    let mut design = vec![
        Row {
            key: "psi",
            label: "ψ",
            value: format!("{:.3}", selection.psi),
            formula: Some("design.head_coefficient"),
        },
        Row {
            key: "phi",
            label: "φ",
            value: format!("{:.3}", selection.phi),
            formula: Some("design.flow_coefficient"),
        },
        Row {
            key: "beta2",
            label: "β2",
            value: format!("{:.1}°", selection.beta2_deg),
            formula: Some("design.euler_cu2"),
        },
        Row {
            key: "blade_count",
            label: "Z",
            value: selection.blade_count.to_string(),
            formula: None,
        },
    ];
    for spec in PARAM_SPECS.iter().filter(|spec| spec.formula.is_some()) {
        if let Some(value) = hm_design::designed_si(result, spec.name) {
            design.push(Row {
                key: spec.name,
                label: spec.name,
                value: format_si(value, spec.unit),
                formula: spec.formula,
            });
        }
    }

    let hydraulics = vec![
        Row {
            key: "beta1b",
            label: "β1B",
            value: format_si(result.beta1b, "rad"),
            formula: Some("design.blade_inlet_angle"),
        },
        Row {
            key: "transition",
            label: "Subida NPSH3 Qsa/Qd",
            value: format!("{:.2}", result.transition_ratio),
            formula: Some("design.volute_matching_flow_cm"),
        },
        Row {
            key: "recirculation",
            label: "Recirculación q*RB",
            value: format!("{:.2}", result.recirculation_onset),
            formula: Some("design.recirculation_onset"),
        },
    ];

    let mut mechanics = Vec::new();
    if let Ok(mech) = &outcome.mechanics {
        let double_volute = mech.volute_wrap_deg.is_some();
        let items: [(&'static str, &'static str, String, Option<&'static str>); 10] = [
            (
                "mech.shaft",
                "Eje mínimo (torsión)",
                format_si(mech.shaft_min_diameter, "m"),
                Some("mech.shaft_min_diameter"),
            ),
            (
                "mech.hub",
                "Cubo",
                format_si(mech.hub_diameter, "m"),
                Some("mech.hub_diameter"),
            ),
            (
                "mech.blade",
                "Espesor del álabe",
                format_si(mech.blade_thickness, "m"),
                Some("mech.blade_thickness_ratio"),
            ),
            (
                "mech.shroud",
                "Espesor de las tapas",
                format_si(mech.shroud_thickness, "m"),
                Some("mech.shroud_thickness"),
            ),
            (
                "mech.clearance",
                "Holgura del anillo Δd",
                format_si(mech.seal_clearance, "m"),
                Some("mech.seal_clearance"),
            ),
            (
                "mech.radial0",
                if double_volute {
                    "Empuje radial, válvula cerrada (voluta doble)"
                } else {
                    "Empuje radial, válvula cerrada (voluta simple)"
                },
                format_si(mech.radial_thrust_shutoff, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.radial1",
                if double_volute {
                    "Empuje radial, diseño (voluta doble)"
                } else {
                    "Empuje radial, diseño (voluta simple)"
                },
                format_si(mech.radial_thrust_design, "N"),
                Some("mech.radial_thrust"),
            ),
            (
                "mech.axial",
                "Empuje axial",
                format!("{:.0}..{:.0} N", mech.axial_thrust.0, mech.axial_thrust.1),
                Some(if model::balanced_axial_thrust(result) {
                    "mech.axial_thrust_balanced"
                } else {
                    "mech.axial_thrust_estimate"
                }),
            ),
            (
                "mech.driver",
                "Potencia mínima del motor",
                format!(
                    "{} × {:.2}",
                    format_si(mech.rated_power, "W"),
                    mech.driver_margin
                ),
                Some("mech.driver_power"),
            ),
            (
                "mech.motor",
                "Motor IEC",
                mech.motor_rating
                    .map_or("fuera de la serie".into(), |watts| format_si(watts, "W")),
                Some("mech.driver_power"),
            ),
        ];
        mechanics.extend(items.into_iter().map(|(key, label, value, formula)| Row {
            key,
            label,
            value,
            formula,
        }));
    }

    let casing = model::casing_rows(outcome)
        .into_iter()
        .map(|row| Row {
            key: row.key,
            label: row.label,
            value: row.value,
            formula: row.formula,
        })
        .collect();

    vec![
        ("Diseño", design),
        ("Hidráulica fina", hydraulics),
        ("Carcasa", casing),
        ("Mecánica", mechanics),
    ]
}

pub(super) fn summary(ui: &mut egui::Ui, outcome: &Outcome, selected: &mut &'static str) {
    ui.label(
        RichText::new("Pulsa un valor para ver su fórmula, fuente y supuestos.")
            .small()
            .weak(),
    );
    for (title, group) in rows(outcome) {
        if group.is_empty() {
            continue;
        }
        // Los valores de diseño son cortos; los de mecánica llevan etiquetas
        // largas y van en dos columnas.
        let columns = if title == "Diseño" { 4 } else { 2 };
        egui::CollapsingHeader::new(RichText::new(title).strong())
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new(title)
                    .num_columns(columns)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for (index, row) in group.iter().enumerate() {
                            let text = format!("{}  {}", row.label, row.value);
                            if ui.selectable_label(*selected == row.key, text).clicked() {
                                *selected = row.key;
                            }
                            if index % columns == columns - 1 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }
}
