//! Hand-written lexer, precedence-climbing Pratt parser and pretty printer.

use egg::{Id, RecExpr};

use crate::constant::{Constant, Matrix, Real};
use crate::error::ParseError;
use crate::functions::Function;
use crate::language::Math;

/// A parsed expression.  This is deliberately independent of egg's
/// `RecExpr` so rewrite rules can treat bare identifiers as pattern
/// variables while ordinary expressions treat them as symbols.
#[derive(Clone, Debug, PartialEq)]
pub enum Ast {
    Const(Constant),
    Symbol(String),
    Add(Box<Ast>, Box<Ast>),
    Sub(Box<Ast>, Box<Ast>),
    Mul(Box<Ast>, Box<Ast>),
    Div(Box<Ast>, Box<Ast>),
    Pow(Box<Ast>, Box<Ast>),
    Neg(Box<Ast>),
    Call(String, Vec<Ast>),
    Vec(Vec<Ast>),
}

impl Ast {
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Ast::Symbol(name) => Some(name),
            _ => None,
        }
    }

    pub fn contains_symbol(&self, needle: &str) -> bool {
        match self {
            Ast::Const(_) => false,
            Ast::Symbol(name) => name == needle,
            Ast::Add(a, b) | Ast::Sub(a, b) | Ast::Mul(a, b) | Ast::Div(a, b) | Ast::Pow(a, b) => {
                a.contains_symbol(needle) || b.contains_symbol(needle)
            }
            Ast::Neg(inner) => inner.contains_symbol(needle),
            Ast::Call(_, args) | Ast::Vec(args) => {
                args.iter().any(|arg| arg.contains_symbol(needle))
            }
        }
    }

    pub fn symbols(&self, output: &mut Vec<String>) {
        match self {
            Ast::Const(_) => {}
            Ast::Symbol(name) => {
                if !output.contains(name) {
                    output.push(name.clone());
                }
            }
            Ast::Add(a, b) | Ast::Sub(a, b) | Ast::Mul(a, b) | Ast::Div(a, b) | Ast::Pow(a, b) => {
                a.symbols(output);
                b.symbols(output);
            }
            Ast::Neg(inner) => inner.symbols(output),
            Ast::Call(_, args) | Ast::Vec(args) => {
                for arg in args {
                    arg.symbols(output);
                }
            }
        }
    }

    /// Convert an AST into an egg recursive expression.
    pub fn to_recexpr(&self) -> RecExpr<Math> {
        let mut expression = RecExpr::default();
        add_ast(&mut expression, self);
        expression
    }

    fn vec_literal(items: Vec<Ast>) -> Ast {
        match constant_from_items(&items) {
            Some(value) => Ast::Const(value),
            None => Ast::Vec(items),
        }
    }
}

