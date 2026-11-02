//! Lectura de cantidades y unidades escritas por el usuario.

use crate::dim::Dimension;
use crate::error::UnitError;
use crate::quantity::Quantity;
use crate::registry;
use crate::unit::Unit;

/// Interpreta una cantidad como `"120 m3/h"` o `"20 degC"`.
pub fn parse_quantity(input: &str) -> Result<Quantity, UnitError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UnitError::InvalidQuantity(input.to_string()));
    }

    let (value, rest) =
        split_number(text).ok_or_else(|| UnitError::InvalidQuantity(input.to_string()))?;
    let rest = rest.trim();

    if rest.is_empty() {
        Ok(Quantity::new(value, Unit::dimensionless()))
    } else {
        let unit = parse_unit(rest)?;
        Ok(Quantity::new(value, unit))
    }
}

/// Interpreta una unidad como `"m/s"`, `"kg/m3"` o `"1/s"`.
pub fn parse_unit(input: &str) -> Result<Unit, UnitError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UnitError::InvalidUnit(input.to_string()));
    }

    let mut parser = UnitParser { text, index: 0 };
    let unit = parser.expression()?;
    parser.skip_whitespace();
    if parser.index != text.len() {
        return Err(UnitError::InvalidUnit(input.to_string()));
    }
    Ok(unit)
}

fn split_number(text: &str) -> Option<(f64, &str)> {
    let bytes = text.as_bytes();
    let mut i = 0;

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }

    let mut has_digit = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        has_digit = true;
    }

    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            has_digit = true;
        }
    }

    if !has_digit {
        return None;
    }

    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        let saved = i;
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let mut has_exponent = false;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            has_exponent = true;
        }
        if !has_exponent {
            i = saved;
        }
    }

    let value: f64 = text[..i].parse().ok()?;
    Some((value, &text[i..]))
}

struct UnitParser<'a> {
    text: &'a str,
    index: usize,
}

impl<'a> UnitParser<'a> {
    fn bytes(&self) -> &'a [u8] {
        self.text.as_bytes()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes().get(self.index).copied()
    }

    fn advance(&mut self) {
        self.index += 1;
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.advance();
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expression(&mut self) -> Result<Unit, UnitError> {
        let mut unit = self.term()?;
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'*') => {
                    self.advance();
                    let right = self.term()?;
                    unit = unit.mul(&right);
                }
                Some(b'/') => {
                    self.advance();
                    let right = self.term()?;
                    unit = unit.div(&right);
                }
                _ => break,
            }
        }
        Ok(unit)
    }

    fn term(&mut self) -> Result<Unit, UnitError> {
        let mut unit = self.factor()?;
        self.skip_whitespace();
        if self.eat(b'^') {
            self.skip_whitespace();
            let exponent = self.signed_integer()?;
            unit = unit.powi(exponent);
        }
        Ok(unit)
    }

    fn factor(&mut self) -> Result<Unit, UnitError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'(') => {
                self.advance();
                let unit = self.expression()?;
                self.skip_whitespace();
                if !self.eat(b')') {
                    return Err(UnitError::InvalidUnit(self.text.to_string()));
                }
                Ok(unit)
            }
            Some(byte) if byte.is_ascii_digit() => {
                let number = self.unsigned_number()?;
                Ok(Unit::new(format!("{number}"), Dimension::NONE, number))
            }
            _ => {
                let name = self.identifier()?;
                let mut unit = registry::lookup(name)
                    .ok_or_else(|| UnitError::UnknownUnit(name.to_string()))?;
                if let Some(byte) = self.peek()
                    && byte.is_ascii_digit()
                {
                    let exponent = self.unsigned_integer()?;
                    unit = unit.powi(exponent as i32);
                }
                Ok(unit)
            }
        }
    }

    fn identifier(&mut self) -> Result<&'a str, UnitError> {
        self.skip_whitespace();
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphabetic() {
                self.advance();
            } else {
                break;
            }
        }
        if self.index == start {
            Err(UnitError::InvalidUnit(self.text.to_string()))
        } else {
            Ok(&self.text[start..self.index])
        }
    }

    fn unsigned_integer(&mut self) -> Result<u32, UnitError> {
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if self.index == start {
            return Err(UnitError::InvalidUnit(self.text.to_string()));
        }
        self.text[start..self.index]
            .parse()
            .map_err(|_| UnitError::InvalidUnit(self.text.to_string()))
    }

    fn signed_integer(&mut self) -> Result<i32, UnitError> {
        self.skip_whitespace();
        let negative = self.eat(b'-');
        let _ = self.eat(b'+');
        let value = self.unsigned_integer()? as i32;
        Ok(if negative { -value } else { value })
    }

    fn unsigned_number(&mut self) -> Result<f64, UnitError> {
        let start = self.index;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || byte == b'.' {
                self.advance();
            } else {
                break;
            }
        }
        self.text[start..self.index]
            .parse()
            .map_err(|_| UnitError::InvalidUnit(self.text.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_quantity() {
        let q = parse_quantity("120 m3/h").unwrap();
        assert!((q.si - 120.0 / 3600.0).abs() < 1e-12);
        assert_eq!(q.dim(), Dimension::VOLUME.div(Dimension::TIME));
    }

    #[test]
    fn parses_rpm() {
        let q = parse_quantity("2900 rpm").unwrap();
        assert!((q.si - 2900.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn parses_celsius() {
        let q = parse_quantity("20 degC").unwrap();
        assert!((q.si - 293.15).abs() < 1e-9);
    }

    #[test]
    fn parses_composed_unit() {
        let u = parse_unit("kg/m3").unwrap();
        assert_eq!(u.dim, Dimension::DENSITY);
    }

    #[test]
    fn parses_reciprocal() {
        let u = parse_unit("1/s").unwrap();
        assert_eq!(u.dim, Dimension::FREQUENCY);
    }

    #[test]
    fn rejects_unknown_unit() {
        assert!(parse_unit("furlong").is_err());
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(parse_unit("m s").is_err());
    }

    #[test]
    fn parses_force_times_length() {
        let u = parse_unit("N*m").unwrap();
        assert_eq!(u.dim, Dimension::ENERGY);
    }
}
