//! Loading human-readable rewrite rules and bridging them into egg.

use core::fmt;
use std::collections::HashMap;
use std::path::Path;

use egg::{
    Applier, ConditionalApplier, EGraph, ENodeOrVar, Id, Language, Pattern, PatternAst, Rewrite,
    Subst, Symbol, Var,
};

use crate::analysis::{ConstantFolding, MathEGraph, MathRewrite};
use crate::ast::{Ast, parse_expression, pretty_ast_exact};
use crate::condition::ConditionExpr;
use crate::constant::Constant;
use crate::error::{AlgebraError, Result};
use crate::language::Math;

/// A parsed rewrite rule from a rule file.
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub lhs: Ast,
    pub rhs: Ast,
    pub condition: Option<ConditionExpr>,
    pub origin: String,
}

impl Rule {
    pub fn to_rewrite(&self) -> Result<MathRewrite> {
        self.to_bounded_rewrite(usize::MAX)
    }

    pub fn to_bounded_rewrite(&self, max_depth: usize) -> Result<MathRewrite> {
        let mut variables = HashMap::new();
        let lhs = to_pattern_ast(&self.lhs, true, &mut variables);
        let rhs = to_pattern_ast(&self.rhs, true, &mut variables);

        let searcher = Pattern::new(lhs);
        let applier = SafePatternApplier { ast: rhs };
        let applier = ConditionalApplier {
            condition: OptionalCondition(self.condition.clone()),
            applier,
        };
        let applier = DepthBoundedApplier {
            inner: applier,
            rhs: self.rhs.clone(),
            max_depth,
        };
        Rewrite::new(self.name.clone(), searcher, applier)
            .map_err(|message| AlgebraError::Rule(format!("{}: {message}", self.name)))
    }
}

#[derive(Clone, Debug)]
struct OptionalCondition(Option<ConditionExpr>);

impl egg::Condition<Math, ConstantFolding> for OptionalCondition {
    fn check(&self, egraph: &mut MathEGraph, eclass: Id, subst: &Subst) -> bool {
        self.0
            .as_ref()
            .is_none_or(|condition| condition.check(egraph, eclass, subst))
    }

    fn vars(&self) -> Vec<Var> {
        self.0
            .as_ref()
            .map_or_else(Vec::new, ConditionExpr::collect_vars)
    }
}

/// Instantiates a rule's right-hand side but refuses to merge two e-classes
/// that already contain different constants.
///
/// Equality saturation normally never puts two different constants in the
/// same e-class.  Floating-point rounding and underflow can nevertheless
/// produce near-equal constants (ultimately including zero) for expressions
/// that are mathematically equal.  If such a class is then used as a pattern
/// constant, every rule with a literal `0` or `1` can match unrelated terms.
/// Keeping the one-constant-per-class invariant makes literal patterns sound.
struct SafePatternApplier {
    ast: PatternAst<Math>,
}

impl Applier<Math, ConstantFolding> for SafePatternApplier {
    fn apply_one(
        &self,
        egraph: &mut EGraph<Math, ConstantFolding>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<Math>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let mut ids = vec![Id::from(0); self.ast.len()];
        for (index, node) in self.ast.as_ref().iter().enumerate() {
            let id = match node {
                ENodeOrVar::Var(variable) => subst[*variable],
                ENodeOrVar::ENode(enode) => {
                    let instantiated = enode.clone().map_children(|child| ids[usize::from(child)]);
                    egraph.add(instantiated)
                }
            };
            ids[index] = id;
        }

        let Some(rhs) = ids.last().copied() else {
            return Vec::new();
        };
        if constants_conflict(egraph, rhs, eclass) {
            return Vec::new();
        }

        if egraph.union(rhs, eclass) {
            vec![eclass]
        } else {
            Vec::new()
        }
    }

