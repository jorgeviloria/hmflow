use hm_units::{Dimension, Quantity, Unit, UnitError, parse_quantity, parse_unit};

#[test]
fn adding_compatible_units_keeps_left_unit() {
    let a = parse_quantity("1 m").unwrap();
    let b = parse_quantity("50 cm").unwrap();
    let sum = a.add(&b).unwrap();
    assert!((sum.value() - 1.5).abs() < 1e-12);
}

#[test]
fn adding_incompatible_units_fails() {
    let a = parse_quantity("1 m").unwrap();
    let b = parse_quantity("1 s").unwrap();
    assert!(a.add(&b).is_err());
}

#[test]
fn multiplying_combines_dimensions() {
    let a = parse_quantity("2 m").unwrap();
    let b = parse_quantity("3 m").unwrap();
    let area = a.mul(&b);
    assert_eq!(area.dim(), Dimension::AREA);
    assert!((area.si - 6.0).abs() < 1e-12);
}

#[test]
fn density_from_mass_and_volume() {
    let mass = parse_quantity("998.2 kg").unwrap();
    let volume = parse_quantity("1 m3").unwrap();
    let rho = mass.div(&volume).unwrap();
    assert_eq!(rho.dim(), Dimension::DENSITY);
    assert!((rho.si - 998.2).abs() < 1e-9);
}

#[test]
fn pressure_is_force_over_area() {
    let force = parse_quantity("10 N").unwrap();
    let area = parse_quantity("2 m2").unwrap();
    let p = force.div(&area).unwrap();
    assert_eq!(p.dim(), Dimension::PRESSURE);
    assert!((p.si - 5.0).abs() < 1e-12);
}

#[test]
fn conversion_between_units() {
    let q = parse_quantity("1 bar").unwrap();
    let kpa = parse_unit("kPa").unwrap();
    let converted = q.to(&kpa).unwrap();
    assert!((converted.value() - 100.0).abs() < 1e-9);
}

#[test]
fn sqrt_of_area_is_length() {
    let area = parse_quantity("4 m2").unwrap();
    let side = area.sqrt();
    assert_eq!(side.dim(), Dimension::LENGTH);
    assert!((side.si - 2.0).abs() < 1e-12);
}

#[test]
fn division_by_zero_fails() {
    let a = parse_quantity("1 m").unwrap();
    let b = parse_quantity("0 m").unwrap();
    assert!(a.div(&b).is_err());
}

#[test]
fn subtracting_celsius_gives_kelvin_delta() {
    let a = parse_quantity("20 degC").unwrap();
    let b = parse_quantity("10 degC").unwrap();
    let difference = a.sub(&b).unwrap();

    assert_eq!(difference.dim(), Dimension::TEMPERATURE);
    assert_eq!(difference.unit.symbol, "K");
    assert!((difference.value() - 10.0).abs() < 1e-9);
}

#[test]
fn adding_absolute_temperatures_fails() {
    let a = parse_quantity("20 degC").unwrap();
    let b = parse_quantity("10 degC").unwrap();
    assert!(matches!(
        a.add(&b),
        Err(UnitError::InvalidTemperatureOperation)
    ));
}

#[test]
fn adding_celsius_and_kelvin_is_allowed() {
    let a = parse_quantity("20 degC").unwrap();
    let b = parse_quantity("10 K").unwrap();
    let sum = a.add(&b).unwrap();
    assert!((sum.value() - 30.0).abs() < 1e-9);
}

#[test]
fn subtracting_kelvin_from_celsius_keeps_celsius() {
    let difference = parse_quantity("20 degC")
        .unwrap()
        .sub(&parse_quantity("10 K").unwrap())
        .unwrap();
    assert_eq!(difference.unit.symbol, "degC");
    assert!((difference.value() - 10.0).abs() < 1e-9);
}

#[test]
fn subtracting_celsius_from_kelvin_is_a_kelvin_delta() {
    let difference = parse_quantity("10 K")
        .unwrap()
        .sub(&parse_quantity("20 degC").unwrap())
        .unwrap();
    assert_eq!(difference.unit.symbol, "K");
    assert!((difference.value() + 283.15).abs() < 1e-9);
}

#[test]
fn non_finite_is_not_physical() {
    let value = Quantity::from_si(f64::NAN, Unit::dimensionless());
    assert_eq!(value.ensure_physical(), Err(UnitError::NonFinite));
}

#[test]
fn offset_unit_roundtrip() {
    let q = parse_quantity("20 degC").unwrap();
    let kelvin = parse_unit("K").unwrap();
    assert!((q.to(&kelvin).unwrap().value() - 293.15).abs() < 1e-9);
}

#[test]
fn us_units_convert() {
    let q = parse_quantity("1000 gpm").unwrap();
    // 1 gal = 3.785411784 L
    let expected = 1000.0 * 3.785_411_784e-3 / 60.0;
    assert!((q.si - expected).abs() < 1e-9);
}

#[test]
fn unit_base_symbol_is_readable() {
    let u = Unit::base(Dimension::POWER);
    assert_eq!(u.symbol, "m2*kg/s3");
}

#[test]
fn composed_units_keep_their_symbol() {
    let cases = [
        ("120 m3/h", 120.0, "m3/h"),
        ("1 km/h", 1.0, "km/h"),
        ("1 kW*h", 1.0, "kW*h"),
        ("1 kg/m/s", 1.0, "kg/m/s"),
        ("1 kg/(m*s)", 1.0, "kg/(m*s)"),
        ("1 (m/s)^2", 1.0, "(m/s)^2"),
        ("1 s^-1", 1.0, "s^-1"),
        ("2 1/s", 2.0, "1/s"),
    ];
    for (text, value, symbol) in cases {
        let quantity = parse_quantity(text).unwrap();
        assert_eq!(quantity.unit.symbol, symbol, "{text}");
        assert!((quantity.value() - value).abs() < 1e-12, "{text}");
    }
}

#[test]
fn composed_symbols_parse_back_to_the_same_unit() {
    for text in [
        "m3/h", "km/h", "kW*h", "kg/m/s", "kg/(m*s)", "(m/s)^2", "s^-1", "L/min", "N*m", "1/s",
        "kg/m3",
    ] {
        let unit = parse_unit(text).unwrap();
        let again = parse_unit(&unit.symbol).unwrap();
        assert_eq!(again.dim, unit.dim, "{text} → {}", unit.symbol);
        assert!(
            (again.factor - unit.factor).abs() <= 1e-12 * unit.factor.abs(),
            "{text} → {}",
            unit.symbol
        );
    }
}
