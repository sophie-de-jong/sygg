//! The simplification engine: strategies, egg runs and cost-based extraction.

use core::fmt;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use egg::{
    BackoffScheduler, CostFunction, EGraph, Extractor, Id, Language, RecExpr, Runner, StopReason,
};
use log::{debug, trace};

use crate::analysis::{ConstantFolding, MathRewrite};
use crate::ast::{Ast, parse_expression, pretty_ast, pretty_recexpr};
use crate::constant::Constant;
use crate::error::{AlgebraError, Result};
use crate::language::Math;
use crate::rewrite::{Rule, parse_rules, parse_rules_file};

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub max_iterations: usize,
    pub max_nodes: usize,
    pub time_limit: Duration,
    pub max_depth: usize,
    pub load_default_rules: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_iterations: 15,
            max_nodes: 20_000,
            time_limit: Duration::from_millis(750),
            max_depth: 32,
            load_default_rules: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VarDef {
    pub name: String,
    pub value: Ast,
}

#[derive(Clone, Debug)]
pub struct StrategyResult {
    pub strategy: &'static str,
    pub description: &'static str,
    pub expression: String,
    pub cost: usize,
    pub cost_kind: CostKind,
    pub stop_reason: StopReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostKind {
    Size,
    Depth,
}

impl fmt::Display for CostKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Size => f.write_str("size"),
            Self::Depth => f.write_str("depth"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Const,
    Symbol,
    Sum,
    Product,
    Quotient,
    Power,
    Negation,
    Call,
    Vector,
}

impl From<&Math> for NodeKind {
    fn from(enode: &Math) -> Self {
        match enode {
            Math::Const(_) => Self::Const,
            Math::Symbol(_) => Self::Symbol,
            Math::Add(_) | Math::Sub(_) => Self::Sum,
            Math::Mul(_) => Self::Product,
            Math::Div(_) => Self::Quotient,
            Math::Pow(_) => Self::Power,
            Math::Neg(_) => Self::Negation,
            Math::Call(_, _) => Self::Call,
            Math::VecLit(_) => Self::Vector,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cost {
    pub value: usize,
    pub kind: NodeKind,
    negative_constant: bool,
}

impl PartialEq for Cost {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

#[derive(Clone, Copy, Debug)]
struct Profile {
    leaf: usize,
    add: usize,
    sub: usize,
    mul: usize,
    div: usize,
    pow: usize,
    neg: usize,
    call: usize,
    vec: usize,
    mul_sum_penalty: usize,
    pow_sum_penalty: usize,
    sum_product_penalty: usize,
    sum_power_penalty: usize,
    negative_power_penalty: usize,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            leaf: 1,
            // Prefer canonical coefficients and powers over repeated nodes:
            // x + x -> 2 * x, x * x -> x^2.
            add: 3,
            sub: 3,
            mul: 2,
            div: 1,
            pow: 1,
            neg: 1,
            call: 1,
            vec: 1,
            mul_sum_penalty: 0,
            pow_sum_penalty: 0,
            // Prefer factored sums when node counts are tied.
            sum_product_penalty: 1,
            sum_power_penalty: 0,
            // `x^(-1)` and `1 / x` have the same node count; prefer the
            // reciprocal, which is the conventional simplified form.
            negative_power_penalty: 1,
        }
    }
}

impl Profile {
    fn weight(&self, enode: &Math) -> usize {
        match enode {
            Math::Add(_) => self.add,
            Math::Sub(_) => self.sub,
            Math::Mul(_) => self.mul,
            Math::Div(_) => self.div,
            Math::Pow(_) => self.pow,
            Math::Neg(_) => self.neg,
            Math::Call(_, _) => self.call,
            Math::VecLit(_) => self.vec,
            Math::Const(_) | Math::Symbol(_) => self.leaf,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Strategy {
    name: &'static str,
    description: &'static str,
    cost_kind: CostKind,
    profile: Profile,
}

impl Strategy {
    pub fn name(&self) -> &str {
        self.name
    }

    pub fn description(&self) -> &str {
        self.description
    }

    pub fn cost_kind(&self) -> CostKind {
        self.cost_kind
    }
}

impl CostFunction<Math> for Strategy {
    type Cost = Cost;

    fn cost<C>(&mut self, enode: &Math, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let kind = NodeKind::from(enode);
        let mut value = self.profile.weight(enode);
        let mut max_child = 0;
        let mut first_child_kind = None;
        let mut child_has_sum = false;
        let mut child_has_product = false;
        let mut child_has_power = false;
        let mut negative_exponent = false;

        for (index, child_id) in enode.children().iter().enumerate() {
            let child = costs(*child_id);

            if index == 0 {
                first_child_kind = Some(child.kind);
            }
            if index == 1 && child.negative_constant {
                negative_exponent = true;
            }
            if child.kind == NodeKind::Sum {
                child_has_sum = true;
            }
            if child.kind == NodeKind::Product {
                child_has_product = true;
            }
            if child.kind == NodeKind::Power {
                child_has_power = true;
            }

            match self.cost_kind {
                CostKind::Size => value = value.saturating_add(child.value),
                CostKind::Depth => max_child = max_child.max(child.value),
            }
        }

        if self.cost_kind == CostKind::Depth {
            value = self.profile.weight(enode).saturating_add(max_child);
        }

        if kind == NodeKind::Product && child_has_sum {
            value = value.saturating_add(self.profile.mul_sum_penalty);
        }
        if kind == NodeKind::Power && first_child_kind == Some(NodeKind::Sum) {
            value = value.saturating_add(self.profile.pow_sum_penalty);
        }
        if kind == NodeKind::Power && negative_exponent {
            value = value.saturating_add(self.profile.negative_power_penalty);
        }
        if kind == NodeKind::Sum && child_has_product {
            value = value.saturating_add(self.profile.sum_product_penalty);
        }
        if kind == NodeKind::Sum && child_has_power {
            value = value.saturating_add(self.profile.sum_power_penalty);
        }

        let negative_constant = matches!(
            enode,
            Math::Const(Constant::Real(real)) if real.get() < 0.0
        );

        Cost {
            value,
            kind,
            negative_constant,
        }
    }
}

static STRATEGIES: LazyLock<Vec<Strategy>> = LazyLock::new(|| {
    vec![
        Strategy {
            name: "simplified",
            description: "compact weighted form",
            cost_kind: CostKind::Size,
            profile: Profile::default(),
        },
        Strategy {
            name: "expanded",
            description: "distributed products and powers",
            cost_kind: CostKind::Size,
            profile: Profile {
                mul_sum_penalty: 10_000,
                pow_sum_penalty: 10_000,
                sum_product_penalty: 0,
                ..Profile::default()
            },
        },
        Strategy {
            name: "factored",
            description: "products preferred over sums",
            cost_kind: CostKind::Size,
            profile: Profile {
                add: 4,
                sub: 4,
                sum_power_penalty: 10_000,
                ..Profile::default()
            },
        },
        Strategy {
            name: "shallow",
            description: "lowest nesting depth",
            cost_kind: CostKind::Depth,
            profile: Profile::default(),
        },
    ]
});

pub struct DefaultRuleFile {
    pub name: &'static str,
    pub source: &'static str,
}

const DEFAULT_RULE_FILES: &[DefaultRuleFile] = &[
    DefaultRuleFile {
        name: "rules/core.rw",
        source: include_str!("../rules/core.rw"),
    },
    DefaultRuleFile {
        name: "rules/normalize.rw",
        source: include_str!("../rules/normalize.rw"),
    },
    DefaultRuleFile {
        name: "rules/expanded.rw",
        source: include_str!("../rules/expanded.rw"),
    },
    DefaultRuleFile {
        name: "rules/factored.rw",
        source: include_str!("../rules/factored.rw"),
    },
];

pub struct AlgebraEngine {
    config: EngineConfig,
    rules: Vec<Rule>,
    vars: Vec<VarDef>,
}

impl AlgebraEngine {
    pub fn new(config: EngineConfig) -> Self {
        let load_defaults = config.load_default_rules;
        let mut engine = Self {
            config,
            rules: Vec::new(),
            vars: Vec::new(),
        };
        if load_defaults {
            engine
                .load_default_rules()
                .expect("preset rule files are valid");
        }
        log::debug!("engine initialized with {} rule(s)", engine.rules.len());
        engine
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn vars(&self) -> &[VarDef] {
        &self.vars
    }

    pub fn load_default_rules(&mut self) -> Result<usize> {
        let mut total = 0;
        for file in DEFAULT_RULE_FILES {
            // Prefer the rules on disk so a checkout can be edited without
            // rebuilding.  Embedded copies keep the binary self-contained
            // when it is run outside the project directory.
            let source = match std::fs::read_to_string(file.name) {
                Ok(source) => source,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    file.source.to_string()
                }
                Err(error) => return Err(error.into()),
            };
            let rules = parse_rules(&source, file.name)?;
            let count = self.add_rules(rules)?;
            log::debug!("loaded {count} preset rule(s) from {}", file.name);
            total += count;
        }
        log::info!("loaded {total} preset rule(s)");
        Ok(total)
    }

    pub fn strategies(&self) -> impl Iterator<Item = &Strategy> + '_ {
        STRATEGIES.iter()
    }

    pub fn load_rules_file(&mut self, path: &Path) -> Result<usize> {
        let rules = parse_rules_file(path)?;
        let count = self.add_rules(rules)?;
        log::info!("loaded {count} rule(s) from {}", path.display());
        Ok(count)
    }

    pub fn import_rules(&mut self, source: &str, origin: &str) -> Result<usize> {
        let rules = parse_rules(source, origin)?;
        let count = self.add_rules(rules)?;
        log::info!("loaded {count} rule(s) from {origin}");
        Ok(count)
    }

    fn add_rules(&mut self, rules: Vec<Rule>) -> Result<usize> {
        // Validate first so a broken import does not partially change the
        // active rule set.
        for rule in &rules {
            rule.to_rewrite()?;
        }

        let count = rules.len();
        for mut rule in rules {
            rule.name = self.unique_rule_name(&rule.name);
            log::trace!("adding rule {} from {}", rule.name, rule.origin);
            self.rules.push(rule);
        }
        log::debug!("added {count} rule(s)");
        Ok(count)
    }

    fn unique_rule_name(&self, requested: &str) -> String {
        let mut candidate = requested.to_string();
        let mut suffix = 2;
        loop {
            if !self.rules.iter().any(|rule| rule.name == candidate) {
                return candidate;
            }
            candidate = format!("{requested}-{suffix}");
            suffix += 1;
        }
    }

    pub fn define_var(&mut self, name: &str, value: Ast) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AlgebraError::Message(
                "variable name cannot be empty".to_string(),
            ));
        }
        if name.starts_with('?') {
            return Err(AlgebraError::Message(
                "variable names cannot start with `?`".to_string(),
            ));
        }

        let mut candidate = self.vars.clone();
        if let Some(existing) = candidate
            .iter_mut()
            .find(|definition| definition.name == name)
        {
            existing.value = value;
        } else {
            candidate.push(VarDef {
                name: name.to_string(),
                value,
            });
        }

        if let Some(cycle) = find_var_cycle(&candidate) {
            return Err(AlgebraError::Message(format!(
                "cycle detected in variable definitions: {cycle}"
            )));
        }

        self.vars = candidate;
        log::debug!("defined var {name}");
        Ok(())
    }

    pub fn remove_var(&mut self, name: &str) -> bool {
        let before = self.vars.len();
        self.vars.retain(|definition| definition.name != name);
        before != self.vars.len()
    }

    pub fn clear_vars(&mut self) {
        self.vars.clear();
    }

    pub fn evaluate(&self, input: &str) -> Result<Vec<StrategyResult>> {
        let ast = parse_expression(input)?;
        self.simplify_ast(&ast)
    }

    pub fn simplify_ast(&self, ast: &Ast) -> Result<Vec<StrategyResult>> {
        debug!("simplifying {}", pretty_ast(ast));
        let expression = ast.to_recexpr();
        let rules = self
            .rules
            .iter()
            .map(|rule| rule.to_bounded_rewrite(self.config.max_depth))
            .collect::<Result<Vec<_>>>()?;

        // One strategy-independent saturation run.  Every strategy extracts
        // from the same e-graph; only its cost function differs.
        let runner = self.run_with_rules(&expression, &rules)?;
        let root = runner
            .roots
            .first()
            .copied()
            .ok_or_else(|| AlgebraError::Message("egg did not produce a root".to_string()))?;
        let stop_reason = runner
            .stop_reason
            .clone()
            .ok_or_else(|| AlgebraError::Message("egg did not report a stop reason".to_string()))?;

        let mut results = Vec::new();
        let mut seen = HashSet::new();

        for strategy in STRATEGIES.iter() {
            let extractor = Extractor::new(&runner.egraph, strategy.clone());
            let (cost, best) = extractor.find_best(root);

            let result = StrategyResult {
                strategy: strategy.name,
                description: strategy.description,
                expression: pretty_recexpr(&best),
                cost: cost.value,
                cost_kind: strategy.cost_kind,
                stop_reason: stop_reason.clone(),
            };

            trace!(
                "strategy {} produced {} ({} {})",
                result.strategy, result.expression, result.cost_kind, result.cost
            );

            if seen.insert(result.expression.clone()) {
                results.push(result);
            }
        }

        Ok(results)
    }

    fn run_with_rules(
        &self,
        expression: &RecExpr<Math>,
        rules: &[MathRewrite],
    ) -> Result<Runner<Math, ConstantFolding, ()>> {
        // Variables are represented as ordinary symbols in the e-graph.
        // Union each symbol with its definition so congruence, rewrites and
        // constant folding see the definition without a separate AST pass.
        let analysis = ConstantFolding::new(self.config.max_depth);
        let mut egraph = EGraph::<Math, ConstantFolding>::new(analysis);
        let mut variable_ids = Vec::new();
        for variable in &self.vars {
            let variable_id = egraph.add(Math::Symbol(variable.name.as_str().into()));
            let value_id = egraph.add_expr(&variable.value.to_recexpr());
            egraph.union(variable_id, value_id);
            variable_ids.push((variable.name.as_str(), variable_id));
        }
        egraph.rebuild();

        let mut runner = Runner::<Math, ConstantFolding, ()>::default()
            .with_egraph(egraph)
            .with_expr(expression)
            .with_iter_limit(self.config.max_iterations)
            .with_node_limit(self.config.max_nodes)
            .with_time_limit(self.config.time_limit)
            .with_scheduler(
                BackoffScheduler::default()
                    .with_initial_match_limit(256)
                    .with_ban_length(5),
            )
            .run(rules);

        debug!(
            "runner finished after {} iteration(s), {} e-node(s), stop reason {:?}",
            runner.iterations.len(),
            runner.egraph.total_size(),
            runner.stop_reason
        );

        // Variable symbols were only needed to seed the unions.
        for (name, variable_id) in variable_ids {
            let class_id = runner.egraph.find(variable_id);
            runner.egraph[class_id]
                .nodes
                .retain(|node| !matches!(node, Math::Symbol(symbol) if symbol.as_str() == name));
        }

        Ok(runner)
    }
}

fn find_var_cycle(vars: &[VarDef]) -> Option<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Visiting,
        Done,
    }

