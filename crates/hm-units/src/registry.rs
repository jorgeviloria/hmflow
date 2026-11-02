//! Catálogo de unidades conocidas.

use crate::dim::Dimension;
use crate::unit::Unit;

const RPM: f64 = 1.0 / 60.0;
const DEG: f64 = std::f64::consts::PI / 180.0;

/// Busca una unidad por nombre o alias.
pub fn lookup(name: &str) -> Option<Unit> {
    use Dimension as D;

    let unit = match name {
        "1" => Unit::new("1", D::NONE, 1.0),

        // Longitud
        "m" | "meter" | "metro" => Unit::new("m", D::LENGTH, 1.0),
        "mm" => Unit::new("mm", D::LENGTH, 1e-3),
        "cm" => Unit::new("cm", D::LENGTH, 1e-2),
        "km" => Unit::new("km", D::LENGTH, 1e3),
        "in" | "inch" | "pulgada" => Unit::new("in", D::LENGTH, 0.0254),
        "ft" | "foot" | "pie" => Unit::new("ft", D::LENGTH, 0.3048),

        // Tiempo
        "s" | "sec" | "segundo" => Unit::new("s", D::TIME, 1.0),
        "ms" => Unit::new("ms", D::TIME, 1e-3),
        "min" | "minuto" => Unit::new("min", D::TIME, 60.0),
        "h" | "hr" | "hora" => Unit::new("h", D::TIME, 3600.0),

        // Masa
        "kg" => Unit::new("kg", D::MASS, 1.0),
        "g" => Unit::new("g", D::MASS, 1e-3),
        "t" | "tonne" | "tonelada" => Unit::new("t", D::MASS, 1e3),
        "lb" => Unit::new("lb", D::MASS, 0.453_592_37),

        // Temperatura
        "K" | "kelvin" => Unit::new("K", D::TEMPERATURE, 1.0),
        "degC" | "celsius" => Unit::new("degC", D::TEMPERATURE, 1.0).with_offset(273.15),
        "degF" | "fahrenheit" => {
            Unit::new("degF", D::TEMPERATURE, 5.0 / 9.0).with_offset(255.372_222_222_222_2)
        }

        // Presión
        "Pa" => Unit::new("Pa", D::PRESSURE, 1.0),
        "hPa" => Unit::new("hPa", D::PRESSURE, 1e2),
        "kPa" => Unit::new("kPa", D::PRESSURE, 1e3),
        "MPa" => Unit::new("MPa", D::PRESSURE, 1e6),
        "bar" => Unit::new("bar", D::PRESSURE, 1e5),
        "mbar" => Unit::new("mbar", D::PRESSURE, 1e2),
        "psi" => Unit::new("psi", D::PRESSURE, 6_894.757_293_168),
        "atm" => Unit::new("atm", D::PRESSURE, 101_325.0),
        "mmHg" => Unit::new("mmHg", D::PRESSURE, 133.322_387_415),

        // Fuerza, energía, potencia
        "N" => Unit::new("N", D::FORCE, 1.0),
        "kN" => Unit::new("kN", D::FORCE, 1e3),
        "kgf" => Unit::new("kgf", D::FORCE, 9.806_65),
        "J" => Unit::new("J", D::ENERGY, 1.0),
        "kJ" => Unit::new("kJ", D::ENERGY, 1e3),
        "kWh" => Unit::new("kWh", D::ENERGY, 3.6e6),
        "W" => Unit::new("W", D::POWER, 1.0),
        "kW" => Unit::new("kW", D::POWER, 1e3),
        "MW" => Unit::new("MW", D::POWER, 1e6),
        "HP" | "hp" => Unit::new("HP", D::POWER, 745.699_871_582_270_2),
        "cv" => Unit::new("cv", D::POWER, 735.498_75),

        // Volumen y caudal
        "L" | "l" | "litro" => Unit::new("L", D::VOLUME, 1e-3),
        "gal" => Unit::new("gal", D::VOLUME, 3.785_411_784e-3),
        "gpm" => Unit::new("gpm", D::VOLUME.div(D::TIME), 3.785_411_784e-3 / 60.0),

        // Frecuencia y ángulo
        "rpm" => Unit::new("rpm", D::FREQUENCY, RPM),
        "rad" => Unit::new("rad", D::NONE, 1.0),
        "deg" | "grado" => Unit::new("deg", D::NONE, DEG),

        // Velocidad
        "knot" | "nudo" => Unit::new("knot", D::VELOCITY, 0.514_444),

        // Viscosidad dinámica (Pa·s) y cinemática (m²/s)
        "P" | "poise" => Unit::new("P", D::PRESSURE.mul(D::TIME), 0.1),
        "cP" => Unit::new("cP", D::PRESSURE.mul(D::TIME), 1e-3),
        "St" | "stokes" => Unit::new("St", D::AREA.div(D::TIME), 1e-4),
        "cSt" => Unit::new("cSt", D::AREA.div(D::TIME), 1e-6),

        _ => return None,
    };

    Some(unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpm_is_frequency() {
        let rpm = lookup("rpm").unwrap();
        assert_eq!(rpm.dim, Dimension::FREQUENCY);
        assert!((rpm.factor - 1.0 / 60.0).abs() < 1e-12);
    }

    #[test]
    fn celsius_has_offset() {
        let c = lookup("degC").unwrap();
        assert!((c.to_si(0.0) - 273.15).abs() < 1e-9);
        assert!((c.from_si(273.15) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_unit_is_none() {
        assert!(lookup("furlong").is_none());
    }
}
