//! A small condition language for rewrite rules.
//!
//! Conditions are evaluated against the analysis data of a matched e-class.
//! The language has arithmetic value expressions, comparisons and boolean
//! combinators:
//!
//! ```text
//! if a != 0
//! if a + 1 > b
//! if a == 0 or b != 0
//! ```

use std::fmt;

use egg::{Condition, Id, Subst, Var};

use crate::analysis::{ConstantFolding, MathEGraph};
use crate::constant::{Constant, Real};
use crate::error::ParseError;
use crate::language::Math;

#[derive(Clone, Debug)]
pub enum ConditionExpr {
    Compare(ValueExpr, CompareOp, ValueExpr),
    Not(Box<ConditionExpr>),
    And(Vec<ConditionExpr>),
    Or(Vec<ConditionExpr>),
}

#[derive(Clone, Debug)]
pub enum ValueExpr {
    Var(Var),
    Const(Constant),
    Neg(Box<ValueExpr>),
    Binary(ValueOp, Box<ValueExpr>, Box<ValueExpr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CompareOp {
    fn from_token(kind: &TokenKind) -> Option<Self> {
        Some(match kind {
            TokenKind::EqEq => Self::Eq,
            TokenKind::NotEq => Self::Ne,
            TokenKind::Lt => Self::Lt,
            TokenKind::Le => Self::Le,
            TokenKind::Gt => Self::Gt,
            TokenKind::Ge => Self::Ge,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }

    fn evaluate(self, left: Option<Constant>, right: Option<Constant>) -> bool {
        match self {
            // `!=` means "not known to be equal".  This is what makes
            // conditions such as `a != 0` usable for symbolic `a`: only an
            // e-class known to contain the constant zero fails the check.
            Self::Ne => !matches!((&left, &right), (Some(left), Some(right)) if left == right),
            Self::Eq => matches!((&left, &right), (Some(left), Some(right)) if left == right),
            _ => {
                let (Some(Constant::Real(left)), Some(Constant::Real(right))) = (left, right)
                else {
                    return false;
                };
                match self {
                    Self::Lt => left < right,
                    Self::Le => left <= right,
                    Self::Gt => left > right,
                    Self::Ge => left >= right,
                    Self::Eq | Self::Ne => unreachable!(),
                }
            }
        }
    }
}

impl ValueOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Pow => "^",
        }
    }
}

impl ConditionExpr {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut parser = ConditionParser::new(text)?;
        let expression = parser.parse_or()?;
        parser.expect_eof()?;
        Ok(expression)
    }

    pub fn collect_vars(&self) -> Vec<Var> {
        let mut vars = Vec::new();
        self.gather_vars(&mut vars);
        vars
    }

    fn gather_vars(&self, vars: &mut Vec<Var>) {
        match self {
            Self::Compare(left, _, right) => {
                left.gather_vars(vars);
                right.gather_vars(vars);
            }
            Self::Not(inner) => inner.gather_vars(vars),
            Self::And(parts) | Self::Or(parts) => {
                for part in parts {
                    part.gather_vars(vars);
                }
            }
        }
    }

    fn evaluate(&self, egraph: &MathEGraph, subst: &Subst) -> bool {
        match self {
            Self::Compare(left, operator, right) => {
                operator.evaluate(left.evaluate(egraph, subst), right.evaluate(egraph, subst))
            }
            Self::Not(inner) => !inner.evaluate(egraph, subst),
            Self::And(parts) => parts.iter().all(|part| part.evaluate(egraph, subst)),
            Self::Or(parts) => parts.iter().any(|part| part.evaluate(egraph, subst)),
        }
    }
}

impl ValueExpr {
    fn gather_vars(&self, vars: &mut Vec<Var>) {
        match self {
            ValueExpr::Var(variable) => {
                if !vars.contains(variable) {
                    vars.push(*variable);
                }
            }
            ValueExpr::Const(_) => {}
            ValueExpr::Neg(inner) => inner.gather_vars(vars),
            ValueExpr::Binary(_, left, right) => {
                left.gather_vars(vars);
                right.gather_vars(vars);
            }
        }
    }

    fn evaluate(&self, egraph: &MathEGraph, subst: &Subst) -> Option<Constant> {
        match self {
            ValueExpr::Var(variable) => {
                let id = subst.get(*variable)?;
                egraph[*id].data.clone()
            }
            ValueExpr::Const(value) => Some(value.clone()),
            ValueExpr::Neg(inner) => inner.evaluate(egraph, subst)?.neg(),
            ValueExpr::Binary(operator, left, right) => {
                let left = left.evaluate(egraph, subst)?;
                let right = right.evaluate(egraph, subst)?;
                match operator {
                    ValueOp::Add => left.add(&right),
                    ValueOp::Sub => left.sub(&right),
                    ValueOp::Mul => left.mul(&right),
                    ValueOp::Div => left.div(&right),
                    ValueOp::Pow => left.pow(&right),
                }
            }
        }
    }

