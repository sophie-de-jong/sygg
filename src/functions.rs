//! Built-in elementary functions and their constant folding behaviour.

use std::str::FromStr;

use crate::constant::{Constant, Real};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sinh,
    Cosh,
    Tanh,
    Exp,
    Ln,
    Log,
    Log2,
    Log10,
    Sqrt,
    Cbrt,
    Abs,
    Floor,
    Ceil,
    Round,
    Sign,
    Min,
    Max,
    Dot,
    Cross,
    Det,
    Transpose,
    Norm,
}

pub const FUNCTIONS: &[Function] = &[
    Function::Sin,
    Function::Cos,
    Function::Tan,
    Function::Asin,
    Function::Acos,
    Function::Atan,
    Function::Atan2,
    Function::Sinh,
    Function::Cosh,
    Function::Tanh,
    Function::Exp,
    Function::Ln,
    Function::Log,
    Function::Log2,
    Function::Log10,
    Function::Sqrt,
    Function::Cbrt,
    Function::Abs,
    Function::Floor,
    Function::Ceil,
    Function::Round,
    Function::Sign,
    Function::Min,
    Function::Max,
    Function::Dot,
    Function::Cross,
    Function::Det,
    Function::Transpose,
    Function::Norm,
];

pub struct ParseFunctionError;

impl FromStr for Function {
    type Err = ParseFunctionError;

    fn from_str(name: &str) -> Result<Function, ParseFunctionError> {
        Ok(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "tan" => Self::Tan,
            "asin" => Self::Asin,
            "acos" => Self::Acos,
            "atan" => Self::Atan,
            "atan2" => Self::Atan2,
            "sinh" => Self::Sinh,
            "cosh" => Self::Cosh,
            "tanh" => Self::Tanh,
            "exp" => Self::Exp,
            "ln" => Self::Ln,
            "log" => Self::Log,
            "log2" => Self::Log2,
            "log10" => Self::Log10,
            "sqrt" => Self::Sqrt,
            "cbrt" => Self::Cbrt,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "ceil" => Self::Ceil,
            "round" => Self::Round,
            "sign" => Self::Sign,
            "min" => Self::Min,
            "max" => Self::Max,
            "dot" => Self::Dot,
            "cross" => Self::Cross,
            "det" => Self::Det,
            "transpose" => Self::Transpose,
            "norm" => Self::Norm,
            _ => return Err(ParseFunctionError),
        })
    }
}