fn constant_from_items(items: &[Ast]) -> Option<Constant> {
    if items.is_empty() {
        return Some(Constant::Vector(Vec::new()));
    }

    let values = items
        .iter()
        .map(|item| match item {
            Ast::Const(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    if values
        .iter()
        .all(|value| matches!(value, Constant::Real(_)))
    {
        return Some(Constant::Vector(
            values
                .into_iter()
                .map(|value| match value {
                    Constant::Real(value) => value,
                    _ => unreachable!(),
                })
                .collect(),
        ));
    }

    if values
        .iter()
        .all(|value| matches!(value, Constant::Vector(_)))
    {
        let rows = values
            .into_iter()
            .map(|value| match value {
                Constant::Vector(row) => row,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        return Matrix::from_rows(rows).map(Constant::Matrix);
    }

    None
}

/// Parse an expression from text.
pub fn parse_expression(input: &str) -> Result<Ast, ParseError> {
    let mut parser = Parser::new(input)?;
    let ast = parser.parse_expr(0)?;
    parser.expect_eof()?;
    Ok(ast)
}

/// Parse a `let name = expression` line.
pub fn parse_let(input: &str) -> Result<(String, Ast), ParseError> {
    let mut parser = Parser::new(input)?;
    let column = parser.column();
    let keyword = parser.take_identifier("`let`")?;
    if keyword != "let" {
        return Err(ParseError::new("expected `let`", column));
    }
    let name = parser.take_identifier("a variable name")?;
    if name.starts_with('?') {
        return Err(ParseError::new(
            "variable names cannot start with `?`",
            column,
        ));
    }
    parser.expect(TokenKind::Eq, "`=`")?;
    let value = parser.parse_expr(0)?;
    parser.expect_eof()?;
    Ok((name, value))
}

/// Returns true when the input's first word is `let`.
pub fn starts_with_let(input: &str) -> bool {
    input
        .split_whitespace()
        .next()
        .is_some_and(|word| word == "let")
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Number(Real),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Eq,
    Arrow,
    Iff,
    Semi,
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    column: usize,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let ch = chars[index];
        let column = index + 1;

        if ch.is_whitespace() {
            index += 1;
            continue;
        }

        if ch == '#' || (ch == '/' && chars.get(index + 1) == Some(&'/')) {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }

        if ch.is_ascii_digit()
            || (ch == '.' && chars.get(index + 1).is_some_and(char::is_ascii_digit))
        {
            let start = index;
            let mut has_dot = false;
            while index < chars.len() {
                let current = chars[index];
                if current.is_ascii_digit() {
                    index += 1;
                } else if current == '.' && !has_dot {
                    has_dot = true;
                    index += 1;
                } else {
                    break;
                }
            }

            if index < chars.len() && (chars[index] == 'e' || chars[index] == 'E') {
                let mut lookahead = index + 1;
                if lookahead < chars.len() && (chars[lookahead] == '+' || chars[lookahead] == '-') {
                    lookahead += 1;
                }
                if lookahead < chars.len() && chars[lookahead].is_ascii_digit() {
                    index = lookahead + 1;
                    while index < chars.len() && chars[index].is_ascii_digit() {
                        index += 1;
                    }
                }
            }

            let text = chars[start..index].iter().collect::<String>();
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

        if ch.is_alphabetic() || ch == '_' || ch == '?' {
            let start = index;
            index += 1;
            while index < chars.len() {
                let current = chars[index];
                if current.is_alphanumeric() || current == '_' || current == '\'' {
                    index += 1;
                } else {
                    break;
                }
            }
            let text = chars[start..index].iter().collect::<String>();
            tokens.push(Token {
                kind: TokenKind::Ident(text),
                column,
            });
            continue;
        }

        let kind = match ch {
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '^' => TokenKind::Caret,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semi,
            '=' => {
                if chars.get(index + 1) == Some(&'>') {
                    index += 1;
                    TokenKind::Arrow
                } else {
                    TokenKind::Eq
                }
            }
            '<' => {
                if chars.get(index + 1) == Some(&'=') && chars.get(index + 2) == Some(&'>') {
                    index += 2;
                    TokenKind::Iff
                } else {
                    return Err(ParseError::new(
                        "unexpected `<`; rewrite arrows are `=>` or `<=>`",
                        column,
                    ));
                }
            }
            _ => {
                return Err(ParseError::new(
                    format!("unexpected character `{ch}`"),
                    column,
                ));
            }
        };

        tokens.push(Token { kind, column });
        index += 1;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        column: chars.len() + 1,
    });
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn new(input: &str) -> Result<Self, ParseError> {
        Ok(Self {
            tokens: tokenize(input)?,
            position: 0,
        })
    }

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.position].kind
    }

    fn peek_kind(&self) -> &TokenKind {
        self.peek()
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

    fn expect(&mut self, expected: TokenKind, description: &str) -> Result<(), ParseError> {
        if self.peek_kind() == &expected {
            self.advance();
            Ok(())
        } else {
            Err(ParseError::new(
                format!(
                    "expected {description}, found {}",
                    describe(self.peek_kind())
                ),
                self.column(),
            ))
        }
    }

    fn expect_eof(&mut self) -> Result<(), ParseError> {
        if matches!(self.peek_kind(), TokenKind::Eof) {
            Ok(())
        } else {
            Err(ParseError::new(
                format!("unexpected trailing {}", describe(self.peek_kind())),
                self.column(),
            ))
        }
    }

    fn take_identifier(&mut self, description: &str) -> Result<String, ParseError> {
        match self.peek_kind().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(name)
            }
            other => Err(ParseError::new(
                format!("expected {description}, found {}", describe(&other)),
                self.column(),
            )),
        }
    }

    fn parse_expr(&mut self, min_binding_power: u8) -> Result<Ast, ParseError> {
        let mut left = self.parse_prefix()?;

        while let Some((left_bp, right_bp)) = infix_binding_power(self.peek_kind()) {
            if left_bp < min_binding_power {
                break;
            }

            let operator = self.advance();
            let right = self.parse_expr(right_bp)?;
            left = match operator {
                TokenKind::Plus => Ast::Add(Box::new(left), Box::new(right)),
                TokenKind::Minus => Ast::Sub(Box::new(left), Box::new(right)),
                TokenKind::Star => Ast::Mul(Box::new(left), Box::new(right)),
                TokenKind::Slash => Ast::Div(Box::new(left), Box::new(right)),
                TokenKind::Caret => Ast::Pow(Box::new(left), Box::new(right)),
                _ => unreachable!(),
            };
        }

        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Ast, ParseError> {
        let column = self.column();
        match self.peek_kind().clone() {
            TokenKind::Number(value) => {
                self.advance();
                Ok(Ast::Const(Constant::Real(value)))
            }
            TokenKind::Ident(name) => {
                self.advance();
                if matches!(self.peek_kind(), TokenKind::LParen) {
                    self.advance();
                    let arguments = self.parse_comma_list(TokenKind::RParen)?;
                    if let Ok(function) = name.parse::<Function>()
                        && !function.accepts(arguments.len())
                    {
                        return Err(ParseError::new(
                            format!("`{}` expects {}", function.name(), function.arity_text()),
                            column,
                        ));
                    }
                    Ok(Ast::Call(name, arguments))
                } else {
                    Ok(Ast::Symbol(name))
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr(0)?;
                self.expect(TokenKind::RParen, "`)`")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                self.advance();
                let items = self.parse_comma_list(TokenKind::RBracket)?;
                Ok(Ast::vec_literal(items))
            }
            TokenKind::Minus => {
                self.advance();
                let operand = self.parse_expr(25)?;
                Ok(Ast::Neg(Box::new(operand)))
            }
            TokenKind::Plus => {
                self.advance();
                self.parse_expr(25)
            }
            TokenKind::Eof => Err(ParseError::new("unexpected end of input", self.column())),
            other => Err(ParseError::new(
                format!("expected an expression, found {}", describe(&other)),
                column,
            )),
        }
    }

    fn parse_comma_list(&mut self, closing: TokenKind) -> Result<Vec<Ast>, ParseError> {
        let mut items = Vec::new();
        if self.peek_kind() == &closing {
            self.advance();
            return Ok(items);
        }

        loop {
            items.push(self.parse_expr(0)?);
            if self.peek_kind() == &closing {
                self.advance();
                break;
            }
            self.expect(TokenKind::Comma, "`,` or closing bracket")?;
            if self.peek_kind() == &closing {
                self.advance();
                break;
            }
        }
        Ok(items)
    }
}

fn infix_binding_power(kind: &TokenKind) -> Option<(u8, u8)> {
    match kind {
        TokenKind::Plus | TokenKind::Minus => Some((10, 11)),
        TokenKind::Star | TokenKind::Slash => Some((20, 21)),
        TokenKind::Caret => Some((30, 30)),
        _ => None,
    }
}

fn describe(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Number(value) => format!("number `{value}`"),
        TokenKind::Ident(name) => format!("identifier `{name}`"),
        TokenKind::Plus => "`+`".to_string(),
        TokenKind::Minus => "`-`".to_string(),
        TokenKind::Star => "`*`".to_string(),
        TokenKind::Slash => "`/`".to_string(),
        TokenKind::Caret => "`^`".to_string(),
        TokenKind::LParen => "`(`".to_string(),
        TokenKind::RParen => "`)`".to_string(),
        TokenKind::LBracket => "`[`".to_string(),
        TokenKind::RBracket => "`]`".to_string(),
        TokenKind::Comma => "`,`".to_string(),
        TokenKind::Colon => "`:`".to_string(),
        TokenKind::Eq => "`=`".to_string(),
        TokenKind::Arrow => "`=>`".to_string(),
        TokenKind::Iff => "`<=>`".to_string(),
        TokenKind::Semi => "`;`".to_string(),
        TokenKind::Eof => "end of input".to_string(),
    }
}

fn add_ast(expression: &mut RecExpr<Math>, ast: &Ast) -> Id {
    match ast {
        Ast::Const(value) => expression.add(Math::Const(value.clone())),
        Ast::Symbol(name) => expression.add(Math::Symbol(name.as_str().into())),
        Ast::Add(a, b) => {
            let a = add_ast(expression, a);
            let b = add_ast(expression, b);
            expression.add(Math::Add([a, b]))
        }
        Ast::Sub(a, b) => {
            let a = add_ast(expression, a);
            let b = add_ast(expression, b);
            expression.add(Math::Sub([a, b]))
        }
        Ast::Mul(a, b) => {
            let a = add_ast(expression, a);
            let b = add_ast(expression, b);
            expression.add(Math::Mul([a, b]))
        }
        Ast::Div(a, b) => {
            let a = add_ast(expression, a);
            let b = add_ast(expression, b);
            expression.add(Math::Div([a, b]))
        }
        Ast::Pow(a, b) => {
            let a = add_ast(expression, a);
            let b = add_ast(expression, b);
            expression.add(Math::Pow([a, b]))
        }
        Ast::Neg(inner) => {
            let inner = add_ast(expression, inner);
            expression.add(Math::Neg(inner))
        }
        Ast::Call(name, args) => {
            let ids = args
                .iter()
                .map(|arg| add_ast(expression, arg))
                .collect::<Vec<_>>();
            expression.add(Math::Call(name.as_str().into(), ids))
        }
        Ast::Vec(items) => {
            if let Some(value) = constant_from_items(items) {
                expression.add(Math::Const(value))
            } else {
                let ids = items
                    .iter()
                    .map(|item| add_ast(expression, item))
                    .collect::<Vec<_>>();
                expression.add(Math::VecLit(ids.into_boxed_slice()))
            }
        }
    }
}

/// Pretty print an AST with minimal parentheses.
pub fn pretty_ast(ast: &Ast) -> String {
    pretty_recexpr(&ast.to_recexpr())
}

/// Pretty print a recursive expression with minimal parentheses.
pub fn pretty_recexpr(expression: &RecExpr<Math>) -> String {
    let mut output = String::new();
    write_node(expression, expression.root(), &mut output);
    output
}

/// Pretty print an AST while preserving the exact tree grouping.  This is used
/// for rule listings, where `(a + b) + c` and `a + (b + c)` are different
/// patterns even though they print identically with minimal parentheses.
pub fn pretty_ast_exact(ast: &Ast) -> String {
    let expression = ast.to_recexpr();
    let mut output = String::new();
    write_node_exact(&expression, expression.root(), &mut output);
    output
}

fn write_node_exact(expression: &RecExpr<Math>, id: Id, output: &mut String) {
    let node = &expression[id];
    match node {
        Math::Const(value) => output.push_str(&value.to_string()),
        Math::Symbol(name) => output.push_str(name.as_str()),
        Math::Add([a, b]) => {
            write_child_exact(expression, *a, output);
            output.push_str(" + ");
            write_child_exact(expression, *b, output);
        }
        Math::Sub([a, b]) => {
            write_child_exact(expression, *a, output);
            output.push_str(" - ");
            write_child_exact(expression, *b, output);
        }
        Math::Mul([a, b]) => {
            write_child_exact(expression, *a, output);
            output.push_str(" * ");
            write_child_exact(expression, *b, output);
        }
        Math::Div([a, b]) => {
            write_child_exact(expression, *a, output);
            output.push_str(" / ");
            write_child_exact(expression, *b, output);
        }
        Math::Pow([a, b]) => {
            write_child_exact(expression, *a, output);
            output.push('^');
            write_child_exact(expression, *b, output);
        }
        Math::Neg(inner) => {
            output.push('-');
            write_child_exact(expression, *inner, output);
        }
        Math::VecLit(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_node_exact(expression, *item, output);
            }
            output.push(']');
        }
        Math::Call(name, arguments) => {
            output.push_str(name.as_str());
            output.push('(');
            for (index, argument) in arguments.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_node_exact(expression, *argument, output);
            }
            output.push(')');
        }
    }
}

