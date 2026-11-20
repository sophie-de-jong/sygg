//! The egg language used by the algebra engine.

use crate::constant::Constant;
use egg::{Id, Symbol, define_language};

define_language! {
    /// The expression language shared by expressions and rewrite rules.
    pub enum Math {
        Const(Constant),
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "/" = Div([Id; 2]),
        "^" = Pow([Id; 2]),
        "neg" = Neg(Id),
        "vec" = VecLit(Box<[Id]>),
        Symbol(Symbol),
        Call(Symbol, Vec<Id>),
    }
}