    fn vars(&self) -> Vec<Var> {
        self.ast
            .as_ref()
            .iter()
            .filter_map(|node| match node {
                ENodeOrVar::Var(variable) => Some(*variable),
                ENodeOrVar::ENode(_) => None,
            })
            .collect()
    }

    fn get_pattern_ast(&self) -> Option<&PatternAst<Math>> {
        Some(&self.ast)
    }
}

fn constants_conflict(egraph: &MathEGraph, left: Id, right: Id) -> bool {
    let left = egraph.find(left);
    let right = egraph.find(right);
    if left == right {
        return false;
    }

    let mut constants = class_constants(egraph, left);
    constants.extend(class_constants(egraph, right));
    let Some(first) = constants.first() else {
        return false;
    };
    constants.iter().any(|constant| constant != first)
}

fn class_constants(egraph: &MathEGraph, id: Id) -> Vec<Constant> {
    let class = &egraph[egraph.find(id)];
    let explicit = class
        .nodes
        .iter()
        .filter_map(|node| match node {
            Math::Const(value) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if explicit.is_empty() {
        class.data.constant.iter().cloned().collect()
    } else {
        explicit
    }
}

struct DepthBoundedApplier<A> {
    inner: A,
    rhs: Ast,
    max_depth: usize,
}

impl<A> Applier<Math, ConstantFolding> for DepthBoundedApplier<A>
where
    A: Applier<Math, ConstantFolding>,
{
    fn apply_one(
        &self,
        egraph: &mut EGraph<Math, ConstantFolding>,
        eclass: Id,
        subst: &Subst,
        searcher_ast: Option<&PatternAst<Math>>,
        rule_name: Symbol,
    ) -> Vec<Id> {
        if self.max_depth != usize::MAX
            && rhs_depth(&self.rhs, egraph, subst).is_some_and(|depth| depth > self.max_depth)
        {
            return Vec::new();
        }
        self.inner
            .apply_one(egraph, eclass, subst, searcher_ast, rule_name)
    }

    fn vars(&self) -> Vec<Var> {
        self.inner.vars()
    }

    fn get_pattern_ast(&self) -> Option<&PatternAst<Math>> {
        self.inner.get_pattern_ast()
    }
}

fn rhs_depth(ast: &Ast, egraph: &MathEGraph, subst: &Subst) -> Option<usize> {
    match ast {
        Ast::Const(_) => Some(1),
        Ast::Symbol(name) => {
            let full = if name.starts_with('?') && name.len() > 1 {
                name.clone()
            } else {
                format!("?{name}")
            };
            let variable: Var = full.parse().ok()?;
            let id = subst.get(variable)?;
            Some(egraph[*id].data.depth)
        }
        Ast::Add(a, b) | Ast::Sub(a, b) | Ast::Mul(a, b) | Ast::Div(a, b) | Ast::Pow(a, b) => {
            Some(1 + rhs_depth(a, egraph, subst)?.max(rhs_depth(b, egraph, subst)?))
        }
        Ast::Neg(inner) => Some(1 + rhs_depth(inner, egraph, subst)?),
        Ast::Call(_, args) | Ast::Vec(args) => {
            let depth = args
                .iter()
                .map(|arg| rhs_depth(arg, egraph, subst))
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .max()
                .unwrap_or(0);
            Some(1 + depth)
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}: {} => {}",
            self.name,
            pretty_ast_exact(&self.lhs),
            pretty_ast_exact(&self.rhs)
        )?;
        if let Some(condition) = &self.condition {
            write!(f, " if {condition}")?;
        }
        Ok(())
    }
}

/// Parse a rules file from disk.
pub fn parse_rules_file(path: &Path) -> Result<Vec<Rule>> {
    let text = std::fs::read_to_string(path)?;
    parse_rules(&text, &path.display().to_string())
}

/// Parse rules from a string.  The origin is used in diagnostics.
pub fn parse_rules(text: &str, origin: &str) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        match parse_rule_line(line, origin, line_number, rules.len()) {
            Ok(Some(mut parsed)) => rules.append(&mut parsed),
            Ok(None) => {}
            Err(message) => {
                return Err(AlgebraError::RuleFile {
                    path: origin.to_string(),
                    line: line_number,
                    message,
                });
            }
        }
    }
    log::debug!("parsed {} rule(s) from {}", rules.len(), origin);
    Ok(rules)
}