fn write_child_exact(expression: &RecExpr<Math>, id: Id, output: &mut String) {
    let compound = !matches!(
        &expression[id],
        Math::Const(_) | Math::Symbol(_) | Math::Call(_, _) | Math::VecLit(_)
    );
    if compound {
        output.push('(');
    }
    write_node_exact(expression, id, output);
    if compound {
        output.push(')');
    }
}

#[derive(Clone, Copy)]
enum Context {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Neg,
}

fn node_precedence(node: &Math) -> u8 {
    match node {
        Math::Add(_) | Math::Sub(_) => 1,
        Math::Mul(_) | Math::Div(_) => 2,
        Math::Neg(_) => 3,
        Math::Pow(_) => 4,
        Math::Const(Constant::Real(value)) if value.get() < 0.0 => 3,
        _ => 10,
    }
}

fn needs_parentheses(parent: Context, child: &Math, right: bool) -> bool {
    let child_precedence = node_precedence(child);
    match parent {
        Context::Add => false,
        Context::Sub => right && child_precedence <= 1,
        Context::Mul => {
            if right {
                child_precedence < 2 || matches!(child, Math::Div(_))
            } else {
                child_precedence < 2
            }
        }
        Context::Div => {
            if right {
                child_precedence <= 2
            } else {
                child_precedence < 2
            }
        }
        Context::Pow => {
            if right {
                child_precedence < 4
            } else {
                child_precedence < 4 || matches!(child, Math::Pow(_))
            }
        }
        Context::Neg => child_precedence <= 3,
    }
}

