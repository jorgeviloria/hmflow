use hm_design::geometry::{BLADE_STEPS, BladePoint, blade_length, geometry, wrap_angle};
use hm_design::{DesignInput, DesignOptions, design, design_with_options};
use hm_units::parse_quantity;

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual {actual}, esperado {expected}, tolerancia {tolerance}"
    );
}

fn input(q: &str, h: &str, n: &str) -> DesignInput {
    DesignInput {
        q: parse_quantity(q).unwrap(),
        h: parse_quantity(h).unwrap(),
        n: parse_quantity(n).unwrap(),
        rho: parse_quantity("998.2 kg/m3").unwrap(),
        eta_m: 0.96,
        npsha: None,
        nu: None,
    }
}

#[test]
fn point_by_point_integration_reproduces_d1a1() {
    // Srinivasan 2017, tablas D1.A2 y D1.A3 (pp. 418-419): radios y ángulos
    // publicados; el envolvimiento acumulado termina en 111,06°.
    let table: [(f64, f64); 10] = [
        (0.0394, 25.00),
        (0.0450, 24.87),
        (0.0500, 24.74),
        (0.0550, 24.62),
        (0.0600, 24.49),
        (0.0650, 23.51),
        (0.0700, 23.51),
        (0.0800, 23.93),
        (0.0900, 23.63),
        (0.0940, 23.50),
    ];
    let points: Vec<(f64, f64)> = table
        .iter()
        .map(|(r, beta)| (*r, beta.to_radians()))
        .collect();
    close(wrap_angle(&points).to_degrees(), 111.06, 0.3);
    // Primer tramo: 16,44°.
    close(wrap_angle(&points[..2]).to_degrees(), 16.44, 0.05);
}

#[test]
fn camber_line_matches_the_design_ends() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let geometry = geometry(&input, &result).unwrap();
    let blade = &geometry.impeller.blade;

    assert_eq!(blade.len(), BLADE_STEPS + 1);
    let first = &blade[0];
    let last = blade.last().unwrap();
    close(first.r, result.d1.si / 2.0, 1e-12);
    close(last.r, result.d2.si / 2.0, 1e-12);
    close(first.beta, geometry.impeller.beta1b, 1e-12);
    close(last.beta, result.selection.beta2_deg.to_radians(), 1e-12);
    close(first.theta, 0.0, 1e-12);
    close(last.theta, geometry.impeller.wrap_angle, 1e-12);
    // b(r) = Qth/(2π r cm): B1 en la entrada y B2 en la salida.
    close(first.width, result.b1.si, 1e-9);
    close(last.width, result.b2.si, 1e-9);
    for pair in blade.windows(2) {
        assert!(pair[1].theta > pair[0].theta);
    }
    // β1B = arctan(τ1·cm0/u1) + 2°, con τ1 ≥ 1. El bloqueo lo sube.
    let without_blockage = (result.selection.cm0 / result.u1.si).atan() + 2f64.to_radians();
    assert!(
        geometry.impeller.beta1b > without_blockage + 1.0e-3,
        "β1B {:.3}° sin bloqueo {:.3}°",
        geometry.impeller.beta1b.to_degrees(),
        without_blockage.to_degrees()
    );
}

#[test]
fn d1a1_blade_with_published_selections() {
    // Con el ojo y Z publicados, τ1 lleva β1B a 23,2°. El libro escribe 25°
    // con un bloqueo K1 = 1,5. El envolvimiento baja a 115°, frente a 111°.
    let input = input("0.016 m3/s", "13.8 m", "1800 rpm");
    let options = DesignOptions {
        psi: Some(0.87),
        phi: Some(0.13),
        beta2_rad: Some(23.5f64.to_radians()),
        blade_count: Some(7),
        c0: Some(3.6),
        cm0: Some(2.25),
        j: Some(1.03),
        // El ηv de Srinivasan (0,9722); el motor usa el de Gülich 3.5.10.
        eta_v: Some(0.9722),
        ..DesignOptions::default()
    };
    let result = design_with_options(&input, &options).unwrap();
    let geometry = geometry(&input, &result).unwrap();
    close(geometry.impeller.beta1b.to_degrees(), 23.18, 0.1);
    let wrap = geometry.impeller.wrap_angle.to_degrees();
    close(wrap, 115.4, 1.0);
}

#[test]
fn discharge_diffuser_meets_the_allowable_angle() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let outlet = geometry(&input, &result).unwrap().outlet;

    let throat = result.volute.throat_diameter.si;
    assert!(outlet.nominal_diameter >= throat);
    close(
        outlet.area_ratio,
        (outlet.nominal_diameter / throat).powi(2),
        1e-12,
    );
    // Con L mínima el semiángulo es el admisible, 16,5°·√(R1/L), salvo que
    // ya se cumpla en el borde de la fig. 1.20 (L = R1).
    let allowable = 16.5f64.to_radians() * (throat / 2.0 / outlet.min_length).sqrt();
    let at_chart_edge = (outlet.min_length - throat / 2.0).abs() < 1e-12;
    assert!(outlet.half_angle <= allowable + 1e-9);
    if !at_chart_edge {
        close(outlet.half_angle, allowable, 1e-6);
    }
    assert!(outlet.min_length >= throat / 2.0 - 1e-12);
    let real = ((outlet.nominal_diameter - throat) / 2.0 / outlet.min_length).atan();
    close(outlet.half_angle, real, 1e-12);
}

#[test]
fn cutwater_defaults_to_gulich_and_warns_when_closer() {
    let input = input("120 m3/h", "45 m", "2900 rpm");
    let result = design(&input).unwrap();
    let k3 = result.volute.base_radius.si / (result.d2.si / 2.0);
    // 1,03 + 0,1·30,47/40 + 0,07·998,2·45/10⁶
    close(result.cutwater_min_ratio, 1.10933, 1e-4);
    close(k3, result.cutwater_min_ratio, 1e-12);
    let codes = |result: &hm_design::DesignResult| -> Vec<String> {
        result.diagnostics.iter().map(|d| d.code.clone()).collect()
    };
    assert!(!codes(&result).contains(&"HM-DES-023".to_string()));

    let close_tongue = design_with_options(
        &input,
        &DesignOptions {
            k3: Some(1.04),
            ..DesignOptions::default()
        },
    )
    .unwrap();
    assert!(codes(&close_tongue).contains(&"HM-DES-023".to_string()));
}

fn point(r: f64, theta: f64) -> BladePoint {
    BladePoint {
        r,
        beta: 0.3,
        theta,
        width: 0.02,
    }
}

#[test]
fn blade_length_follows_the_camber_line() {
    // Radial recto: de r = 0,1 a 0,2 sin envolvimiento mide 0,1 m.
    close(
        blade_length(&[point(0.1, 0.0), point(0.2, 0.0)]),
        0.1,
        1e-12,
    );
    // Arco a r = 0,2 con Δθ = 0,5 rad: r·Δθ = 0,1 m.
    close(
        blade_length(&[point(0.2, 0.0), point(0.2, 0.5)]),
        0.1,
        1e-12,
    );
    // Tramo con r medio 0,15: √(0,1² + (0,15·1)²) = 0,180278 m.
    close(
        blade_length(&[point(0.1, 0.0), point(0.2, 1.0)]),
        0.180_278,
        1e-6,
    );
    // Dos tramos suman; un solo punto no tiene longitud.
    close(
        blade_length(&[point(0.1, 0.0), point(0.2, 0.0), point(0.3, 0.0)]),
        0.2,
        1e-12,
    );
    close(blade_length(&[point(0.1, 0.0)]), 0.0, 0.0);
}
