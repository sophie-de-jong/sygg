use sygg::{AlgebraEngine, EngineConfig};

fn results(expression: &str) -> Vec<sygg::StrategyResult> {
    AlgebraEngine::new(EngineConfig::default())
        .evaluate(expression)
        .unwrap_or_else(|error| panic!("failed to simplify `{expression}`: {error}"))
}

fn simplify(expression: &str) -> Vec<String> {
    results(expression)
        .into_iter()
        .map(|result| result.expression)
        .collect()
}

fn assert_contains(expression: &str, expected: &str) {
    let results = simplify(expression);
    assert!(
        results.iter().any(|result| result.contains(expected)),
        "`{expression}` did not produce a form containing `{expected}`: {results:?}"
    );
}

fn assert_contains_any(expression: &str, expected: &[&str]) {
    let results = simplify(expression);
    assert!(
        results
            .iter()
            .any(|result| expected.iter().any(|expected| result.contains(expected))),
        "`{expression}` did not produce any of {expected:?}: {results:?}"
    );
}

fn assert_not_contains(expression: &str, unexpected: &str) {
    let results = simplify(expression);
    assert!(
        results.iter().all(|result| !result.contains(unexpected)),
        "`{expression}` unexpectedly produced `{unexpected}`: {results:?}"
    );
}

#[test]
fn subtraction_reduces_coefficient() {
    assert_contains("2 * x - x", "x");
}

#[test]
fn subtraction_reduces_two_coefficients() {
    assert_contains("3 * x - 2 * x", "x");
}

#[test]
fn three_like_terms_reduce() {
    assert_contains("x + x - x", "x");
}

#[test]
fn negative_like_term_reduces() {
    assert_contains("2 * x + (-x)", "x");
}

#[test]
fn negative_power_cancels() {
    assert_contains("x^(-1) * x", "1");
}

#[test]
fn decimal_sum_does_not_round_to_zero() {
    assert_contains("0.5 + 0.5 + 0.5", "1.5");
}

#[test]
fn quarter_sum_does_not_round_to_zero() {
    assert_contains("0.25 + 0.25 + 0.5", "1");
}

#[test]
fn repeating_thirds_sum_to_one() {
    assert_contains("1/3 + 1/3 + 1/3", "1");
}

#[test]
fn decimal_coefficients_are_collected() {
    assert_contains("x * 0.5 + x * 0.5", "x");
}

#[test]
fn decimal_sum_inside_product_is_folded() {
    assert_contains("x * (0.5 + 0.5)", "x");
}

#[test]
fn several_decimal_coefficients_are_collected() {
    assert_contains("x * 0.2 + x * 0.2 + x * 0.6", "x");
}

#[test]
fn decimal_coefficients_display_without_float_noise() {
    assert_contains("x * 0.1 + x * 0.2", "0.3 * x");
}

#[test]
fn matrix_multiplication_keeps_operand_order() {
    assert_contains("[[5,6],[7,8]] * [[1,2],[3,4]]", "[[23, 34], [31, 46]]");
}

#[test]
fn matrix_multiplication_with_zero_rows_is_correct() {
    assert_contains("[[1,0],[0,0]] * [[0,1],[0,0]]", "[[0, 1], [0, 0]]");
}

#[test]
fn matrix_zero_power_is_identity() {
    assert_contains("[[1,2],[3,4]]^0", "[[1, 0], [0, 1]]");
}

#[test]
fn vector_zero_power_is_elementwise() {
    assert_contains("[1,2]^0", "[1, 1]");
}

#[test]
fn vector_times_zero_stays_a_vector() {
    assert_contains("[1,2]*0", "[0, 0]");
}

#[test]
fn vector_subtraction_stays_a_vector() {
    assert_contains("[1,2] - [1,2]", "[0, 0]");
}

#[test]
fn scalar_divided_by_vector_is_not_collapsed_to_scalar_zero() {
    assert_contains("0/[1,2]", "0 / [1, 2]");
}

#[test]
fn symbolic_vector_product_is_not_turned_into_vector_power() {
    assert_not_contains("[x,y]*[x,y]", "[x, y]^2");
}

#[test]
fn nested_dot_product_stays_correct() {
    assert_contains("([1,2]*[3,4])*[1,2]", "[11, 22]");
}

#[test]
fn exp_of_log_of_negative_stays_symbolic() {
    assert_contains("exp(ln(-1))", "exp(ln(-1))");
}

#[test]
fn exp_of_log_of_zero_stays_symbolic() {
    assert_contains("exp(ln(0))", "exp(ln(0))");
}

#[test]
fn decimal_coefficient_expression_is_correct() {
    assert_contains("x * 0.5 + x * 0.5", "x");
}

#[test]
fn multiplication_constants_are_collected() {
    assert_contains("2 * x * 3 * x", "6 * x^2");
}