    fn precedence(&self) -> u8 {
        match self {
            ValueExpr::Var(_) | ValueExpr::Const(_) => 5,
            ValueExpr::Neg(_) => 3,
            ValueExpr::Binary(ValueOp::Add | ValueOp::Sub, _, _) => 1,
            ValueExpr::Binary(ValueOp::Mul | ValueOp::Div, _, _) => 2,
            ValueExpr::Binary(ValueOp::Pow, _, _) => 4,
        }
    }

    fn fmt_at(&self, parent_precedence: u8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parentheses = self.precedence() < parent_precedence;
        if parentheses {
            f.write_str("(")?;
        }

        match self {
            ValueExpr::Var(variable) => {
                let name = variable.to_string();
                f.write_str(name.trim_start_matches('?'))?;
            }
            ValueExpr::Const(value) => write!(f, "{value}")?,
            ValueExpr::Neg(inner) => {
                f.write_str("-")?;
                inner.fmt_at(3, f)?;
            }
            ValueExpr::Binary(operator, left, right) => {
                let left_precedence = match operator {
                    ValueOp::Pow => 5,
                    _ => operator_precedence(*operator),
                };
                left.fmt_at(left_precedence, f)?;
                if *operator == ValueOp::Pow {
                    f.write_str("^")?;
                } else {
                    write!(f, " {} ", operator.as_str())?;
                }
                right.fmt_at(operator_precedence(*operator) + 1, f)?;
            }
        }

        if parentheses {
            f.write_str(")")?;
        }
        Ok(())
    }
}

fn operator_precedence(operator: ValueOp) -> u8 {
    match operator {
        ValueOp::Add | ValueOp::Sub => 1,
        ValueOp::Mul | ValueOp::Div => 2,
        ValueOp::Pow => 4,
    }
}

impl ConditionExpr {
    fn precedence(&self) -> u8 {
        match self {
            Self::Or(_) => 1,
            Self::And(_) => 2,
            Self::Not(_) => 3,
            Self::Compare(_, _, _) => 4,
        }
    }

    fn fmt_at(&self, parent_precedence: u8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parentheses = self.precedence() < parent_precedence;
        if parentheses {
            f.write_str("(")?;
        }

        match self {
            Self::Compare(left, operator, right) => {
                left.fmt_at(0, f)?;
                write!(f, " {} ", operator.as_str())?;
                right.fmt_at(0, f)?;
            }
            Self::Not(inner) => {
                f.write_str("not ")?;
                inner.fmt_at(3, f)?;
            }
            Self::And(parts) => {
                for (index, part) in parts.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" and ")?;
                    }
                    part.fmt_at(2, f)?;
                }
            }
            Self::Or(parts) => {
                for (index, part) in parts.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" or ")?;
                    }
                    part.fmt_at(1, f)?;
                }
            }
        }

        if parentheses {
            f.write_str(")")?;
        }
        Ok(())
    }
}

impl fmt::Display for ConditionExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_at(0, f)
    }
}

impl Condition<Math, ConstantFolding> for ConditionExpr {
    fn check(&self, egraph: &mut MathEGraph, _eclass: Id, subst: &Subst) -> bool {
        self.evaluate(egraph, subst)
    }

