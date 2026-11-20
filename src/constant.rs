//! Primitive constants: real numbers, vectors and matrices.
//!
//! The types here are deliberately ordered and hashable so they can be stored
//! directly in an egg language.  `Real` wraps `f64` with a total order (and
//! rejects non-finite values).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

/// A finite, totally ordered floating point number.
#[derive(Clone, Copy, Debug)]
pub struct Real(f64);

impl Real {
    /// Create a finite real.  NaN and infinities are rejected.
    pub fn new(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        // Normalize -0.0 so equality and hashing stay coherent.
        Some(Self(if value == 0.0 { 0.0 } else { value }))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl PartialEq for Real {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for Real {}

impl Hash for Real {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl PartialOrd for Real {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Real {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl fmt::Display for Real {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.get();

        if value == 0.0 {
            return f.write_str("0");
        }

        let magnitude = value.abs();
        if magnitude >= 1e15 || magnitude < 1e-9 {
            return write!(f, "{value:e}");
        }

        write!(f, "{value}")
    }
}

impl FromStr for Real {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value: f64 = s
            .trim()
            .parse()
            .map_err(|_| format!("`{s}` is not a real number"))?;
        Real::new(value).ok_or_else(|| format!("`{s}` is not a finite real number"))
    }
}

/// A dense row-major matrix.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<Real>,
}

impl Matrix {
    pub fn new(rows: usize, cols: usize, data: Vec<Real>) -> Option<Self> {
        if rows.checked_mul(cols)? != data.len() {
            return None;
        }
        Some(Self { rows, cols, data })
    }

    pub fn from_rows(rows: Vec<Vec<Real>>) -> Option<Self> {
        let row_count = rows.len();
        let col_count = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|row| row.len() != col_count) {
            return None;
        }
        let data = rows.into_iter().flatten().collect();
        Self::new(row_count, col_count, data)
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn data(&self) -> &[Real] {
        &self.data
    }

    pub fn row(&self, row: usize) -> &[Real] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    pub fn get(&self, row: usize, col: usize) -> Real {
        self.data[row * self.cols + col]
    }

    pub fn identity(size: usize) -> Self {
        let mut data = vec![Real::new(0.0).unwrap(); size * size];
        for i in 0..size {
            data[i * size + i] = Real::new(1.0).unwrap();
        }
        Self::new(size, size, data).unwrap()
    }

    pub fn transpose(&self) -> Self {
        let mut data = Vec::with_capacity(self.data.len());
        for col in 0..self.cols {
            for row in 0..self.rows {
                data.push(self.get(row, col));
            }
        }
        Self::new(self.cols, self.rows, data).unwrap()
    }

    pub fn zip_map<F>(&self, other: &Self, mut f: F) -> Option<Self>
    where
        F: FnMut(f64, f64) -> Option<f64>,
    {
        if self.rows != other.rows || self.cols != other.cols {
            return None;
        }
        let data = self
            .data
            .iter()
            .zip(&other.data)
            .map(|(a, b)| f(a.get(), b.get()).and_then(Real::new))
            .collect::<Option<Vec<_>>>()?;
        Self::new(self.rows, self.cols, data)
    }

    pub fn map<F>(&self, mut f: F) -> Option<Self>
    where
        F: FnMut(f64) -> Option<f64>,
    {
        let data = self
            .data
            .iter()
            .map(|a| f(a.get()).and_then(Real::new))
            .collect::<Option<Vec<_>>>()?;
        Self::new(self.rows, self.cols, data)
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        self.zip_map(other, |a, b| Some(a + b))
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        self.zip_map(other, |a, b| Some(a - b))
    }

    pub fn scale(&self, factor: f64) -> Option<Self> {
        self.map(|value| Some(value * factor))
    }

    pub fn matmul(&self, other: &Self) -> Option<Self> {
        if self.cols != other.rows {
            return None;
        }
        let mut data = Vec::with_capacity(self.rows * other.cols);
        for row in 0..self.rows {
            for col in 0..other.cols {
                let mut sum = 0.0;
                for k in 0..self.cols {
                    sum += self.get(row, k).get() * other.get(k, col).get();
                }
                data.push(Real::new(sum)?);
            }
        }
        Self::new(self.rows, other.cols, data)
    }

    pub fn mul_vector(&self, vector: &[Real]) -> Option<Vec<Real>> {
        if self.cols != vector.len() {
            return None;
        }
        let mut result = Vec::with_capacity(self.rows);
        for row in 0..self.rows {
            let mut sum = 0.0;
            for (col, value) in vector.iter().enumerate() {
                sum += self.get(row, col).get() * value.get();
            }
            result.push(Real::new(sum)?);
        }
        Some(result)
    }

    pub fn determinant(&self) -> Option<Real> {
        if self.rows != self.cols {
            return None;
        }
        match self.rows {
            0 => Real::new(1.0),
            1 => Some(self.get(0, 0)),
            2 => Real::new(
                self.get(0, 0).get() * self.get(1, 1).get()
                    - self.get(0, 1).get() * self.get(1, 0).get(),
            ),
            n => {
                // Laplace expansion.  Constant matrices in a calculator are
                // expected to be small.
                let mut sum = 0.0;
                for col in 0..n {
                    let mut minor = Vec::with_capacity((n - 1) * (n - 1));
                    for row in 1..n {
                        for inner_col in 0..n {
                            if inner_col != col {
                                minor.push(self.get(row, inner_col));
                            }
                        }
                    }
                    let minor = Matrix::new(n - 1, n - 1, minor)?;
                    let sign = if col % 2 == 0 { 1.0 } else { -1.0 };
                    sum += sign * self.get(0, col).get() * minor.determinant()?.get();
                }
                Real::new(sum)
            }
        }
    }

    pub fn pow(&self, exponent: i64) -> Option<Self> {
        if self.rows != self.cols || exponent < 0 {
            return None;
        }
        let mut result = Self::identity(self.rows);
        let mut base = self.clone();
        let mut power = exponent as u64;
        while power > 0 {
            if power & 1 == 1 {
                result = result.matmul(&base)?;
            }
            power >>= 1;
            if power > 0 {
                base = base.matmul(&base)?;
            }
        }
        Some(result)
    }
}

impl fmt::Display for Matrix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for row in 0..self.rows {
            if row > 0 {
                f.write_str(", ")?;
            }
            f.write_str("[")?;
            for col in 0..self.cols {
                if col > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{}", self.get(row, col))?;
            }
            f.write_str("]")?;
        }
        f.write_str("]")
    }
}

/// A primitive constant value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Constant {
    Real(Real),
    Vector(Vec<Real>),
    Matrix(Matrix),
}