#[test]
fn multiplication_constants_reorder_across_symbols() {
    assert_contains("x * 2 * 3", "6 * x");
}

#[test]
fn power_product_adds_exponents() {
    assert_contains("x^2 * x^3", "x^5");
}

#[test]
fn power_division_subtracts_exponents() {
    assert_contains("x^5 / x^3", "x^2");
}

#[test]
fn nested_powers_multiply_exponents() {
    assert_contains("(x^2)^3", "x^6");
}

#[test]
fn fractional_power_cancels_in_product() {
    assert_contains("x^(-2) * x^2", "1");
}

#[test]
fn symbolic_power_product_adds_exponents() {
    assert_contains("x^a * x^b", "x^(a + b)");
}

#[test]
fn division_cancels_factor() {
    assert_contains("2 * x / x", "2");
}

#[test]
fn division_cancels_symbolic_factor() {
    assert_contains("(x * y) / x", "y");
}

#[test]
fn power_division_cancels_base() {
    assert_contains("(x^2) / x", "x");
}

#[test]
fn division_by_scalar_cancels() {
    assert_contains("(x / 2) * 2", "x");
}

#[test]
fn division_by_symbol_cancels() {
    assert_contains("(x / y) * y", "x");
}

#[test]
fn reciprocal_product_cancels() {
    assert_contains("x * (1 / x)", "1");
}

#[test]
fn reciprocal_of_reciprocal_cancels() {
    assert_contains("1 / (1 / x)", "x");
}

#[test]
fn sum_cancellation_removes_like_terms() {
    assert_contains("x + y - x", "y");
}

#[test]
fn negative_terms_combine() {
    assert_contains("-x - x", "-2 * x");
}

#[test]
fn negation_distributes() {
    assert_contains("-(x + y)", "-x - y");
}

#[test]
fn even_power_removes_negation() {
    assert_contains("(-x)^2", "x^2");
}

#[test]
fn product_negation_cancels() {
    assert_contains("x * (y + z) - x * y", "x * z");
}

#[test]
fn pythagorean_identity_for_sine() {
    assert_contains("1 - sin(x)^2", "cos(x)^2");
}

#[test]
fn pythagorean_identity_for_cosine() {
    assert_contains("1 - cos(x)^2", "sin(x)^2");
}

#[test]
fn sine_over_cosine_is_tangent() {
    assert_contains("sin(x) / cos(x)", "tan(x)");
}

#[test]
fn absolute_value_square_removes_absolute_value() {
    assert_contains("abs(x)^2", "x^2");
}

#[test]
fn absolute_value_of_product_splits() {
    assert_contains("abs(x * y)", "abs(x) * abs(y)");
}

#[test]
fn min_of_identical_values_collapses() {
    assert_contains("min(x, x)", "x");
}

#[test]
fn max_of_identical_values_collapses() {
    assert_contains("max(x, x)", "x");
}

#[test]
fn negative_cube_root_is_folded() {
    assert_contains("(-8)^(1/3)", "-2");
}

// Known limitations kept visible for future work.
#[test]
fn like_fraction_denominators_are_combined() {
    assert_contains("x / 2 + x / 2", "x");
}

#[test]
fn unlike_fraction_denominators_are_combined() {
    assert_contains("x / 2 + x / 3", "5 * x / 6");
}

#[test]
fn negative_power_is_written_as_reciprocal() {
    assert_contains("x / x^2", "1 / x");
}

#[test]
fn expanded_square_cancels_terms() {
    assert_contains_any("(x + y)^2 - x^2 - y^2", &["2 * x * y", "2 * y * x"]);
}

#[test]
fn depth_limit_blocks_growing_rewrites() {
    let mut engine = AlgebraEngine::new(EngineConfig {
        load_default_rules: false,
        max_depth: 1,
        ..EngineConfig::default()
    });
    engine.import_rules("grow: x => x + 1", "test").unwrap();
    assert_eq!(engine.evaluate("x").unwrap()[0].expression, "x");
}

#[test]
fn subtraction_after_factoring_cancels() {
    assert_contains("x * (y + z) - x * y", "x * z");
}

#[test]
#[ignore = "logarithm product expansion is domain-sensitive and not implemented yet"]
fn log_of_product_expands() {
    assert_contains("ln(x * y)", "ln(x) + ln(y)");
}

#[test]
#[ignore = "exponential product combination is not selected by any strategy yet"]
fn exponential_product_combines() {
    assert_contains("exp(x) * exp(y)", "exp(x + y)");
}

#[test]
#[ignore = "factoring higher-degree polynomials is not implemented yet"]
fn quartic_binomial_expands() {
    assert_contains("(x + y)^4", "x^4 + 4 * x^3 * y");
}

#[test]
#[ignore = "common numeric coefficient factoring is not implemented yet"]
fn common_numeric_factor_is_collected() {
    assert_contains("2 * x^2 + 4 * x", "2 * x * (x + 2)");
}