    fn visit(
        name: &str,
        definitions: &HashMap<&str, &VarDef>,
        states: &mut HashMap<String, State>,
        stack: &mut Vec<String>,
    ) -> Option<String> {
        match states.get(name) {
            Some(State::Done) => return None,
            Some(State::Visiting) => {
                let mut cycle = stack.clone();
                cycle.push(name.to_string());
                return Some(cycle.join(" -> "));
            }
            None => {}
        }

        states.insert(name.to_string(), State::Visiting);
        stack.push(name.to_string());

        if let Some(definition) = definitions.get(name) {
            let mut dependencies = Vec::new();
            definition.value.symbols(&mut dependencies);
            for dependency in dependencies {
                if definitions.contains_key(dependency.as_str())
                    && let Some(cycle) = visit(&dependency, definitions, states, stack)
                {
                    return Some(cycle);
                }
            }
        }

        stack.pop();
        states.insert(name.to_string(), State::Done);
        None
    }

    let definitions = vars
        .iter()
        .map(|definition| (definition.name.as_str(), definition))
        .collect::<HashMap<_, _>>();
    let mut states = HashMap::new();
    let mut stack = Vec::new();

    for definition in vars {
        if let Some(cycle) = visit(
            definition.name.as_str(),
            &definitions,
            &mut states,
            &mut stack,
        ) {
            return Some(cycle);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::parse_expression;

    fn engine() -> AlgebraEngine {
        AlgebraEngine::new(EngineConfig::default())
    }

    fn outputs(input: &str) -> Vec<String> {
        engine()
            .evaluate(input)
            .unwrap()
            .into_iter()
            .map(|result| result.expression)
            .collect()
    }

    #[test]
    fn folds_constants() {
        assert_eq!(outputs("1 + 2 * 3"), vec!["7"]);
        assert_eq!(outputs("2 ^ 10"), vec!["1024"]);
        assert_eq!(outputs("sqrt(81) + abs(-4)"), vec!["13"]);
    }

    #[test]
    fn folds_vectors_and_matrices() {
        assert_eq!(outputs("[1, 2] + [3, 4]"), vec!["[4, 6]"]);
        assert_eq!(
            outputs("[[1, 2], [3, 4]] * [[5, 6], [7, 8]]"),
            vec!["[[19, 22], [43, 50]]"]
        );
    }

    #[test]
    fn expanded_strategy_shows_distributed_form() {
        let results = engine().evaluate("x * (y + z)").unwrap();
        assert!(
            results
                .iter()
                .any(|result| result.strategy == "expanded"
                    && result.expression == "x * y + x * z"),
            "{results:#?}"
        );
        assert_eq!(outputs("2 ^ 10"), vec!["1024"]);
    }

    #[test]
    fn canonicalizes_repeated_terms_and_factors() {
        assert_eq!(outputs("x * x"), vec!["x^2"]);
        assert_eq!(outputs("x + x"), vec!["2 * x"]);
        let repeated = outputs("x * y * x");
        assert!(
            repeated.iter().any(|value| value.contains("x^2")),
            "{repeated:?}"
        );
        assert!(
            repeated.iter().all(|value| !value.contains("x * x")),
            "{repeated:?}"
        );
        assert_eq!(outputs("x * x * x"), vec!["x^3"]);

        let factored = outputs("x * y + x");
        assert!(
            factored
                .iter()
                .any(|value| value.contains("x * (") && value.contains("y + 1")),
            "{factored:?}"
        );
    }

    #[test]
    fn var_bindings_union() {
        let mut engine = engine();
        engine
            .define_var("x", parse_expression("2").unwrap())
            .unwrap();
        engine
            .define_var("z", parse_expression("x + y").unwrap())
            .unwrap();
        let results = engine.evaluate("z * 2").unwrap();
        assert!(results.iter().any(|result| result.expression.contains('y')));
        assert!(
            results
                .iter()
                .all(|result| !result.expression.contains('z'))
        );
    }

    #[test]
    fn factoring_and_depth_differ() {
        let results = engine().evaluate("x * (y + z)").unwrap();
        let expressions = results
            .iter()
            .map(|result| result.expression.clone())
            .collect::<Vec<_>>();
        assert!(expressions.iter().any(|value| value.contains("x * y")));
        assert!(expressions.iter().any(|value| value.contains("y + z")));
    }

    #[test]
    fn conditional_rules_respect_analysis_data() {
        let mut engine = AlgebraEngine::new(EngineConfig {
            load_default_rules: false,
            ..EngineConfig::default()
        });
        engine
            .import_rules("div: a / a => 1 if a != 0", "test")
            .unwrap();
        assert_eq!(engine.evaluate("x / x").unwrap()[0].expression, "1");
        assert_eq!(engine.evaluate("0 / 0").unwrap()[0].expression, "0 / 0");
    }

    #[test]
    fn no_default_rules_means_no_rewrites() {
        let engine = AlgebraEngine::new(EngineConfig {
            load_default_rules: false,
            ..EngineConfig::default()
        });
        assert!(engine.rules().is_empty());
        assert_eq!(engine.evaluate("x + 0").unwrap()[0].expression, "x + 0");
    }

    #[test]
    fn profile_defaults_are_canonical() {
        let profile = Profile::default();
        assert_eq!(profile.leaf, 1);
        assert_eq!(profile.add, 3);
        assert_eq!(profile.mul, 2);
        assert_eq!(profile.pow, 1);
        assert_eq!(profile.mul_sum_penalty, 0);
        assert_eq!(profile.sum_product_penalty, 1);
        assert_eq!(profile.sum_power_penalty, 0);
    }

    #[test]
    fn results_expose_cost_kind_and_stop_reason() {
        let results = engine().evaluate("x + 0").unwrap();
        let result = &results[0];
        assert_eq!(result.cost_kind, CostKind::Size);
        assert!(
            matches!(
                result.stop_reason,
                StopReason::Saturated
                    | StopReason::IterationLimit(_)
                    | StopReason::NodeLimit(_)
                    | StopReason::TimeLimit(_)
                    | StopReason::Other(_)
            ),
            "{:?}",
            result.stop_reason
        );
    }

    #[test]
    fn detects_cycles() {
        let mut engine = engine();
        engine
            .define_var("x", parse_expression("y").unwrap())
            .unwrap();
        assert!(
            engine
                .define_var("y", parse_expression("x + 1").unwrap())
                .is_err()
        );
    }
}