impl Constant {
    pub fn real(value: f64) -> Option<Self> {
        Real::new(value).map(Constant::Real)
    }

    pub fn vector(values: Vec<Real>) -> Self {
        Constant::Vector(values)
    }

    pub fn matrix(matrix: Matrix) -> Self {
        Constant::Matrix(matrix)
    }

    pub fn as_real(&self) -> Option<Real> {
        match self {
            Constant::Real(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<&[Real]> {
        match self {
            Constant::Vector(values) => Some(values),
            _ => None,
        }
    }

    pub fn as_matrix(&self) -> Option<&Matrix> {
        match self {
            Constant::Matrix(matrix) => Some(matrix),
            _ => None,
        }
    }

    pub fn map_reals<F>(&self, mut f: F) -> Option<Self>
    where
        F: FnMut(f64) -> Option<f64>,
    {
        match self {
            Constant::Real(value) => f(value.get()).and_then(Real::new).map(Constant::Real),
            Constant::Vector(values) => values
                .iter()
                .map(|value| f(value.get()).and_then(Real::new))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            Constant::Matrix(matrix) => matrix.map(f).map(Constant::Matrix),
        }
    }

    pub fn zip_map<F>(&self, other: &Self, mut f: F) -> Option<Self>
    where
        F: FnMut(f64, f64) -> Option<f64>,
    {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) => {
                f(a.get(), b.get()).and_then(Real::new).map(Constant::Real)
            }
            (Constant::Vector(a), Constant::Vector(b)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .map(|(a, b)| f(a.get(), b.get()).and_then(Real::new))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Matrix(a), Constant::Matrix(b)) => a.zip_map(b, f).map(Constant::Matrix),
            _ => None,
        }
    }

    pub fn add(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) => Constant::real(a.get() + b.get()),
            (Constant::Vector(a), Constant::Vector(b)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .map(|(a, b)| Real::new(a.get() + b.get()))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Matrix(a), Constant::Matrix(b)) => a.add(b).map(Constant::Matrix),
            _ => None,
        }
    }

    pub fn sub(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) => Constant::real(a.get() - b.get()),
            (Constant::Vector(a), Constant::Vector(b)) if a.len() == b.len() => a
                .iter()
                .zip(b)
                .map(|(a, b)| Real::new(a.get() - b.get()))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Matrix(a), Constant::Matrix(b)) => a.sub(b).map(Constant::Matrix),
            _ => None,
        }
    }

    pub fn mul(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) => Constant::real(a.get() * b.get()),
            (Constant::Real(a), Constant::Vector(b)) => b
                .iter()
                .map(|value| Real::new(a.get() * value.get()))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Vector(a), Constant::Real(b)) => a
                .iter()
                .map(|value| Real::new(b.get() * value.get()))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Real(a), Constant::Matrix(b)) => b.scale(a.get()).map(Constant::Matrix),
            (Constant::Matrix(a), Constant::Real(b)) => a.scale(b.get()).map(Constant::Matrix),
            (Constant::Vector(a), Constant::Vector(b)) if a.len() == b.len() => {
                let mut sum = 0.0;
                for (a, b) in a.iter().zip(b) {
                    sum += a.get() * b.get();
                }
                Constant::real(sum)
            }
            (Constant::Matrix(a), Constant::Matrix(b)) => a.matmul(b).map(Constant::Matrix),
            (Constant::Matrix(a), Constant::Vector(b)) => a.mul_vector(b).map(Constant::Vector),
            (Constant::Vector(a), Constant::Matrix(b)) => {
                // Row vector times matrix.
                if a.len() != b.rows() {
                    return None;
                }
                let mut result = Vec::with_capacity(b.cols());
                for col in 0..b.cols() {
                    let mut sum = 0.0;
                    for (row, value) in a.iter().enumerate() {
                        sum += value.get() * b.get(row, col).get();
                    }
                    result.push(Real::new(sum)?);
                }
                Some(Constant::Vector(result))
            }
            _ => None,
        }
    }

    pub fn div(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) if b.get() != 0.0 => {
                Constant::real(a.get() / b.get())
            }
            (Constant::Vector(a), Constant::Real(b)) if b.get() != 0.0 => a
                .iter()
                .map(|value| Real::new(value.get() / b.get()))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Matrix(a), Constant::Real(b)) if b.get() != 0.0 => {
                a.scale(1.0 / b.get()).map(Constant::Matrix)
            }
            _ => None,
        }
    }

    pub fn pow(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Constant::Real(a), Constant::Real(b)) => {
                let exponent = b.get();
                if a.get() < 0.0 && exponent.fract() != 0.0 {
                    return None;
                }
                Constant::real(a.get().powf(exponent))
            }
            (Constant::Vector(a), Constant::Real(b)) => a
                .iter()
                .map(|value| Real::new(value.get().powf(b.get())))
                .collect::<Option<Vec<_>>>()
                .map(Constant::Vector),
            (Constant::Matrix(a), Constant::Real(b)) => {
                if b.get().fract() != 0.0 {
                    return None;
                }
                let exponent = b.get();
                if exponent < 0.0 || exponent > i64::MAX as f64 {
                    return None;
                }
                a.pow(exponent as i64).map(Constant::Matrix)
            }
            _ => None,
        }
    }

    pub fn neg(&self) -> Option<Self> {
        self.map_reals(|value| Some(-value))
    }

    pub fn norm(&self) -> Option<Real> {
        match self {
            Constant::Real(value) => Real::new(value.get().abs()),
            Constant::Vector(values) => Real::new(
                values
                    .iter()
                    .map(|value| value.get() * value.get())
                    .sum::<f64>()
                    .sqrt(),
            ),
            Constant::Matrix(matrix) => Real::new(
                matrix
                    .data()
                    .iter()
                    .map(|value| value.get() * value.get())
                    .sum::<f64>()
                    .sqrt(),
            ),
        }
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Constant::Real(value) => write!(f, "{value}"),
            Constant::Vector(values) => {
                f.write_str("[")?;
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{value}")?;
                }
                f.write_str("]")
            }
            Constant::Matrix(matrix) => write!(f, "{matrix}"),
        }
    }
}

