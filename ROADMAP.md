# Roadmap

## Version 0.1
- Show commit hash when displaying version, unless it's a release load for GitHub
    - <crate-name> <major>.<minor>.<patch> (<git-hash>)
- Replace all instances of "expr-rs" or "sygg" with CARGO_PKG_NAME wherever possible
- Have the crate name, version and authors display above the description in the help output
- Add logs. Integrate existing egg logs with new logs for this crate with env_logger
    - Verbose flag (i.e. -v, -vvv)

## Version 0.2
- No postfix plus
- Greek letter insertions
    - A user types "theta" and it shows "→ θ" in dim, they press space and it converts theta into the symbol
    - The user can press escape to remove the "→ θ" and it won't convert
    - Other things could also be inserted, like infinity.
- Better error messages for syntax errors
    - This includes REPL syntax errors where it can print an arrow showing where the issue is and suggest tips on how to fix it
    - Rule file errors, this would look like more of a rustc style error with the line an arrow and brief description of the problem
- Strategy for exact values
    - Tries to preserve exact values as much as possible
    - If the user enters something like 0.2, it doesn't convert to rational but if an expression down the line produces 1/2 the strategy should prefer that over 0.5
- Add a where clause for type checking over is_const, is_real, etc.
    - Would go after the if clause if it exists, i.e. my-rule: v+a = a+v if a > 0 where v: vec, a: real
- Support multi-line rule definitions in rule files
    - Requires a custom rule-file parser
    - Goes hand-in-hand with the better error messages
    - New rule-file parser should still be recovering, but the recovery tokens are not as simple as just being a newline anymore

## Future versions    
- Support matching inside of vector/matrix expressions (i.e. f([1, a]) = [1, a+1])
- Add :explain command to show how expression is transformed 
    - Using egg's builtin explain functionality
    - Highlighting to show which subexpressions were matched and how they were transformed
    - Skip steps like folding
- Consider making functions part of env, i.e. you can define your own function f(x) = x^2 and the engine can use it
    - Make builtin functions part of env so you can disable them
- Add support for rule enabling/disabling
- Add support for defining a rule within a shell session
- Consider writing some form of VM for fast condition checking
- Try to minimize duplicate code for conditions and expressions
- Multi-threaded rule parsing and expression simplifying
    - Use egg's RewriteScheduler API
- Benchmark benchmark benchmark everything for performance optimizations