    fn vars(&self) -> Vec<Var> {
        self.collect_vars()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(Real),
    LParen,
    RParen,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Not,
    And,
    Or,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    column: usize,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let characters = input.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < characters.len() {
        let character = characters[index];
        let column = index + 1;

        if character.is_whitespace() {
            index += 1;
            continue;
        }

        if character.is_alphabetic() || character == '_' || character == '?' {
            let start = index;
            index += 1;
            while index < characters.len()
                && (characters[index].is_alphanumeric()
                    || characters[index] == '_'
                    || characters[index] == '\'')
            {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(characters[start..index].iter().collect()),
                column,
            });
            continue;
        }

        if character.is_ascii_digit()
            || (character == '.' && characters.get(index + 1).is_some_and(char::is_ascii_digit))
        {
            let start = index;
            let mut has_dot = false;
            while index < characters.len() {
                let current = characters[index];
                if current.is_ascii_digit() {
                    index += 1;
                } else if current == '.' && !has_dot {
                    has_dot = true;
                    index += 1;
                } else {
                    break;
                }
            }
            if index < characters.len() && (characters[index] == 'e' || characters[index] == 'E') {
                let mut lookahead = index + 1;
                if lookahead < characters.len()
                    && (characters[lookahead] == '+' || characters[lookahead] == '-')
                {
                    lookahead += 1;
                }
                if lookahead < characters.len() && characters[lookahead].is_ascii_digit() {
                    index = lookahead + 1;
                    while index < characters.len() && characters[index].is_ascii_digit() {
                        index += 1;
                    }
                }
            }
            let text = characters[start..index].iter().collect::<String>();
            let value = text
                .parse::<f64>()
                .ok()
                .and_then(Real::new)
                .ok_or_else(|| ParseError::new(format!("invalid number `{text}`"), column))?;
            tokens.push(Token {
                kind: TokenKind::Number(value),
                column,
            });
            continue;
        }

        let kind = match character {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            ',' => TokenKind::Comma,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '^' => TokenKind::Caret,
            '!' => {
                if characters.get(index + 1) == Some(&'=') {
                    index += 1;
                    TokenKind::NotEq
                } else {
                    TokenKind::Not
                }
            }
            '&' => {
                if characters.get(index + 1) == Some(&'&') {
                    index += 1;
                    TokenKind::And
                } else {
                    return Err(ParseError::new("expected `&&`", column));
                }
            }
            '|' => {
                if characters.get(index + 1) == Some(&'|') {
                    index += 1;
                    TokenKind::Or
                } else {
                    return Err(ParseError::new("expected `||`", column));
                }
            }
            '=' => {
                if characters.get(index + 1) == Some(&'=') {
                    index += 1;
                    TokenKind::EqEq
                } else {
                    return Err(ParseError::new("expected `==`", column));
                }
            }
            '<' => {
                if characters.get(index + 1) == Some(&'=') {
                    index += 1;
                    TokenKind::Le
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if characters.get(index + 1) == Some(&'=') {
                    index += 1;
                    TokenKind::Ge
                } else {
                    TokenKind::Gt
                }
            }
            _ => {
                return Err(ParseError::new(
                    format!("unexpected character `{character}` in condition"),
                    column,
                ));
            }
        };
        tokens.push(Token { kind, column });
        index += 1;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        column: characters.len() + 1,
    });
    Ok(tokens)
}

struct ConditionParser {
    tokens: Vec<Token>,
    position: usize,
}

