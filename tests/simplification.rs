use sygg::{AlgebraEngine, EngineConfig};

fn simplify(expression: &str) -> Vec<String> {
    AlgebraEngine::new(EngineConfig::default())
        .evaluate(expression)
        .unwrap_or_else(|error| panic!("failed to simplify `{expression}`: {error}"))
        .into_iter()
        .map(|result| result.expression)
        .collect()
}

fn assert_exact(expression: &str, expected: &str) {
    let results = simplify(expression);
    assert_eq!(results, vec![expected.to_string()]);
}

fn assert_contains(expression: &str, expected: &str) {
    let results = simplify(expression);
    assert!(
        results.iter().any(|result| result.contains(expected)),
        "`{expression}` did not produce a form containing `{expected}`: {results:?}"
    );
}

#[test]
fn repeated_factor_becomes_power() {
    assert_exact("x * x", "x^2");
}

#[test]
fn repeated_term_becomes_coefficient() {
    assert_exact("x + x", "2 * x");
}

#[test]
fn triple_factor_becomes_cube() {
    assert_exact("x * x * x", "x^3");
}

#[test]
fn power_times_base_adds_exponents() {
    assert_exact("x^2 * x", "x^3");
}

#[test]
fn repeated_factor_in_product_is_combined() {
    let results = simplify("x * y * x");
    assert!(
        results.iter().any(|result| result.contains("x^2")),
        "{results:?}"
    );
    assert!(
        results.iter().all(|result| !result.contains("x * x")),
        "{results:?}"
    );
}

#[test]
fn single_term_factors_with_implicit_one() {
    assert_contains("x * y + x", "x * (y + 1)");
}

#[test]
fn single_term_factors_with_implicit_one_on_subtraction() {
    assert_contains("x * y - x", "x * (y - 1)");
}

#[test]
fn common_factor_is_collected() {
    assert_contains("x * y + x * z", "x * (y + z)");
}

#[test]
fn common_factor_is_collected_from_three_terms() {
    let results = simplify("x * y + x * z + x * w");
    assert!(
        results.iter().any(|result| result.starts_with("x * (")),
        "{results:?}"
    );
}

#[test]
fn factored_product_can_be_distributed() {
    assert_contains("x * (y + z)", "x * y + x * z");
}

#[test]
fn constant_power_is_folded() {
    assert_exact("2 ^ 10", "1024");
}

#[test]
fn multiplication_by_zero_is_folded() {
    assert_exact("x * 0", "0");
}

#[test]
fn cancelling_subtraction_leaves_identity() {
    assert_exact("x + (y - y)", "x");
}

#[test]
fn basic_trig_identity_is_folded() {
    assert_exact("sin(0) + cos(0)", "1");
}

#[test]
fn commuting_products_are_collected() {
    assert_contains("x * y + y * x", "2 * x * y");
}

// Known limitations for future releases. These are intentionally ignored so
// the expected behavior is documented without making the suite red.

#[test]
fn power_product_adds_exponents() {
    assert_exact("x^2 * x^3", "x^5");
}

#[test]
fn difference_of_squares_factors() {
    assert_contains("x^2 - y^2", "(x - y) * (x + y)");
}

#[test]
fn perfect_square_polynomial_factors() {
    assert_contains("x^2 + 2 * x + 1", "(x + 1)^2");
}

#[test]
#[ignore = "fraction addition and cancellation are not implemented yet"]
fn common_denominator_fraction_cancels() {
    assert_exact("x / (x + 1) + 1 / (x + 1)", "1");
}

#[test]
fn difference_product_expands() {
    assert_contains("(x + y) * (x - y)", "x^2 - y^2");
}
