//! Líneas de corriente del meridiano: reparto por igual caudal con cm
//! constante en cada estación (Gülich 2020, §7.2.1, p. 419; Srinivasan,
//! ec. 4.63) y corte de cada línea con el borde de ataque.

use hm_design::meridian::{
    MeridianEdits, MeridionalModel, meridional_model, meridional_model_with,
};
use hm_design::streamlines::{
    QuasiNormal, Streamline, StreamlineError, equal_flow_fraction, quasi_normals, streamlines,
};
use hm_design::{DesignInput, default_mechanical_efficiency, design};
use hm_units::parse_quantity;

const COUNTS: [u8; 4] = [3, 5, 7, 9];

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

fn model() -> MeridionalModel {
    meridional_model(&design(&input()).unwrap()).unwrap()
}

/// El punto de máximo rendimiento de B1, como en `tests/case_b1.rs`.
fn b1_model() -> MeridionalModel {
    let q = parse_quantity("1060 gpm").unwrap();
    let n = parse_quantity("1750 rpm").unwrap();
    let input = DesignInput {
        h: parse_quantity("83 m").unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: default_mechanical_efficiency(&q, &n).unwrap(),
        npsha: None,
        nu: None,
        q,
        n,
    };
    meridional_model(&design(&input).unwrap()).unwrap()
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Longitud sobre la polilínea hasta su punto más cercano a `target`.
fn along(line: &[[f64; 2]], target: [f64; 2]) -> f64 {
    let mut best = (f64::INFINITY, 0.0);
    let mut travelled = 0.0;
    for pair in line.windows(2) {
        let length = distance(pair[0], pair[1]);
        let direction = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
        let offset = [target[0] - pair[0][0], target[1] - pair[0][1]];
        let fraction = if length > 0.0 {
            ((offset[0] * direction[0] + offset[1] * direction[1]) / (length * length))
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let nearest = [
            pair[0][0] + fraction * direction[0],
            pair[0][1] + fraction * direction[1],
        ];
        let gap = distance(nearest, target);
        if gap < best.0 {
            best = (gap, travelled + fraction * length);
        }
        travelled += length;
    }
    best.1
}

/// El punto de la polilínea a esa longitud desde su principio.
fn at(line: &[[f64; 2]], length: f64) -> [f64; 2] {
    let mut travelled = 0.0;
    for pair in line.windows(2) {
        let step = distance(pair[0], pair[1]);
        if travelled + step >= length && step > 0.0 {
            let fraction = (length - travelled) / step;
            return [
                pair[0][0] + fraction * (pair[1][0] - pair[0][0]),
                pair[0][1] + fraction * (pair[1][1] - pair[0][1]),
            ];
        }
        travelled += step;
    }
    line[line.len() - 1]
}

#[test]
fn the_fraction_is_psi_when_both_radii_are_equal() {
    close(equal_flow_fraction(50.0, 50.0, 0.3), 0.3, 1e-15);
}

#[test]
fn the_fraction_is_the_root_of_psi_with_the_hub_on_the_axis() {
    close(equal_flow_fraction(0.0, 80.0, 0.25), 0.5, 1e-15);
}

#[test]
fn the_fraction_ends_at_zero_and_one() {
    for (hub, shroud) in [(20.0, 70.0), (70.0, 20.0), (0.0, 80.0), (50.0, 50.0)] {
        assert_eq!(equal_flow_fraction(hub, shroud, 0.0), 0.0);
        close(equal_flow_fraction(hub, shroud, 1.0), 1.0, 1e-15);
    }
}

#[test]
fn the_fraction_is_finite_with_both_radii_at_zero() {
    close(equal_flow_fraction(0.0, 0.0, 0.4), 0.4, 1e-15);
}

#[test]
fn the_fraction_splits_the_static_moment() {
    // F(t) = r_H·t + (r_S − r_H)·t²/2 vale ψ·F₁ en el t devuelto.
    for (hub, shroud) in [(20.0, 70.0), (70.0, 20.0), (33.0, 33.5), (1e-9, 90.0)] {
        for psi in [0.1, 0.25, 0.5, 0.75, 0.9] {
            let t = equal_flow_fraction(hub, shroud, psi);
            let moment = hub * t + (shroud - hub) * t * t / 2.0;
            let whole = (hub + shroud) / 2.0;
            close(moment / whole, psi, 1e-12);
        }
    }
}

#[test]
fn neighbouring_lines_carry_equal_flow_at_every_station() {
    let model = model();
    let stations = quasi_normals(&model);
    for count in COUNTS {
        let lines = streamlines(&model, count).unwrap();
        assert_eq!(lines.len(), usize::from(count));
        for (index, station) in stations.iter().enumerate() {
            let (hub, shroud) = (station.hub, station.shroud);
            let width = distance(hub, shroud);
            let moment = |line: &Streamline| {
                let t = distance(line.points[index], hub) / width;
                hub[1] * t + (shroud[1] - hub[1]) * t * t / 2.0
            };
            let share = (hub[1] + shroud[1]) / 2.0 / f64::from(count - 1);
            for pair in lines.windows(2) {
                let carried = moment(&pair[1]) - moment(&pair[0]);
                assert!(
                    ((carried - share) / share).abs() < 1e-9,
                    "N = {count}, estación {index}: {carried} frente a {share}"
                );
            }
        }
    }
}

#[test]
fn lines_do_not_cross_and_the_cuts_run_from_hub_to_shroud() {
    let model = model();
    let stations = quasi_normals(&model);
    let edge = &model.curve("Borde de ataque").unwrap().points;
    for count in COUNTS {
        let lines = streamlines(&model, count).unwrap();
        for (k, line) in lines.iter().enumerate() {
            close(line.psi, k as f64 / f64::from(count - 1), 1e-15);
            assert_eq!(line.points.len(), stations.len());
        }
        for (index, station) in stations.iter().enumerate() {
            let from_hub: Vec<f64> = lines
                .iter()
                .map(|line| distance(line.points[index], station.hub))
                .collect();
            assert!(
                from_hub.windows(2).all(|pair| pair[1] > pair[0]),
                "N = {count}, estación {index}: {from_hub:?}"
            );
        }
        // Los cortes, ordenados a lo largo del borde, del cubo a la tapa.
        let on_edge: Vec<f64> = lines
            .iter()
            .map(|line| along(edge, line.leading_edge))
            .collect();
        let rising = on_edge.windows(2).all(|pair| pair[1] > pair[0]);
        let falling = on_edge.windows(2).all(|pair| pair[1] < pair[0]);
        assert!(rising || falling, "N = {count}: {on_edge:?}");
    }
}

#[test]
fn the_hub_and_shroud_cuts_are_the_edge_points() {
    let model = model();
    for count in COUNTS {
        let lines = streamlines(&model, count).unwrap();
        // `edge_points` es [tapa, cubo].
        assert_eq!(lines[0].leading_edge, model.params.edge_points[1]);
        assert_eq!(
            lines[lines.len() - 1].leading_edge,
            model.params.edge_points[0]
        );
    }
}

#[test]
fn the_first_and_last_lines_are_the_walls() {
    let model = model();
    let stations = quasi_normals(&model);
    let lines = streamlines(&model, 5).unwrap();
    let hub: Vec<[f64; 2]> = stations.iter().map(|station| station.hub).collect();
    let shroud: Vec<[f64; 2]> = stations.iter().map(|station| station.shroud).collect();
    assert_eq!(lines[0].points, hub);
    assert_eq!(lines[4].points, shroud);
}

#[test]
fn every_cut_lies_on_its_line_at_its_length() {
    let model = model();
    for count in COUNTS {
        let lines = streamlines(&model, count).unwrap();
        let last = lines.len() - 1;
        for (k, line) in lines.iter().enumerate() {
            let on_line = at(&line.points, line.m_leading_edge);
            // Las paredes se muestrean: su corte es el punto exacto del borde
            // y queda a menos de una décima de milímetro de la polilínea.
            let tolerance = if k == 0 || k == last { 0.1 } else { 1e-9 };
            assert!(
                distance(on_line, line.leading_edge) <= tolerance,
                "N = {count}, línea {k}: {on_line:?} frente a {:?}",
                line.leading_edge
            );
            let sine = line.sin_gamma_at_leading_edge();
            assert!((0.0..=1.0).contains(&sine), "sen γ = {sine}");
        }
    }
}

#[test]
fn the_sine_of_gamma_is_one_on_a_radial_line_and_zero_on_an_axial_one() {
    let radial = Streamline {
        psi: 0.5,
        points: vec![[10.0, 20.0], [10.0, 40.0], [10.0, 60.0]],
        leading_edge: [10.0, 30.0],
        m_leading_edge: 10.0,
        leading_edge_area: 1.0,
    };
    close(radial.sin_gamma_at_leading_edge(), 1.0, 1e-15);
    let axial = Streamline {
        psi: 0.5,
        points: vec![[0.0, 30.0], [20.0, 30.0], [40.0, 30.0]],
        leading_edge: [40.0, 30.0],
        m_leading_edge: 40.0,
        leading_edge_area: 1.0,
    };
    close(axial.sin_gamma_at_leading_edge(), 0.0, 1e-15);
    let diagonal = Streamline {
        psi: 0.5,
        points: vec![[0.0, 0.0], [3.0, 4.0]],
        leading_edge: [1.5, 2.0],
        m_leading_edge: 2.5,
        leading_edge_area: 1.0,
    };
    close(diagonal.sin_gamma_at_leading_edge(), 0.8, 1e-15);
}

/// El ángulo, en grados, entre una estación y la línea media que la cruza
/// (los puntos medios de sus vecinas). 90° es una normal exacta.
fn angle_with_the_mean_line(stations: &[QuasiNormal], index: usize) -> f64 {
    let middle = |station: &QuasiNormal| {
        [
            (station.hub[0] + station.shroud[0]) / 2.0,
            (station.hub[1] + station.shroud[1]) / 2.0,
        ]
    };
    let (before, after) = (middle(&stations[index - 1]), middle(&stations[index + 1]));
    let flow = [after[0] - before[0], after[1] - before[1]];
    let station = &stations[index];
    let across = [
        station.shroud[0] - station.hub[0],
        station.shroud[1] - station.hub[1],
    ];
    let cosine = (flow[0] * across[0] + flow[1] * across[1])
        / (flow[0].hypot(flow[1]) * across[0].hypot(across[1]));
    cosine.abs().acos().to_degrees()
}

/// Gülich pide normales (estimadas) a las líneas de corriente (§7.2.1, p. 419).
/// Emparejar las tapas por igual fracción de longitud daba rectas hasta 52°
/// fuera de la normal en la curva del canal de B1.
#[test]
fn the_stations_are_close_to_normal_to_the_mean_line() {
    for model in [model(), b1_model()] {
        let stations = quasi_normals(&model);
        assert!(stations.len() > 20, "{}", stations.len());
        for index in 1..stations.len() - 1 {
            let angle = angle_with_the_mean_line(&stations, index);
            assert!(angle > 80.0, "estación {index}: {angle:.1}°");
        }
    }
}

#[test]
fn the_first_station_is_the_inlet_plane_and_the_last_is_the_outlet() {
    for model in [model(), b1_model()] {
        let stations = quasi_normals(&model);
        let (first, last) = (stations[0], stations[stations.len() - 1]);
        let front = &model.curve("Tapa delantera").unwrap().points;
        let rear = &model.curve("Tapa trasera").unwrap().points;
        assert_eq!(first.shroud, front[0]);
        assert_eq!(first.hub, rear[0]);
        assert_eq!(last.shroud, front[front.len() - 1]);
        assert_eq!(last.hub, rear[rear.len() - 1]);
    }
}

#[test]
fn the_stations_advance_along_both_walls_without_crossing() {
    for model in [model(), b1_model()] {
        let stations = quasi_normals(&model);
        let rear = &model.curve("Tapa trasera").unwrap().points;
        let front = &model.curve("Tapa delantera").unwrap().points;
        for pair in stations.windows(2) {
            assert!(
                along(rear, pair[1].hub) >= along(rear, pair[0].hub) - 1e-9,
                "el cubo retrocede: {pair:?}"
            );
            assert!(
                along(front, pair[1].shroud) >= along(front, pair[0].shroud) - 1e-9,
                "la tapa retrocede: {pair:?}"
            );
        }
    }
}

/// El área de una estación es la del tronco de cono que genera: 2π·r·b.
#[test]
fn the_area_of_a_station_is_two_pi_r_b() {
    let station = QuasiNormal {
        hub: [10.0, 30.0],
        shroud: [10.0, 80.0],
    };
    close(station.width(), 50.0, 1e-12);
    close(
        station.area(),
        std::f64::consts::PI * (80.0_f64.powi(2) - 30.0_f64.powi(2)),
        1e-9,
    );
}

/// El corte de la tapa de B1 cae en el plano de entrada: su sección de paso
/// es la corona del ojo, π·(r1² − rH²), no la superficie del borde inclinado.
#[test]
fn the_section_at_a_cut_in_the_inlet_plane_is_the_eye_annulus() {
    let model = b1_model();
    let lines = streamlines(&model, 5).unwrap();
    let shroud = lines.last().unwrap();
    let r1 = shroud.leading_edge[1];
    let r_hub = model.params.d1i / 2.0;
    let annulus = std::f64::consts::PI * (r1 * r1 - r_hub * r_hub);
    close(shroud.leading_edge_area / annulus, 1.0, 1e-9);
    assert!(model.params.edge_area > 1.1 * annulus);
}

/// La sección de cada corte queda entre las de las dos estaciones que lo
/// encierran.
#[test]
fn the_section_at_each_cut_lies_between_its_two_stations() {
    let model = b1_model();
    let stations = quasi_normals(&model);
    for line in streamlines(&model, 9).unwrap() {
        let brackets = stations.windows(2).any(|pair| {
            let (low, high) = (
                pair[0].area().min(pair[1].area()),
                pair[0].area().max(pair[1].area()),
            );
            (low - 1e-6..=high + 1e-6).contains(&line.leading_edge_area)
        });
        assert!(brackets, "ψ = {}: {}", line.psi, line.leading_edge_area);
        assert!(line.leading_edge_area > 0.0);
    }
}

#[test]
fn b1_with_five_lines_keeps_its_cuts() {
    let model = b1_model();
    let lines = streamlines(&model, 5).unwrap();
    assert_eq!(lines[0].leading_edge[1], model.params.edge_points[1][1]);
    assert_eq!(lines[4].leading_edge[1], model.params.edge_points[0][1]);
    // Oráculo propio, para detectar cambios: los radios de los cinco cortes
    // del meridiano por defecto de este punto, en mm. No es dato de catálogo.
    let radii: Vec<f64> = lines.iter().map(|line| line.leading_edge[1]).collect();
    for (actual, expected) in radii.iter().zip(B1_CUT_RADII_MM) {
        close(*actual, expected, 1e-6);
    }
    // Los cortes van ordenados a lo largo del borde. Por radio no: el borde
    // por defecto baja un poco antes de subir hacia la tapa, y la primera
    // línea lo corta por debajo del radio del cubo.
    let edge = &model.curve("Borde de ataque").unwrap().points;
    let on_edge: Vec<f64> = lines
        .iter()
        .map(|line| along(edge, line.leading_edge))
        .collect();
    let rising = on_edge.windows(2).all(|pair| pair[1] > pair[0]);
    let falling = on_edge.windows(2).all(|pair| pair[1] < pair[0]);
    assert!(rising || falling, "{on_edge:?}");
}

const B1_CUT_RADII_MM: [f64; 5] = [
    64.122_176_202,
    62.618_439_039,
    66.078_084_493,
    72.833_903_799,
    81.197_946_966,
];

#[test]
fn a_model_without_walls_has_no_channel() {
    let mut model = model();
    model.curves.clear();
    assert_eq!(streamlines(&model, 5), Err(StreamlineError::NoChannel));
    assert_eq!(
        StreamlineError::NoChannel.to_string(),
        "faltan las curvas de la tapa delantera o de la trasera"
    );
}

#[test]
fn an_edge_that_misses_a_middle_line_is_an_error() {
    let mut model = model();
    // Un borde que solo cubre el 5 % más cercano a la tapa: ninguna línea
    // intermedia lo corta.
    let [shroud, hub] = model.params.edge_points;
    let stub = [
        shroud[0] + 0.05 * (hub[0] - shroud[0]),
        shroud[1] + 0.05 * (hub[1] - shroud[1]),
    ];
    let edge = model
        .curves
        .iter_mut()
        .find(|curve| curve.name == "Borde de ataque")
        .unwrap();
    edge.points = vec![shroud, stub];
    match streamlines(&model, 5) {
        Err(StreamlineError::NoLeadingEdge { line }) => {
            assert!(line > 0 && line < 4, "línea {line}");
            assert_eq!(
                StreamlineError::NoLeadingEdge { line }.to_string(),
                format!("la línea de corriente {line} no corta el borde de ataque")
            );
        }
        other => panic!("se esperaba NoLeadingEdge, salió {other:?}"),
    }
}

#[test]
fn a_hub_on_the_axis_gives_finite_lines() {
    let result = design(&input()).unwrap();
    let edits = MeridianEdits {
        hub_diameter: Some(0.0),
        ..MeridianEdits::default()
    };
    let model = meridional_model_with(&result, &edits).unwrap();
    for count in COUNTS {
        let lines = streamlines(&model, count).unwrap();
        for line in &lines {
            assert!(line.m_leading_edge.is_finite() && line.m_leading_edge >= 0.0);
            assert!(line.leading_edge.iter().all(|value| value.is_finite()));
            for point in &line.points {
                assert!(point[0].is_finite() && point[1].is_finite());
                assert!(point[1] >= 0.0, "r = {}", point[1]);
            }
        }
    }
}
