//! Constant folding as an egg analysis.
//!
//! The analysis stores the constant value (if any) represented by each
//! e-class.  When a new e-node is added, `make` inspects the analysis data of
//! its children and computes a value; `modify` then inserts the corresponding
//! constant e-node and unions it into the e-class.  This is the idiomatic egg
//! way to perform constant folding and makes the result available to normal
//! rewrite rules and extraction.

use egg::{Analysis, DidMerge, EGraph, Id, Rewrite, merge_option};

use crate::constant::{Constant, Matrix, Real};
use crate::functions::Function;
use crate::language::Math;

pub type MathEGraph = EGraph<Math, ConstantFolding>;
pub type MathRewrite = Rewrite<Math, ConstantFolding>;

#[derive(Clone, Default)]
pub struct ConstantFolding;

impl Analysis<Math> for ConstantFolding {
    type Data = Option<Constant>;

    fn make(egraph: &mut MathEGraph, enode: &Math, _id: Id) -> Self::Data {
        let child = |id: &Id| egraph[*id].data.clone();

        match enode {
            Math::Const(value) => Some(value.clone()),
            Math::Symbol(_) => None,
            Math::Add([a, b]) => child(a)?.add(&child(b)?),
            Math::Sub([a, b]) => child(a)?.sub(&child(b)?),
            Math::Mul([a, b]) => child(a)?.mul(&child(b)?),
            Math::Div([a, b]) => child(a)?.div(&child(b)?),
            Math::Pow([a, b]) => child(a)?.pow(&child(b)?),
            Math::Neg(inner) => child(inner)?.neg(),
            Math::Call(name, arguments) => {
                let function: Function = name.as_str().parse().ok()?;
                let arguments = arguments.iter().map(child).collect::<Option<Vec<_>>>()?;
                function.eval(&arguments)
            }
            Math::VecLit(items) => {
                let values = items.iter().map(child).collect::<Option<Vec<_>>>()?;
                constant_from_values(values)
            }
        }
    }

    fn merge(&mut self, to: &mut Self::Data, from: Self::Data) -> DidMerge {
        merge_option(to, from, |existing, new| {
            if *existing == new {
                DidMerge(false, false)
            } else if new < *existing {
                *existing = new;
                DidMerge(true, true)
            } else {
                DidMerge(false, true)
            }
        })
    }

    fn modify(egraph: &mut MathEGraph, id: Id) {
        if let Some(value) = egraph[id].data.clone() {
            let constant_id = egraph.add(Math::Const(value));
            if egraph.find(id) != egraph.find(constant_id) {
                egraph.union(id, constant_id);
            }
        }
    }
}

fn constant_from_values(values: Vec<Constant>) -> Option<Constant> {
    if values.is_empty() {
        return Some(Constant::Vector(Vec::new()));
    }

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
                .collect::<Vec<Real>>(),
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

#[cfg(test)]
mod tests {
    use egg::{Runner, StopReason};

    use super::*;
    use crate::ast::parse_expression;

    #[test]
    fn analysis_folds_constants_without_rules() {
        let expression = parse_expression("1 + 2 * 3").unwrap().to_recexpr();
        let runner = Runner::<Math, ConstantFolding, ()>::default()
            .with_expr(&expression)
            .run(&[]);
        assert!(matches!(runner.stop_reason, Some(StopReason::Saturated)));
        assert_eq!(
            runner.egraph[runner.roots[0]]
                .data
                .as_ref()
                .unwrap()
                .to_string(),
            "7"
        );
    }
}
