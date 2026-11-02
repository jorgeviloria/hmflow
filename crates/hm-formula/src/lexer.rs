//! Análisis léxico del DSL de fórmulas.

use crate::error::FormulaError;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    Comma,
}

pub fn lex(input: &str) -> Result<Vec<Token>, FormulaError> {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut tokens = Vec::new();

    while index < bytes.len() {
        let byte = bytes[index];

        match byte {
            b' ' | b'\t' | b'\n' | b'\r' => index += 1,
            b'+' => {
                tokens.push(Token::Plus);
                index += 1;
            }
            b'-' => {
                tokens.push(Token::Minus);
                index += 1;
            }
            b'*' => {
                tokens.push(Token::Star);
                index += 1;
            }
            b'/' => {
                tokens.push(Token::Slash);
                index += 1;
            }
            b'^' => {
                tokens.push(Token::Caret);
                index += 1;
            }
            b'(' => {
                tokens.push(Token::LParen);
                index += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                index += 1;
            }
            b',' => {
                tokens.push(Token::Comma);
                index += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = index;
                while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.')
                {
                    index += 1;
                }
                if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
                    let saved = index;
                    index += 1;
                    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
                        index += 1;
                    }
                    if index < bytes.len() && bytes[index].is_ascii_digit() {
                        while index < bytes.len() && bytes[index].is_ascii_digit() {
                            index += 1;
                        }
                    } else {
                        index = saved;
                    }
                }
                let text = &input[start..index];
                let value = text.parse().map_err(|_| FormulaError::Syntax {
                    position: start,
                    message: format!("número inválido: {text}"),
                })?;
                tokens.push(Token::Number(value));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                tokens.push(Token::Ident(input[start..index].to_string()));
            }
            other => {
                return Err(FormulaError::Syntax {
                    position: index,
                    message: format!("carácter inesperado: {}", other as char),
                });
            }
        }
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_expression() {
        let tokens = lex("(u2*cu2 - u1*cu1)/g").unwrap();
        assert_eq!(tokens.len(), 11);
        assert_eq!(tokens[0], Token::LParen);
        assert_eq!(tokens[1], Token::Ident("u2".to_string()));
    }

    #[test]
    fn lexes_exponent_notation() {
        let tokens = lex("1e-3").unwrap();
        assert_eq!(tokens, vec![Token::Number(1e-3)]);
    }

    #[test]
    fn rejects_unknown_character() {
        assert!(lex("2 $ 3").is_err());
    }
}