fn parse_rule_line(
    line: &str,
    origin: &str,
    line_number: usize,
    rule_index: usize,
) -> std::result::Result<Option<Vec<Rule>>, String> {
    let line = line.split('#').next().unwrap_or("");
    let line = line.find("//").map_or(line, |position| &line[..position]);
    let line = line.trim();
    let line = line.trim_end_matches(';').trim();
    if line.is_empty() {
        return Ok(None);
    }

    let iff_position = line.find("<=>");
    let arrow_position = line.find("=>");
    let (position, separator_len, bidirectional) = match (iff_position, arrow_position) {
        (Some(iff), _) => (iff, 3, true),
        (None, Some(arrow)) => (arrow, 2, false),
        (None, None) => {
            return Err("missing rewrite arrow `=>` or `<=>`".to_string());
        }
    };

    let left = line[..position].trim();
    let right = line[position + separator_len..].trim();
    if left.is_empty() {
        return Err("missing left-hand side".to_string());
    }
    if right.is_empty() {
        return Err("missing right-hand side".to_string());
    }

    let (name, lhs_text) = split_rule_name(left, rule_index);
    if lhs_text.is_empty() {
        return Err("missing left-hand side".to_string());
    }

    let (right, condition_text) = split_condition(right);
    let condition = match condition_text {
        Some(text) if text.trim().is_empty() => {
            return Err("empty condition after `if`".to_string());
        }
        Some(text) => {
            Some(ConditionExpr::parse(text).map_err(|error| format!("condition: {error}"))?)
        }
        None => None,
    };

    let lhs = parse_expression(lhs_text).map_err(|error| format!("left-hand side: {error}"))?;
    let rhs = parse_expression(right).map_err(|error| format!("right-hand side: {error}"))?;

    let origin = format!("{origin}:{line_number}");
    let mut rules = vec![Rule {
        name,
        lhs: lhs.clone(),
        rhs: rhs.clone(),
        condition: condition.clone(),
        origin: origin.clone(),
    }];

    if bidirectional {
        rules.push(Rule {
            name: format!("{}-rev", rules[0].name),
            lhs: rhs,
            rhs: lhs,
            condition,
            origin,
        });
    }

    Ok(Some(rules))
}

fn split_condition(right: &str) -> (&str, Option<&str>) {
    let mut depth = 0i32;
    let mut previous = None;

    for (index, character) in right.char_indices() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }

        if depth == 0
            && right[index..].starts_with("if")
            && previous.is_none_or(|character: char| character.is_whitespace())
        {
            let after = right[index + 2..].chars().next();
            if after.is_some_and(|character| character.is_whitespace()) {
                return (
                    right[..index].trim_end(),
                    Some(right[index + 2..].trim_start()),
                );
            }
        }

        previous = Some(character);
    }

    (right, None)
}

fn split_rule_name(left: &str, rule_index: usize) -> (String, &str) {
    for (index, ch) in left.char_indices() {
        if ch == ':' || ch == '=' {
            let name = left[..index].trim();
            let expression = left[index + 1..].trim();
            if !name.is_empty() && !expression.is_empty() {
                return (name.to_string(), expression);
            }
        }
    }
    (format!("rule-{}", rule_index + 1), left)
}

fn variable_for(name: &str, variables: &mut HashMap<String, Var>) -> Var {
    let full_name = if name.starts_with('?') && name.len() > 1 {
        name.to_string()
    } else {
        format!("?{name}")
    };
    *variables.entry(full_name.clone()).or_insert_with(|| {
        full_name
            .parse::<Var>()
            .expect("generated variable is valid")
    })
}