impl FromStr for Constant {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.starts_with('[') && s.ends_with(']') {
            let inner = &s[1..s.len() - 1];
            let parts = split_top_level(inner)?;
            if parts.is_empty() {
                return Ok(Constant::Vector(Vec::new()));
            }

            let values = parts
                .iter()
                .map(|part| part.parse::<Constant>())
                .collect::<Result<Vec<_>, _>>()?;

            if values
                .iter()
                .all(|value| matches!(value, Constant::Real(_)))
            {
                return Ok(Constant::Vector(
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
                if let Some(matrix) = Matrix::from_rows(rows) {
                    return Ok(Constant::Matrix(matrix));
                }
                return Err("matrix rows have different lengths".to_string());
            }

            return Err("nested constants must be a vector or a matrix".to_string());
        }

        s.parse::<Real>().map(Constant::Real)
    }
}

fn split_top_level(inner: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in inner.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "unmatched `]` in constant".to_string())?;
            }
            ',' if depth == 0 => {
                let part = inner[start..index].trim();
                if part.is_empty() {
                    return Err("empty element in constant list".to_string());
                }
                parts.push(part.to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("unmatched `[` in constant".to_string());
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        parts.push(last.to_string());
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::approx_constant)]
    fn real_display() {
        assert_eq!(Real::new(2.0).unwrap().to_string(), "2");
        assert_eq!(Real::new(3.14).unwrap().to_string(), "3.14");
        assert_eq!(Real::new(-0.0).unwrap().to_string(), "0");
    }

    #[test]
    fn matrices_multiply() {
        let a: Constant = "[[1, 2], [3, 4]]".parse().unwrap();
        let b: Constant = "[[5, 6], [7, 8]]".parse().unwrap();
        assert_eq!(a.mul(&b).unwrap().to_string(), "[[19, 22], [43, 50]]");
    }

    #[test]
    fn vectors_and_matrices_round_trip() {
        for text in ["[1, 2, 3]", "[[1, 2], [3, 4]]", "[]"] {
            let value: Constant = text.parse().unwrap();
            assert_eq!(value.to_string(), text);
        }
    }
}