impl ConditionParser {
    fn new(input: &str) -> Result<Self, ParseError> {
        Ok(Self {
            tokens: tokenize(input)?,
            position: 0,
        })
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.position].kind
    }

    fn column(&self) -> usize {
        self.tokens[self.position].column
    }

    fn advance(&mut self) -> TokenKind {
        let kind = self.tokens[self.position].kind.clone();
        if self.position + 1 < self.tokens.len() {
            self.position += 1;
        }
        kind
    }

    fn expect(&mut self, expected: &TokenKind, description: &str) -> Result<(), ParseError> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(ParseError::new(
                format!("expected {description}, found {}", describe(self.peek())),
                self.column(),
            ))
        }
    }

    fn expect_eof(&mut self) -> Result<(), ParseError> {
        if matches!(self.peek(), TokenKind::Eof) {
            Ok(())
        } else {
            Err(ParseError::new(
                format!("unexpected trailing {}", describe(self.peek())),
                self.column(),
            ))
        }
    }

    fn parse_or(&mut self) -> Result<ConditionExpr, ParseError> {
        let mut parts = vec![self.parse_and()?];
        while self.is_or() {
            self.advance();
            parts.push(self.parse_and()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            ConditionExpr::Or(parts)
        })
    }

    fn parse_and(&mut self) -> Result<ConditionExpr, ParseError> {
        let mut parts = vec![self.parse_unary()?];
        while self.is_and() {
            self.advance();
            parts.push(self.parse_unary()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().unwrap()
        } else {
            ConditionExpr::And(parts)
        })
    }

    fn parse_unary(&mut self) -> Result<ConditionExpr, ParseError> {
        if self.is_not() {
            self.advance();
            return Ok(ConditionExpr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<ConditionExpr, ParseError> {
        let column = self.column();

        // A parenthesised expression can either be a boolean group or the
        // left side of an arithmetic comparison.  Try the value expression
        // first and backtrack if it is not followed by a comparison operator.
        if matches!(self.peek(), TokenKind::LParen) {
            let saved = self.position;
            if let Ok(left) = self.parse_value()
                && let Some(operator) = CompareOp::from_token(self.peek())
            {
                self.advance();
                let right = self.parse_value()?;
                return Ok(ConditionExpr::Compare(left, operator, right));
            }
            self.position = saved;
            self.advance();
            let expression = self.parse_or()?;
            self.expect(&TokenKind::RParen, "`)`")?;
            return Ok(expression);
        }

        match self.peek().clone() {
            TokenKind::Ident(_) | TokenKind::Number(_) | TokenKind::Minus => {
                let left = self.parse_value()?;
                let Some(operator) = CompareOp::from_token(self.peek()) else {
                    return Err(ParseError::new(
                        "expected a comparison operator",
                        self.column(),
                    ));
                };
                self.advance();
                let right = self.parse_value()?;
                Ok(ConditionExpr::Compare(left, operator, right))
            }
            other => Err(ParseError::new(
                format!("expected a condition, found {}", describe(&other)),
                column,
            )),
        }
    }

    fn parse_value(&mut self) -> Result<ValueExpr, ParseError> {
        self.parse_sum()
    }

    fn parse_sum(&mut self) -> Result<ValueExpr, ParseError> {
        let mut left = self.parse_product()?;
        loop {
            let operator = match self.peek() {
                TokenKind::Plus => ValueOp::Add,
                TokenKind::Minus => ValueOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_product()?;
            left = ValueExpr::Binary(operator, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_product(&mut self) -> Result<ValueExpr, ParseError> {
        let mut left = self.parse_power()?;
        loop {
            let operator = match self.peek() {
                TokenKind::Star => ValueOp::Mul,
                TokenKind::Slash => ValueOp::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_power()?;
            left = ValueExpr::Binary(operator, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<ValueExpr, ParseError> {
        let left = self.parse_value_unary()?;
        if matches!(self.peek(), TokenKind::Caret) {
            self.advance();
            let right = self.parse_power()?;
            return Ok(ValueExpr::Binary(
                ValueOp::Pow,
                Box::new(left),
                Box::new(right),
            ));
        }
        Ok(left)
    }

    fn parse_value_unary(&mut self) -> Result<ValueExpr, ParseError> {
        if matches!(self.peek(), TokenKind::Minus) {
            self.advance();
            return Ok(ValueExpr::Neg(Box::new(self.parse_value_unary()?)));
        }
        self.parse_value_primary()
    }

    fn parse_value_primary(&mut self) -> Result<ValueExpr, ParseError> {
        let column = self.column();
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(ValueExpr::Var(variable_for(&name)))
            }
            TokenKind::Number(value) => {
                self.advance();
                Ok(ValueExpr::Const(Constant::Real(value)))
            }
            TokenKind::LParen => {
                self.advance();
                let value = self.parse_value()?;
                self.expect(&TokenKind::RParen, "`)`")?;
                Ok(value)
            }
            other => Err(ParseError::new(
                format!("expected a condition value, found {}", describe(&other)),
                column,
            )),
        }
    }

    fn is_or(&self) -> bool {
        matches!(self.peek(), TokenKind::Or)
            || matches!(self.peek(), TokenKind::Ident(name) if name == "or")
    }

    fn is_and(&self) -> bool {
        matches!(self.peek(), TokenKind::And)
            || matches!(self.peek(), TokenKind::Ident(name) if name == "and")
    }

    fn is_not(&self) -> bool {
        matches!(self.peek(), TokenKind::Not)
            || matches!(self.peek(), TokenKind::Ident(name) if name == "not")
    }
}

fn variable_for(name: &str) -> Var {
    let name = if name.starts_with('?') && name.len() > 1 {
        name.to_string()
    } else {
        format!("?{name}")
    };
    name.parse::<Var>().expect("condition variable is valid")
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Ident(name) => format!("identifier `{name}`"),
        TokenKind::Number(value) => format!("number `{value}`"),
        TokenKind::LParen => "`(`".to_string(),
        TokenKind::RParen => "`)`".to_string(),
        TokenKind::Comma => "`,`".to_string(),
        TokenKind::Plus => "`+`".to_string(),
        TokenKind::Minus => "`-`".to_string(),
        TokenKind::Star => "`*`".to_string(),
        TokenKind::Slash => "`/`".to_string(),
        TokenKind::Caret => "`^`".to_string(),
        TokenKind::Not => "`!`".to_string(),
        TokenKind::And => "`&&`".to_string(),
        TokenKind::Or => "`||`".to_string(),
        TokenKind::EqEq => "`==`".to_string(),
        TokenKind::NotEq => "`!=`".to_string(),
        TokenKind::Lt => "`<`".to_string(),
        TokenKind::Le => "`<=`".to_string(),
        TokenKind::Gt => "`>`".to_string(),
        TokenKind::Ge => "`>=`".to_string(),
        TokenKind::Eof => "end of condition".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_arithmetic_values() {
        let condition = ConditionExpr::parse("a ^ 2 + b * 2 <= 10").unwrap();
        assert_eq!(condition.collect_vars().len(), 2);
        assert_eq!(condition.to_string(), "a^2 + b * 2 <= 10");
    }

    #[test]
    fn parses_parenthesised_arithmetic_comparisons() {
        let condition = ConditionExpr::parse("(a + 1) * 2 > 3").unwrap();
        assert_eq!(condition.to_string(), "(a + 1) * 2 > 3");
    }
}
