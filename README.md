# sygg

A symbolic algebra REPL and simplifier written in Rust, powered by
[egg](https://github.com/egraphs-good/egg).

`sygg` does not insist on one canonical "simplified" answer.  Every rule is
loaded into the same e-graph, and strategies differ only in the extraction
cost used to choose an equivalent form.  For example:

```text
expr › x * (y + z)
  simplified  →  x * (y + z)  [size 8]
  expanded    →  x * y + x * z  [size 11]

expr › x + x
  simplified  →  2 * x  [size 4]

expr › x * x
  simplified  →  x^2  [size 3]
```

## Features

- **Pratt parser** with full operator precedence, right-associative `^`,
  unary minus, function calls, vectors and matrices.
- **Precedence-aware pretty printing** with the minimum necessary
  parentheses.
- **Rewrite rules from files** using readable syntax such as
  `commute: x + y => y + x`, with bidirectional `<=>` and conditional
  `if ...` rules.
- **Variables**: `let pi = 3.14`, `let z = x + y`, with cycle detection.
- **Constant folding** via egg's `Analysis` trait for reals, vectors and
  matrices: arithmetic, functions, matrix products, powers, determinants,
  transposes and more.
- **Cost-based strategies**: all rules run together in one e-graph; a
  size/depth cost kind, per-node weights and optional structural penalties
  decide which equivalent form each strategy displays.  Duplicate results
  are collapsed.
- **Preset rule files** in `rules/` are loaded by default; use
  `--no-default-rules` to start with an empty rule set.
- **Modern crossterm REPL** with history, completion, multi-line input,
  Ctrl-C/Ctrl-D handling and word-wise editing.  Styling uses crossterm.
- **clap CLI** with repeatable `--eval`, startup rule files and resource
  limits for the e-graph runner.

## Build and run

```bash
cargo build --release
./target/release/sygg
```

Or with Cargo:

```bash
cargo run
```

Pass one or more rule files at startup:

```bash
cargo run -- examples/rules.rw
cargo run -- --no-default-rules -r my-rules.rw
```

Evaluate expressions without starting the REPL:

```bash
cargo run -- -e '2 + 3 * 4'
cargo run -- -e 'x * (y + z)'
cargo run -- -e '[1, 2] + [3, 4]'
cargo run -- -e '[[1, 2], [3, 4]] ^ 2'
```

`--version` shows the package version and, for non-release builds, the current
git hash.  Release builds omit the hash.

## REPL

```text
expr › let pi = 3.14
  pi = 3.14
expr › 2 * pi
  simplified  →  6.28  [size 1]
expr › :import examples/rules.rw
  loaded 5 rules from examples/rules.rw
expr › (a * b) ^ 2
  simplified  →  (a * b)^2  [size 5]
  shallow     →  a^2 * b^2  [depth 3]
```

Useful commands:

| Command | Meaning |
| --- | --- |
| `:help` | Show help |
| `:import <file>` | Load rewrite rules (alias `:load`) |
| `:rules` | List all loaded rules |
| `:strategies` | List strategies and their cost metric |
| `:env` | List variables |
| `:var <name> = <expr>` | Define a variable |
| `:del <name>` | Remove a variable |
| `:reset` | Remove all variables |
| `:clear` | Clear the screen |
| `:quit`, `:q` | Exit |

Keys:

- `Ctrl-C` exits the REPL.
- `Ctrl-D` exits on an empty line.
- `Up` / `Down` browse history (persisted between sessions).
- `Tab` completes commands, function names and variables.
- `Ctrl-A`/`Home`, `Ctrl-E`/`End`, word movement with `Ctrl-Left/Right` or
  `Alt-B`/`Alt-F`.
- `Ctrl-U`, `Ctrl-K`, `Ctrl-W` edit the line; `Ctrl-L` clears the screen.
- Unclosed `(`, `[` or a trailing `\` continues on the next line.

## Expression syntax

Precedence from lowest to highest:

| Operators | Associativity |
| --- | --- |
| `+`, `-` | left |
| `*`, `/` | left |
| unary `-`, `+` | prefix |
| `^` | right |

Examples:

```text
2 + 3 * 4        = 14
2 ^ 3 ^ 2        = 512      (right associative)
-2 ^ 2           = -4
(-2) ^ 2         = 4
a * (b + c)      keeps its parentheses
a * b + c        prints without extra parentheses
```

Vectors are written `[1, 2, 3]`; matrices use nested lists:

```text
[[1, 2], [3, 4]]
```

Function calls use `name(arg, ...)`.  Built-in functions are constant-folded
where possible:

`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`,
`exp`, `ln`, `log` (base 10, or `log(x, base)`), `log2`, `log10`, `sqrt`,
`cbrt`, `abs`, `floor`, `ceil`, `round`, `sign`, `min`, `max`, `dot`, `cross`,
`det`, `transpose`, `norm`.

Unknown function names are kept symbolically.

## Constants

Primitive constants are:

- real numbers, e.g. `3.14`, `-2`, `1e-6`
- vectors, e.g. `[1, 2, 3]`
- matrices, e.g. `[[1, 2], [3, 4]]`

Supported constant operations include:

- elementwise `+`, `-` for vectors and matrices
- scalar multiplication/division for vectors and matrices
- vector dot product for `u * v`
- matrix multiplication for `A * B`, `A * v`, `u * A`
- matrix power for integer exponents
- elementwise powers for vectors with a real exponent
- elementwise unary functions on vectors and matrices
- `det`, `transpose`, `norm`, `dot`, `cross`, `min`, `max`

Division by zero and out-of-domain values are left symbolic rather than
producing `NaN` or infinities.

## Rewrite rules

A rule file contains one rule per line.  Blank lines and `#` or `//` comments
are ignored.

```text
# Optional name, then `lhs => rhs`
commute: x + y => y + x
distribute: a * (b + c) => a * b + a * c

# Names are optional
x * 1 => x

# Bidirectional rules are supported
idempotent: x + x <=> 2 * x

# Conditional rules are evaluated against egg's analysis data
div-self: a / a => 1 if a != 0
pow-zero: a ^ 0 => 1 if a != 0
add-self: a + a => 2 * a
```

Bare identifiers in a rule are pattern variables and must be bound on the
left-hand side.  Conditions are a small expression language with arithmetic,
comparisons and boolean operators:

```text
if a != 0
if a + 1 > b * 2
if a == 0 or b != 0
```

Comparisons are `==`, `!=`, `<`, `<=`, `>`, `>=`.  `!=` means "not known to
be equal", so symbolic values satisfy `a != 0`; only an e-class known to be
zero fails.  Boolean operators are `and`/`&&`, `or`/`||`, and `not`/`!`.

The preset rule set lives in [`rules/`](rules/) and is loaded through the same
parser as user rule files.  It is read from disk when present, with embedded
copies as a fallback so the binary remains self-contained.  Import rules at
startup:

```bash
sygg my-rules.rw
```

or inside the REPL:

```text
expr › :import my-rules.rw
```

See [`examples/rules.rw`](examples/rules.rw) for a complete example.

## Variables

```text
expr › let pi = 3.14
expr › let z = x + y
expr › z * 2
  simplified  →  2 * (x + y)  [size 8]
  expanded    →  2 * x + 2 * y  [size 11]
```

Variables are unioned with their defining expressions in the e-graph, so they
compose with rewrite rules. Definitions are checked for cycles and may refer to
names that are defined later.

## Strategies

All strategies combine the preset and user rules into one rule set and run
them over one e-graph.  The difference in output comes only from the
extraction cost used after saturation stops.  A strategy is a name,
description, cost kind (`size` or `depth`) and a profile.  The default
profile encodes canonical preferences: products and sums are weighted above
leaves so repeated terms become coefficients and repeated factors become
powers (`x + x -> 2*x`, `x*x -> x^2`), while sums of products are penalized
so factoring wins when node counts are tied.  Each strategy may then adjust
the weights or penalties that match its purpose.

| Strategy | Purpose | Cost |
| --- | --- | --- |
| `simplified` | canonical weighted form | size with canonical weights |
| `expanded` | distributed products and powers | size with sum-in-product/power penalties |
| `factored` | products preferred over sums | size with higher add/sub weights |
| `shallow` | lowest nesting depth | depth with canonical weights |

Identical output strings are shown only once.  A stop-reason marker such as
`~iteration limit` means the runner stopped before saturation; tune the limits
with `--max-iterations`, `--max-nodes`, `--max-depth` and `--time-limit`.

Saturation uses egg's backoff scheduler for expensive rules and rejects
rewrites that would exceed the configured expression depth.  This keeps
inverse rules such as distribution and factoring from growing the e-graph
without bound while preserving a single, strategy-independent rule set.  The
engine also keeps at most one constant value in an e-class, so floating-point
rounding or underflow cannot make a literal `0`/`1` rule match an unrelated
symbolic term.

## Logging

`sygg` writes to `sygg.log` in the same directory as its history file:

- with `XDG_STATE_HOME` set: `$XDG_STATE_HOME/sygg/sygg.log`
- otherwise: `$HOME/sygg.log`

The log file is appended to on each run.  Its verbosity is controlled by
repeating `-v`:

| Flag | Level |
| --- | --- |
| none | `warn` |
| `-v` | `info` |
| `-vv` | `debug` |
| `-vvv` | `trace` |

Egg's internal logs use the same logger, so `-vv` and above also show e-graph
activity.

## Architecture

- `src/` contains the reusable library: parser, pretty printer, egg language,
  analysis, rewrite parser, condition language and simplification engine.
- `bin/` contains the frontend: clap CLI, crossterm REPL, line editing,
  history and terminal styling.

## Development

```bash
cargo test
cargo clippy --all-targets
```