fn to_pattern_ast(
    ast: &Ast,
    symbols_are_variables: bool,
    variables: &mut HashMap<String, Var>,
) -> PatternAst<Math> {
    let mut expression = PatternAst::default();
    add_pattern(&mut expression, ast, symbols_are_variables, variables);
    expression
}

fn add_pattern(
    expression: &mut PatternAst<Math>,
    ast: &Ast,
    symbols_are_variables: bool,
    variables: &mut HashMap<String, Var>,
) -> Id {
    match ast {
        Ast::Const(value) => expression.add(ENodeOrVar::ENode(Math::Const(value.clone()))),
        Ast::Symbol(name) => {
            if symbols_are_variables {
                expression.add(ENodeOrVar::Var(variable_for(name, variables)))
            } else {
                expression.add(ENodeOrVar::ENode(Math::Symbol(name.as_str().into())))
            }
        }
        Ast::Add(a, b) => {
            let a = add_pattern(expression, a, symbols_are_variables, variables);
            let b = add_pattern(expression, b, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Add([a, b])))
        }
        Ast::Sub(a, b) => {
            let a = add_pattern(expression, a, symbols_are_variables, variables);
            let b = add_pattern(expression, b, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Sub([a, b])))
        }
        Ast::Mul(a, b) => {
            let a = add_pattern(expression, a, symbols_are_variables, variables);
            let b = add_pattern(expression, b, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Mul([a, b])))
        }
        Ast::Div(a, b) => {
            let a = add_pattern(expression, a, symbols_are_variables, variables);
            let b = add_pattern(expression, b, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Div([a, b])))
        }
        Ast::Pow(a, b) => {
            let a = add_pattern(expression, a, symbols_are_variables, variables);
            let b = add_pattern(expression, b, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Pow([a, b])))
        }
        Ast::Neg(inner) => {
            let inner = add_pattern(expression, inner, symbols_are_variables, variables);
            expression.add(ENodeOrVar::ENode(Math::Neg(inner)))
        }
        Ast::Call(name, args) => {
            let ids = args
                .iter()
                .map(|argument| add_pattern(expression, argument, symbols_are_variables, variables))
                .collect::<Vec<_>>();
            expression.add(ENodeOrVar::ENode(Math::Call(name.as_str().into(), ids)))
        }
        Ast::Vec(items) => {
            let ids = items
                .iter()
                .map(|item| add_pattern(expression, item, symbols_are_variables, variables))
                .collect::<Vec<_>>();
            expression.add(ENodeOrVar::ENode(Math::VecLit(ids.into_boxed_slice())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_unnamed_rules() {
        let rules = parse_rules("commute: x + y => y + x\nx * 0 => 0\n", "test").unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "commute");
        assert_eq!(rules[1].name, "rule-2");
    }

    #[test]
    fn parses_bidirectional_rules() {
        let rules = parse_rules("flip: x + y <=> y + x", "test").unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].name, "flip-rev");
    }

    #[test]
    fn catches_unbound_rhs_variables() {
        let rules = parse_rules("bad: x => y", "test").unwrap();
        assert!(rules[0].to_rewrite().is_err());
    }

    #[test]
    fn parses_conditional_rules() {
        let rules = parse_rules("div: a / a => 1 if a != 0", "test").unwrap();
        assert!(rules[0].condition.is_some());
        assert!(rules[0].to_rewrite().is_ok());
    }

    #[test]
    fn catches_unbound_condition_variables() {
        let rules = parse_rules("bad: x => 0 if y != 0", "test").unwrap();
        assert!(rules[0].to_rewrite().is_err());
    }

    #[test]
    fn conditions_survive_bidirectional_rules() {
        let rules = parse_rules("flip: x + y <=> y + x if x != 0", "test").unwrap();
        assert_eq!(rules.len(), 2);
        assert!(rules.iter().all(|rule| rule.condition.is_some()));
    }
}
