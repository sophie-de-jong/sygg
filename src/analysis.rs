//! Constant folding as an egg analysis.
//!
//! The analysis stores the constant value (if any) represented by each
//! e-class.  When a new e-node is added, `make` inspects the analysis data of
//! its children and computes a value; `modify` then inserts the corresponding
//! constant e-node and unions it into the e-class.  This is the idiomatic egg
//! way to perform constant folding and makes the result available to normal
//! rewrite rules and extraction.

use egg::{Analysis, DidMerge, EGraph, Id, Language, Rewrite, merge_option};

use crate::constant::{Constant, Matrix, Real};
use crate::functions::Function;
use crate::language::Math;

pub type MathEGraph = EGraph<Math, ConstantFolding>;
pub type MathRewrite = Rewrite<Math, ConstantFolding>;

#[derive(Clone, Debug, Default)]
pub struct AlgebraData {
    pub constant: Option<Constant>,
    /// Maximum AST depth represented by this e-class.  Using an upper bound
    /// makes the rewrite depth limit conservative: a rule is rejected if any
    /// representative of a child class could make the result too deep.
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct ConstantFolding {
    depth_cap: usize,
}

impl Default for ConstantFolding {
    fn default() -> Self {
        Self { depth_cap: 64 }
    }
}

impl ConstantFolding {
    pub fn new(depth_cap: usize) -> Self {
        Self {
            depth_cap: depth_cap.max(1),
        }
    }

    fn constant_of(egraph: &MathEGraph, id: Id) -> Option<Constant> {
        egraph[id].data.constant.clone()
    }

    fn depth_of(egraph: &MathEGraph, id: Id) -> usize {
        egraph[id].data.depth
    }
}

impl Analysis<Math> for ConstantFolding {
    type Data = AlgebraData;

    fn make(egraph: &mut MathEGraph, enode: &Math, _id: Id) -> Self::Data {
        let child_constant = |id: &Id| Self::constant_of(egraph, *id);
        let depth = (1 + enode
            .children()
            .iter()
            .map(|id| Self::depth_of(egraph, *id))
            .max()
            .unwrap_or(0))
        .min(egraph.analysis.depth_cap);

        let constant = (|| -> Option<Constant> {
            match enode {
                Math::Const(value) => Some(value.clone()),
                Math::Symbol(_) => None,
                Math::Add([a, b]) => child_constant(a)?.add(&child_constant(b)?),
                Math::Sub([a, b]) => child_constant(a)?.sub(&child_constant(b)?),
                Math::Mul([a, b]) => child_constant(a)?.mul(&child_constant(b)?),
                Math::Div([a, b]) => child_constant(a)?.div(&child_constant(b)?),
                Math::Pow([a, b]) => child_constant(a)?.pow(&child_constant(b)?),
                Math::Neg(inner) => child_constant(inner)?.neg(),
                Math::Call(name, arguments) => {
                    let function: Function = name.as_str().parse().ok()?;
                    let arguments = arguments
                        .iter()
                        .map(child_constant)
                        .collect::<Option<Vec<_>>>()?;
                    function.eval(&arguments)
                }
                Math::VecLit(items) => {
                    let values = items
                        .iter()
                        .map(child_constant)
                        .collect::<Option<Vec<_>>>()?;
                    constant_from_values(values)
                }
            }
        })();

        AlgebraData { constant, depth }
    }

    fn merge(&mut self, to: &mut Self::Data, from: Self::Data) -> DidMerge {
        let constant_merge = merge_option(&mut to.constant, from.constant, |existing, new| {
            if *existing == new {
                DidMerge(false, false)
            } else if new < *existing {
                *existing = new;
                DidMerge(true, true)
            } else {
                DidMerge(false, true)
            }
        });

        let merged_depth = to.depth.max(from.depth).min(self.depth_cap);
        let depth_changed = merged_depth > to.depth;
        if depth_changed {
            to.depth = merged_depth;
        }

        constant_merge | DidMerge(depth_changed, false)
    }

    fn modify(egraph: &mut MathEGraph, id: Id) {
        // Do not assert that a class containing a free symbol is constant.
        // A class can acquire a stale or cyclic constant during saturation;
        // folding that value into the class would make unrelated symbolic
        // terms equal (for example x = x * 0.5) and collapse the e-graph.
        if egraph[id]
            .nodes
            .iter()
            .any(|node| matches!(node, Math::Symbol(_)))
        {
            return;
        }
        if let Some(value) = egraph[id].data.constant.clone() {
            // Floating-point evaluation and saturation can make a class
            // contain mathematically equal expressions that rounded to
            // different values, or can underflow a very small value to zero.
            // Never merge distinct constants into one e-class: doing so
            // would make the class simultaneously equal to several numbers
            // (for example an underflowed zero would make `x * 0.5` zero).
            let has_different_constant = egraph[id]
                .nodes
                .iter()
                .any(|node| matches!(node, Math::Const(existing) if existing != &value));
            if has_different_constant {
                return;
            }
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
                .constant
                .as_ref()
                .unwrap()
                .to_string(),
            "7"
        );
    }
}