fn write_node(expression: &RecExpr<Math>, id: Id, output: &mut String) {
    let node = &expression[id];
    match node {
        Math::Const(value) => output.push_str(&value.to_string()),
        Math::Symbol(name) => output.push_str(name.as_str()),
        Math::Add([a, b]) => {
            if let Some(magnitude) = negative_real_magnitude(&expression[*b]) {
                write_child(expression, *a, Context::Add, false, output);
                output.push_str(" - ");
                output.push_str(&magnitude);
            } else if let Math::Neg(inner) = &expression[*b] {
                write_child(expression, *a, Context::Add, false, output);
                output.push_str(" - ");
                write_child(expression, *inner, Context::Sub, true, output);
            } else {
                write_child(expression, *a, Context::Add, false, output);
                output.push_str(" + ");
                write_child(expression, *b, Context::Add, true, output);
            }
        }
        Math::Sub([a, b]) => {
            if let Some(magnitude) = negative_real_magnitude(&expression[*b]) {
                write_child(expression, *a, Context::Sub, false, output);
                output.push_str(" + ");
                output.push_str(&magnitude);
            } else if let Math::Neg(inner) = &expression[*b] {
                write_child(expression, *a, Context::Sub, false, output);
                output.push_str(" + ");
                write_child(expression, *inner, Context::Add, true, output);
            } else {
                write_child(expression, *a, Context::Sub, false, output);
                output.push_str(" - ");
                write_child(expression, *b, Context::Sub, true, output);
            }
        }
        Math::Mul(_) => {
            let mut factors = Vec::new();
            collect_mul_factors(expression, id, &mut factors);

            // Put constant factors first for the conventional `2 * x`
            // spelling.  The relative order of the remaining factors is
            // preserved.
            let (constants, others): (Vec<Id>, Vec<Id>) = factors
                .into_iter()
                .partition(|factor| matches!(expression[*factor], Math::Const(_)));
            for (index, factor) in constants.into_iter().chain(others).enumerate() {
                if index > 0 {
                    output.push_str(" * ");
                }
                write_child(expression, factor, Context::Mul, index > 0, output);
            }
        }
        Math::Div([a, b]) => {
            write_child(expression, *a, Context::Div, false, output);
            output.push_str(" / ");
            write_child(expression, *b, Context::Div, true, output);
        }
        Math::Pow([a, b]) => {
            write_child(expression, *a, Context::Pow, false, output);
            output.push('^');
            write_child(expression, *b, Context::Pow, true, output);
        }
        Math::Neg(inner) => {
            output.push('-');
            write_child(expression, *inner, Context::Neg, false, output);
        }
        Math::VecLit(items) => {
            output.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_node(expression, *item, output);
            }
            output.push(']');
        }
        Math::Call(name, args) => {
            output.push_str(name.as_str());
            output.push('(');
            for (index, argument) in args.iter().enumerate() {
                if index > 0 {
                    output.push_str(", ");
                }
                write_node(expression, *argument, output);
            }
            output.push(')');
        }
    }
}

