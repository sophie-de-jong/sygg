//! A small symbolic algebra system built on top of egg.
//!
//! The public API is intentionally thin: [`AlgebraEngine`] owns the loaded
//! rewrite rules and variable bindings, and [`AlgebraEngine::evaluate`] returns
//! several cost-based simplifications of an input expression.

pub mod analysis;
pub mod ast;
pub mod condition;
pub mod constant;
pub mod engine;
pub mod error;
pub mod functions;
pub mod language;
pub mod rewrite;

pub use analysis::{ConstantFolding, MathEGraph, MathRewrite};
pub use ast::{Ast, parse_expression, parse_let, pretty_ast, pretty_ast_exact, pretty_recexpr};
pub use condition::ConditionExpr;
pub use constant::{Constant, Matrix, Real};
pub use egg::StopReason;
pub use engine::{AlgebraEngine, CostKind, EngineConfig, StrategyResult, VarDef};
pub use error::{AlgebraError, ParseError, Result};
pub use language::Math;
pub use rewrite::Rule;
