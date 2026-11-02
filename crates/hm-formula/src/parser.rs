//! Parser de descenso recursivo con precedencia.

use crate::ast::{BinOp, Expr, UnOp};
use crate::error::FormulaError;
use crate::lexer::{Token, lex};

/// Analiza una expresión y devuelve su árbol.
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        return Err(FormulaError::Syntax {
            position: 0,
            message: "expresión vacía".to_string(),
        });
    }

    let mut parser = Parser { tokens, index: 0 };
    let expr = parser.expression()?;

    if parser.index != parser.tokens.len() {
        return Err(FormulaError::Syntax {
            position: parser.index,
            message: "tokens sobrantes al final de la expresión".to_string(),
        });
    }

    Ok(expr)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.index).cloned();
        if token.is_some() {
            self.index += 1;
        }
        token
    }

    fn eat(&mut self, expected: &Token) -> bool {
        if self.peek() == Some(expected) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), FormulaError> {
        if self.eat(&expected) {
            Ok(())
        } else {
            Err(FormulaError::Syntax {
                position: self.index,
                message: format!("se esperaba {expected:?}"),
            })
        }
    }

    fn expression(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.term()?;

        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.index += 1;
                    let right = self.term()?;
                    left = Expr::binary(BinOp::Add, left, right);
                }
                Some(Token::Minus) => {
                    self.index += 1;
                    let right = self.term()?;
                    left = Expr::binary(BinOp::Sub, left, right);
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, FormulaError> {
        let mut left = self.unary()?;

        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.index += 1;
                    let right = self.unary()?;
                    left = Expr::binary(BinOp::Mul, left, right);
                }
                Some(Token::Slash) => {
                    self.index += 1;
                    let right = self.unary()?;
                    left = Expr::binary(BinOp::Div, left, right);
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, FormulaError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.index += 1;
                Ok(Expr::unary(UnOp::Neg, self.unary()?))
            }
            Some(Token::Plus) => {
                self.index += 1;
                Ok(Expr::unary(UnOp::Pos, self.unary()?))
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Expr, FormulaError> {
        let base = self.primary()?;

        if self.eat(&Token::Caret) {
            let exponent = self.unary()?;
            Ok(Expr::binary(BinOp::Pow, base, exponent))
        } else {
            Ok(base)
        }
    }

    fn primary(&mut self) -> Result<Expr, FormulaError> {
        match self.bump() {
            Some(Token::Number(value)) => Ok(Expr::Number(value)),
            Some(Token::Ident(name)) => {
                if self.eat(&Token::LParen) {
                    let mut args = Vec::new();
                    if !self.eat(&Token::RParen) {
                        loop {
                            args.push(self.expression()?);
                            if self.eat(&Token::Comma) {
                                continue;
                            }
                            self.expect(Token::RParen)?;
                            break;
                        }
                    }
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Some(Token::LParen) => {
                let inner = self.expression()?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            other => Err(FormulaError::Syntax {
                position: self.index,
                message: format!("token inesperado: {other:?}"),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precedence_multiplication_before_addition() {
        let expr = parse("1 + 2*3").unwrap();
        let expected = Expr::binary(
            BinOp::Add,
            Expr::Number(1.0),
            Expr::binary(BinOp::Mul, Expr::Number(2.0), Expr::Number(3.0)),
        );
        assert_eq!(expr, expected);
    }

    #[test]
    fn power_is_right_associative() {
        let expr = parse("2^3^2").unwrap();
        let expected = Expr::binary(
            BinOp::Pow,
            Expr::Number(2.0),
            Expr::binary(BinOp::Pow, Expr::Number(3.0), Expr::Number(2.0)),
        );
        assert_eq!(expr, expected);
    }

    #[test]
    fn unary_minus_binds_less_than_power() {
        let expr = parse("-2^2").unwrap();
        assert_eq!(
            expr,
            Expr::unary(
                UnOp::Neg,
                Expr::binary(BinOp::Pow, Expr::Number(2.0), Expr::Number(2.0))
            )
        );
    }

    #[test]
    fn parses_function_call() {
        let expr = parse("sqrt(Q)").unwrap();
        assert_eq!(expr, Expr::Call("sqrt".to_string(), vec![Expr::var("Q")]));
    }

    #[test]
    fn rejects_unbalanced_parenthesis() {
        assert!(parse("(1 + 2").is_err());
    }
}