fn collect_mul_factors(expression: &RecExpr<Math>, id: Id, factors: &mut Vec<Id>) {
    match &expression[id] {
        Math::Mul([a, b]) => {
            collect_mul_factors(expression, *a, factors);
            collect_mul_factors(expression, *b, factors);
        }
        _ => factors.push(id),
    }
}

fn write_child(
    expression: &RecExpr<Math>,
    id: Id,
    parent: Context,
    right: bool,
    output: &mut String,
) {
    let child = &expression[id];
    let parentheses = needs_parentheses(parent, child, right);
    if parentheses {
        output.push('(');
    }
    write_node(expression, id, output);
    if parentheses {
        output.push(')');
    }
}

fn negative_real_magnitude(node: &Math) -> Option<String> {
    match node {
        Math::Const(Constant::Real(value)) if value.get() < 0.0 => {
            Real::new(-value.get()).map(|magnitude| magnitude.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pretty(input: &str) -> String {
        pretty_ast(&parse_expression(input).unwrap())
    }

    #[test]
    fn respects_precedence() {
        assert_eq!(pretty("1+2*3"), "1 + 2 * 3");
        assert_eq!(pretty("(1+2)*3"), "3 * (1 + 2)");
        assert_eq!(pretty("1-(2+3)"), "1 - (2 + 3)");
        assert_eq!(pretty("1-2-3"), "1 - 2 - 3");
        assert_eq!(pretty("1-(2-3)"), "1 - (2 - 3)");
        assert_eq!(pretty("a/(b*c)"), "a / (b * c)");
        assert_eq!(pretty("a*(b/c)"), "a * (b / c)");
        assert_eq!(pretty("a*b/c"), "a * b / c");
        assert_eq!(pretty("a/(b/c)"), "a / (b / c)");
        assert_eq!(pretty("a^(b^c)"), "a^b^c");
        assert_eq!(pretty("(a^b)^c"), "(a^b)^c");
        assert_eq!(pretty("-a^2"), "-a^2");
        assert_eq!(pretty("(-a)^2"), "(-a)^2");
        assert_eq!(pretty("a^-2"), "a^(-2)");
        assert_eq!(pretty("a+(-b)"), "a - b");
        assert_eq!(pretty("a-(-b)"), "a + b");
    }

    #[test]
    fn exact_printing_preserves_rule_grouping() {
        assert_eq!(
            pretty_ast_exact(&parse_expression("(a + b) + c").unwrap()),
            "(a + b) + c"
        );
        assert_eq!(
            pretty_ast_exact(&parse_expression("a + (b + c)").unwrap()),
            "a + (b + c)"
        );
        assert_eq!(
            pretty_ast_exact(&parse_expression("(a * b) * c").unwrap()),
            "(a * b) * c"
        );
        assert_eq!(
            pretty_ast_exact(&parse_expression("a * (b * c)").unwrap()),
            "a * (b * c)"
        );
    }

    #[test]
    fn parses_vectors_and_matrices() {
        assert_eq!(pretty("[1, 2, 3]"), "[1, 2, 3]");
        assert_eq!(pretty("[[1, 2], [3, 4]]"), "[[1, 2], [3, 4]]");
        assert_eq!(pretty("[x, y+1]"), "[x, y + 1]");
    }

    #[test]
    fn parses_functions() {
        assert_eq!(pretty("sin(x)^2 + cos(x)^2"), "sin(x)^2 + cos(x)^2");
        assert_eq!(pretty("log(8, 2)"), "log(8, 2)");
    }

    #[test]
    fn reports_arity_errors() {
        let error = parse_expression("sin(1, 2)").unwrap_err();
        assert!(error.message.contains("expects 1 argument"));
    }

    #[test]
    fn parses_let() {
        let (name, value) = parse_let("let z = x + y").unwrap();
        assert_eq!(name, "z");
        assert_eq!(pretty_ast(&value), "x + y");
    }

    #[test]
    fn unary_minus_binds_below_exponent() {
        let ast = parse_expression("-2^2").unwrap();
        match ast {
            Ast::Neg(inner) => assert!(matches!(*inner, Ast::Pow(_, _))),
            _ => panic!("expected a negated power"),
        }
    }
}