impl Function {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Cos => "cos",
            Self::Tan => "tan",
            Self::Asin => "asin",
            Self::Acos => "acos",
            Self::Atan => "atan",
            Self::Atan2 => "atan2",
            Self::Sinh => "sinh",
            Self::Cosh => "cosh",
            Self::Tanh => "tanh",
            Self::Exp => "exp",
            Self::Ln => "ln",
            Self::Log => "log",
            Self::Log2 => "log2",
            Self::Log10 => "log10",
            Self::Sqrt => "sqrt",
            Self::Cbrt => "cbrt",
            Self::Abs => "abs",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Round => "round",
            Self::Sign => "sign",
            Self::Min => "min",
            Self::Max => "max",
            Self::Dot => "dot",
            Self::Cross => "cross",
            Self::Det => "det",
            Self::Transpose => "transpose",
            Self::Norm => "norm",
        }
    }

    pub fn arities(self) -> &'static [usize] {
        match self {
            Self::Log => &[1, 2],
            Self::Atan2 | Self::Min | Self::Max | Self::Dot | Self::Cross => &[2],
            _ => &[1],
        }
    }

    pub fn accepts(self, arguments: usize) -> bool {
        self.arities().contains(&arguments)
    }

    pub fn arity_text(self) -> String {
        match self.arities() {
            [one] => format!("{one} argument"),
            [one, two] => format!("{one} or {two} arguments"),
            _ => "arguments".to_string(),
        }
    }

    pub fn eval(self, args: &[Constant]) -> Option<Constant> {
        if !self.accepts(args.len()) {
            return None;
        }

        match self {
            Self::Sin => unary(args, f64::sin),
            Self::Cos => unary(args, f64::cos),
            Self::Tan => unary(args, f64::tan),
            Self::Asin => unary_checked(args, |x| (-1.0..=1.0).contains(&x).then(|| x.asin())),
            Self::Acos => unary_checked(args, |x| (-1.0..=1.0).contains(&x).then(|| x.acos())),
            Self::Atan => unary(args, f64::atan),
            Self::Sinh => unary(args, f64::sinh),
            Self::Cosh => unary(args, f64::cosh),
            Self::Tanh => unary(args, f64::tanh),
            Self::Exp => unary(args, f64::exp),
            Self::Ln => unary_checked(args, |x| (x > 0.0).then(|| x.ln())),
            Self::Log2 => unary_checked(args, |x| (x > 0.0).then(|| x.log2())),
            Self::Log10 => unary_checked(args, |x| (x > 0.0).then(|| x.log10())),
            Self::Log => {
                if args.len() == 1 {
                    let value = &args[0];
                    value.map_reals(|x| (x > 0.0).then(|| x.log10()))
                } else {
                    let base = args[1].as_real()?.get();
                    if base <= 0.0 || base == 1.0 {
                        return None;
                    }
                    let value = &args[0];
                    value.map_reals(|x| (x > 0.0).then(|| x.log(base)))
                }
            }
            Self::Sqrt => unary_checked(args, |x| (x >= 0.0).then(|| x.sqrt())),
            Self::Cbrt => unary(args, f64::cbrt),
            Self::Abs => unary(args, f64::abs),
            Self::Floor => unary(args, f64::floor),
            Self::Ceil => unary(args, f64::ceil),
            Self::Round => unary(args, f64::round),
            Self::Sign => unary(args, |x| if x == 0.0 { 0.0 } else { x.signum() }),
            Self::Atan2 => {
                let y = args[0].as_real()?.get();
                let x = args[1].as_real()?.get();
                Constant::real(y.atan2(x))
            }
            Self::Min | Self::Max => {
                let (a, b) = (args[0].clone(), args[1].clone());
                let choose = |x: f64, y: f64| {
                    if self == Self::Min {
                        x.min(y)
                    } else {
                        x.max(y)
                    }
                };
                a.zip_map(&b, |x, y| Some(choose(x, y)))
            }
            Self::Dot => {
                let a = args[0].as_vector()?;
                let b = args[1].as_vector()?;
                if a.len() != b.len() {
                    return None;
                }
                let mut sum = 0.0;
                for (a, b) in a.iter().zip(b) {
                    sum += a.get() * b.get();
                }
                Constant::real(sum)
            }
            Self::Cross => {
                let a = args[0].as_vector()?;
                let b = args[1].as_vector()?;
                if a.len() != 3 || b.len() != 3 {
                    return None;
                }
                let vector = vec![
                    Real::new(a[1].get() * b[2].get() - a[2].get() * b[1].get())?,
                    Real::new(a[2].get() * b[0].get() - a[0].get() * b[2].get())?,
                    Real::new(a[0].get() * b[1].get() - a[1].get() * b[0].get())?,
                ];
                Some(Constant::Vector(vector))
            }
            Self::Det => args[0]
                .as_matrix()
                .and_then(|matrix| matrix.determinant())
                .map(Constant::Real),
            Self::Transpose => args[0]
                .as_matrix()
                .map(|matrix| Constant::Matrix(matrix.transpose())),
            Self::Norm => args[0].norm().map(Constant::Real),
        }
    }
}

fn unary(args: &[Constant], function: impl Fn(f64) -> f64) -> Option<Constant> {
    args[0].map_reals(|value| Some(function(value)))
}

fn unary_checked(args: &[Constant], function: impl Fn(f64) -> Option<f64>) -> Option<Constant> {
    args[0].map_reals(function)
}

pub fn function_names() -> impl Iterator<Item = &'static str> {
    FUNCTIONS.iter().map(|function| function.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_domains() {
        let nine = Constant::real(9.0).unwrap();
        assert_eq!(Function::Sqrt.eval(&[nine]).unwrap().to_string(), "3");

        let minus_one = Constant::real(-1.0).unwrap();
        assert!(Function::Sqrt.eval(&[minus_one]).is_none());
    }
}
