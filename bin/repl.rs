//! Interactive crossterm REPL and shared line processing.

use std::env;
use std::error::Error;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use crate::{CARGO_PKG_NAME, VERSION_AND_GIT_HASH};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::Stylize;
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, style::Print};
use unicode_width::UnicodeWidthChar;

use sygg::StopReason;
use sygg::ast::{parse_let, pretty_ast, pretty_ast_exact, starts_with_let};
use sygg::engine::{AlgebraEngine, StrategyResult};
use sygg::functions::function_names;

const COMMANDS: &[&str] = &[
    ":help",
    ":quit",
    ":q",
    ":exit",
    ":import",
    ":load",
    ":rules",
    ":strategies",
    ":env",
    ":vars",
    ":var",
    ":del",
    ":delete",
    ":reset",
    ":clear",
    ":version",
];

const HELP: &str = r#"
  Input
    <expression>             simplify using several strategies
    let <name> = <expr>      define a variable (e.g. let pi = 3.14)
    :var <name> = <expr>     same as above
    :import <file>           load rewrite rules from a file

  Rewrite rules
    Each line in a rules file has the form
        name: left-hand side => right-hand side
    Bare identifiers are pattern variables, so this is valid:
        commute: x + y => y + x
    Rules may be unnamed, bidirectional with <=>, and conditional:
        div-self: a / a => 1 if a != 0
        factor: a * b + a * c => a * (b + c)
    Conditions support arithmetic, comparisons (==, !=, <, <=, >, >=) and
    boolean operators (and / or / not).
    Comments start with # or //.

  Commands
    :help                    show this help
    :rules                   list loaded rules
    :strategies              list simplification strategies and costs
    :env                     list variables
    :del <name>              remove a variable
    :reset                   remove all variables
    :clear                   clear the screen
    :quit, :q                exit
"#;

#[derive(Clone, Copy, Debug)]
pub struct ReplOptions {
    pub banner: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

/// Run the interactive REPL, or a plain line-oriented loop when stdin is not
/// a terminal.
pub fn run(engine: &mut AlgebraEngine, options: ReplOptions) -> Result<(), Box<dyn Error>> {
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();

    if !interactive {
        log::debug!("stdin is not a terminal; using plain input mode");
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line?;
            if process_line(engine, &line, false) == Flow::Quit {
                break;
            }
        }
        return Ok(());
    }

    if options.banner {
        print_banner(engine);
    }

    let mut editor = LineEditor::new();
    let primary_prompt = format!("{} {} ", "expr".cyan(), "›".dim());
    let continuation_prompt = format!("{} {} ", "....".dim(), "›".dim());

    loop {
        let candidates = completion_candidates(engine);
        let outcome =
            editor.read_complete_line(&primary_prompt, &continuation_prompt, &candidates)?;

        match outcome {
            ReadOutcome::Line(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                editor.remember(&line);
                if process_line(engine, &line, true) == Flow::Quit {
                    break;
                }
            }
            ReadOutcome::Eof | ReadOutcome::Interrupted => break,
        }
    }

    editor.save_history();
    Ok(())
}

fn print_banner(engine: &AlgebraEngine) {
    println!(
        "{} {}",
        CARGO_PKG_NAME.cyan().bold(),
        format!(
            "v{} · symbolic algebra powered by egg",
            VERSION_AND_GIT_HASH
        )
        .dim()
    );
    println!(
        "{}",
        format!(
            "{} rules · {} variables",
            engine.rules().len(),
            engine.vars().len()
        )
        .dim()
    );
    println!("{}", "Type :help for commands".dim());
    println!();
}

fn completion_candidates(engine: &AlgebraEngine) -> Vec<String> {
    let mut candidates = COMMANDS
        .iter()
        .map(|command| (*command).to_string())
        .collect::<Vec<_>>();
    candidates.extend(function_names().map(str::to_string));
    candidates.extend(
        engine
            .vars()
            .iter()
            .map(|definition| definition.name.clone()),
    );
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Process one input line.  This is shared by the interactive shell, the
/// piped-input mode and `--eval`.
pub fn process_line(engine: &mut AlgebraEngine, input: &str, interactive: bool) -> Flow {
    let trimmed = input.trim();
    log::debug!("input: {trimmed}");
    if trimmed.is_empty() {
        return Flow::Continue;
    }

    if let Some(command) = trimmed.strip_prefix(':') {
        return process_command(engine, command, interactive);
    }

    if starts_with_let(trimmed) {
        match parse_let(trimmed) {
            Ok((name, value)) => match engine.define_var(&name, value.clone()) {
                Ok(()) => println!(
                    "  {} {} {}",
                    name.as_str().cyan(),
                    "=".dim(),
                    pretty_ast(&value).green()
                ),
                Err(error) => print_error(error),
            },
            Err(error) => print_error(error),
        }
        return Flow::Continue;
    }

    match engine.evaluate(trimmed) {
        Ok(results) => print_results(&results),
        Err(error) => print_error(error),
    }
    Flow::Continue
}

fn process_command(engine: &mut AlgebraEngine, command_line: &str, interactive: bool) -> Flow {
    let mut parts = command_line.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    match command {
        "quit" | "q" | "exit" => return Flow::Quit,
        "help" => println!("{HELP}"),
        "import" | "load" => {
            if rest.is_empty() {
                print_error("usage: :import <file>");
            } else {
                let path = unquote(rest);
                match engine.load_rules_file(path.as_ref()) {
                    Ok(count) => print_note(format!(
                        "loaded {count} rule{} from {}",
                        if count == 1 { "" } else { "s" },
                        path.display()
                    )),
                    Err(error) => print_error(error),
                }
            }
        }
        "rules" => print_rules(engine),
        "strategies" => print_strategies(engine),
        "env" | "vars" => print_env(engine),
        "var" => {
            let input = format!("let {rest}");
            match parse_let(&input) {
                Ok((name, value)) => match engine.define_var(&name, value.clone()) {
                    Ok(()) => println!(
                        "  {} {} {}",
                        name.as_str().cyan(),
                        "=".dim(),
                        pretty_ast(&value).green()
                    ),
                    Err(error) => print_error(error),
                },
                Err(error) => print_error(error),
            }
        }
        "del" | "delete" => {
            if rest.is_empty() {
                print_error("usage: :del <name>");
            } else if engine.remove_var(rest) {
                print_note(format!("removed `{rest}`"));
            } else {
                print_error(format!("no such variable `{rest}`"));
            }
        }
        "reset" => {
            engine.clear_vars();
            print_note("cleared all variables");
        }
        "clear" => {
            if interactive {
                let mut stdout = io::stdout();
                let _ = stdout.flush();
                let _ = execute!(
                    stdout,
                    terminal::Clear(ClearType::All),
                    cursor::MoveTo(0, 0)
                );
            }
        }
        "version" => println!("  {} {}", CARGO_PKG_NAME.bold(), VERSION_AND_GIT_HASH),
        "" => {}
        other => print_error(format!("unknown command `:{other}`; try :help")),
    }

    Flow::Continue
}

fn print_error(message: impl std::fmt::Display) {
    println!("  {} {}", "error:".red(), message);
}

fn print_note(message: impl std::fmt::Display) {
    println!("  {}", message.to_string().dim());
}

fn print_results(results: &[StrategyResult]) {
    if results.is_empty() {
        println!("  {}", "(no result)".dim());
        return;
    }

    let label_width = results
        .iter()
        .map(|result| result.strategy.len())
        .max()
        .unwrap_or(0);

    for result in results {
        let label = format!("{:<label_width$}", result.strategy);
        let cost = format!("[{} {}]", result.cost_kind, result.cost);
        let limit = match &result.stop_reason {
            StopReason::Saturated => String::new(),
            StopReason::IterationLimit(_) => " ~iteration limit".dim().to_string(),
            StopReason::NodeLimit(_) => " ~node limit".dim().to_string(),
            StopReason::TimeLimit(_) => " ~time limit".dim().to_string(),
            StopReason::Other(reason) => format!(" ~{reason}").dim().to_string(),
        };
        println!(
            "  {}  {}  {}  {}{}",
            label.cyan(),
            "→".dim(),
            result.expression.as_str().green(),
            cost.dim(),
            limit
        );
    }
}

fn print_rules(engine: &AlgebraEngine) {
    if engine.rules().is_empty() {
        print_note("no rules loaded");
    } else {
        println!(
            "  {}",
            format!(
                "{} rule{}",
                engine.rules().len(),
                if engine.rules().len() == 1 { "" } else { "s" }
            )
            .dim()
        );
        let width = engine
            .rules()
            .iter()
            .map(|rule| rule.name.len())
            .max()
            .unwrap_or(0);
        for rule in engine.rules() {
            print!(
                "  {}  {} {} {}",
                format!("{:<width$}", rule.name).cyan(),
                pretty_ast_exact(&rule.lhs),
                "=>".dim(),
                pretty_ast_exact(&rule.rhs)
            );
            if let Some(condition) = &rule.condition {
                println!(" {} {}", "if".dim(), condition);
            } else {
                println!()
            }
            println!("  {}  {}", " ".repeat(width), rule.origin.as_str().dim());
        }
    }
}

fn print_strategies(engine: &AlgebraEngine) {
    println!("  {}", "strategies".dim());
    let width = engine
        .strategies()
        .map(|strategy| strategy.name().len())
        .max()
        .unwrap_or(0);
    for strategy in engine.strategies() {
        println!(
            "  {}  {}  {}",
            format!("{:<width$}", strategy.name()).cyan(),
            strategy.description(),
            format!("[{}]", strategy.cost_kind()).dim()
        );
    }
}

fn print_env(engine: &AlgebraEngine) {
    if engine.vars().is_empty() {
        print_note("no variables");
        return;
    }
    println!("  {}", "variables".dim());
    for definition in engine.vars() {
        println!(
            "  {} {} {}",
            definition.name.as_str().cyan(),
            "=".dim(),
            pretty_ast(&definition.value).green()
        );
    }
}

fn unquote(text: &str) -> PathBuf {
    let text = text.trim();
    let unquoted = if text.len() >= 2
        && ((text.starts_with('"') && text.ends_with('"'))
            || (text.starts_with('\'') && text.ends_with('\'')))
    {
        &text[1..text.len() - 1]
    } else {
        text
    };
    PathBuf::from(unquoted)
}

#[derive(Debug)]
enum ReadOutcome {
    Line(String),
    Interrupted,
    Eof,
}

struct LineEditor {
    history: History,
}

impl LineEditor {
    fn new() -> Self {
        Self {
            history: History::load(),
        }
    }

    fn remember(&mut self, line: &str) {
        self.history.remember(line);
    }

    fn save_history(&self) {
        self.history.save();
    }

    fn read_complete_line(
        &mut self,
        primary_prompt: &str,
        continuation_prompt: &str,
        completions: &[String],
    ) -> io::Result<ReadOutcome> {
        let mut full = String::new();

        loop {
            let prompt = if full.is_empty() {
                primary_prompt
            } else {
                continuation_prompt
            };

            match self.read_line(prompt, completions)? {
                ReadOutcome::Line(mut line) => {
                    let explicit_continuation = line.trim_end().ends_with('\\');
                    if explicit_continuation {
                        let position = line.rfind('\\').unwrap();
                        line.remove(position);
                    }
                    if !full.is_empty() {
                        full.push('\n');
                    }
                    full.push_str(&line);

                    if !explicit_continuation && is_complete(&full) {
                        return Ok(ReadOutcome::Line(full));
                    }
                }
                ReadOutcome::Interrupted => return Ok(ReadOutcome::Interrupted),
                ReadOutcome::Eof => return Ok(ReadOutcome::Eof),
            }
        }
    }

    fn read_line(&mut self, prompt: &str, completions: &[String]) -> io::Result<ReadOutcome> {
        let _raw_mode = RawMode::enable()?;
        let mut stdout = io::stdout();
        let mut buffer: Vec<char> = Vec::new();
        let mut cursor = 0usize;
        let mut history_index = self.history.len();
        let mut draft: Option<Vec<char>> = None;

        render(&mut stdout, prompt, &buffer, cursor)?;

        loop {
            let Event::Key(KeyEvent {
                code,
                modifiers,
                kind,
                ..
            }) = event::read()?
            else {
                continue;
            };
            if kind == KeyEventKind::Release {
                continue;
            }

            if modifiers.contains(KeyModifiers::CONTROL) {
                match code {
                    KeyCode::Char('c') => {
                        execute!(stdout, Print("^C\r\n"))?;
                        return Ok(ReadOutcome::Interrupted);
                    }
                    KeyCode::Char('d') => {
                        if buffer.is_empty() {
                            execute!(stdout, Print("\r\n"))?;
                            return Ok(ReadOutcome::Eof);
                        }
                        if cursor < buffer.len() {
                            buffer.remove(cursor);
                        }
                    }
                    KeyCode::Char('a') => cursor = 0,
                    KeyCode::Char('e') => cursor = buffer.len(),
                    KeyCode::Char('b') | KeyCode::Left => cursor = word_left(&buffer, cursor),
                    KeyCode::Char('f') | KeyCode::Right => cursor = word_right(&buffer, cursor),
                    KeyCode::Char('h') => {
                        if cursor > 0 {
                            cursor -= 1;
                            buffer.remove(cursor);
                        }
                    }
                    KeyCode::Char('k') => {
                        buffer.truncate(cursor);
                    }
                    KeyCode::Char('u') => {
                        buffer.clear();
                        cursor = 0;
                    }
                    KeyCode::Char('w') => {
                        let start = word_left(&buffer, cursor);
                        buffer.drain(start..cursor);
                        cursor = start;
                    }
                    KeyCode::Char('l') => {
                        execute!(
                            stdout,
                            terminal::Clear(ClearType::All),
                            cursor::MoveTo(0, 0)
                        )?;
                    }
                    KeyCode::Char('p') => {
                        if let Some(previous) =
                            history_up(&self.history, &mut history_index, &mut draft, &buffer)
                        {
                            buffer = previous;
                            cursor = buffer.len();
                        }
                    }
                    KeyCode::Char('n') => {
                        if let Some(next) =
                            history_down(&self.history, &mut history_index, &mut draft, &buffer)
                        {
                            buffer = next;
                            cursor = buffer.len();
                        }
                    }
                    _ => {}
                }
                render(&mut stdout, prompt, &buffer, cursor)?;
                continue;
            }

            if modifiers.contains(KeyModifiers::ALT) {
                match code {
                    KeyCode::Char('b') => cursor = word_left(&buffer, cursor),
                    KeyCode::Char('f') => cursor = word_right(&buffer, cursor),
                    _ => {}
                }
                render(&mut stdout, prompt, &buffer, cursor)?;
                continue;
            }

            match code {
                KeyCode::Enter => {
                    let line = buffer.iter().collect::<String>();
                    execute!(stdout, Print("\r\n"))?;
                    return Ok(ReadOutcome::Line(line));
                }
                KeyCode::Backspace => {
                    if cursor > 0 {
                        cursor -= 1;
                        buffer.remove(cursor);
                    }
                }
                KeyCode::Delete => {
                    if cursor < buffer.len() {
                        buffer.remove(cursor);
                    }
                }
                KeyCode::Left => {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        cursor = word_left(&buffer, cursor);
                    } else {
                        cursor = cursor.saturating_sub(1);
                    }
                }
                KeyCode::Right => {
                    if modifiers.contains(KeyModifiers::CONTROL) {
                        cursor = word_right(&buffer, cursor);
                    } else {
                        cursor = (cursor + 1).min(buffer.len());
                    }
                }
                KeyCode::Home => cursor = 0,
                KeyCode::End => cursor = buffer.len(),
                KeyCode::Up => {
                    if let Some(previous) =
                        history_up(&self.history, &mut history_index, &mut draft, &buffer)
                    {
                        buffer = previous;
                        cursor = buffer.len();
                    }
                }
                KeyCode::Down => {
                    if let Some(next) =
                        history_down(&self.history, &mut history_index, &mut draft, &buffer)
                    {
                        buffer = next;
                        cursor = buffer.len();
                    }
                }
                KeyCode::Tab => match complete(&buffer, cursor, completions) {
                    Completion::Unique { start, replacement } => {
                        buffer.splice(start..cursor, replacement.chars());
                        cursor = start + replacement.chars().count();
                    }
                    Completion::Multiple(matches) => {
                        execute!(
                            stdout,
                            Print("\r\n"),
                            Print(matches.join("  ")),
                            Print("\r\n")
                        )?;
                    }
                    Completion::None => {}
                },
                KeyCode::Char(character)
                    if !modifiers.contains(KeyModifiers::CONTROL)
                        && !modifiers.contains(KeyModifiers::ALT) =>
                {
                    buffer.insert(cursor, character);
                    cursor += 1;
                }
                _ => {}
            }

            render(&mut stdout, prompt, &buffer, cursor)?;
        }
    }
}

fn render(stdout: &mut io::Stdout, prompt: &str, buffer: &[char], cursor: usize) -> io::Result<()> {
    let text = buffer.iter().collect::<String>();
    execute!(
        stdout,
        cursor::MoveToColumn(0),
        terminal::Clear(ClearType::CurrentLine),
        Print(prompt),
        Print(&text)
    )?;

    let after_cursor = buffer[cursor..]
        .iter()
        .map(|character| UnicodeWidthChar::width(*character).unwrap_or(0))
        .sum::<usize>();
    if after_cursor > 0 {
        execute!(stdout, cursor::MoveLeft(after_cursor as u16))?;
    }
    stdout.flush()
}

fn word_left(buffer: &[char], cursor: usize) -> usize {
    let mut index = cursor;
    while index > 0 && buffer[index - 1].is_whitespace() {
        index -= 1;
    }
    while index > 0 && !buffer[index - 1].is_whitespace() {
        index -= 1;
    }
    index
}

fn word_right(buffer: &[char], cursor: usize) -> usize {
    let mut index = cursor;
    while index < buffer.len() && !buffer[index].is_whitespace() {
        index += 1;
    }
    while index < buffer.len() && buffer[index].is_whitespace() {
        index += 1;
    }
    index
}

fn history_up(
    history: &History,
    index: &mut usize,
    draft: &mut Option<Vec<char>>,
    current: &[char],
) -> Option<Vec<char>> {
    if *index == history.len() {
        *draft = Some(current.to_vec());
    }
    if *index == 0 {
        return None;
    }
    *index -= 1;
    Some(history.entries[*index].chars().collect())
}

fn history_down(
    history: &History,
    index: &mut usize,
    draft: &mut Option<Vec<char>>,
    _current: &[char],
) -> Option<Vec<char>> {
    if *index >= history.len() {
        return None;
    }
    *index += 1;
    if *index == history.len() {
        Some(draft.take().unwrap_or_default())
    } else {
        Some(history.entries[*index].chars().collect())
    }
}

enum Completion {
    None,
    Unique { start: usize, replacement: String },
    Multiple(Vec<String>),
}

fn complete(buffer: &[char], cursor: usize, candidates: &[String]) -> Completion {
    let mut start = cursor;
    while start > 0 && is_word_character(buffer[start - 1]) {
        start -= 1;
    }
    if start == cursor {
        return Completion::None;
    }

    let word = buffer[start..cursor].iter().collect::<String>();
    let command_context = start == 0 && word.starts_with(':');
    let mut matches = candidates
        .iter()
        .filter(|candidate| {
            if command_context {
                candidate.starts_with(':')
            } else {
                !candidate.starts_with(':')
            }
        })
        .filter(|candidate| candidate.starts_with(&word))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();

    match matches.len() {
        0 => Completion::None,
        1 => Completion::Unique {
            start,
            replacement: matches.pop().unwrap(),
        },
        _ => Completion::Multiple(matches),
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '?' | ':')
}

fn is_complete(input: &str) -> bool {
    let mut parens = 0i32;
    let mut brackets = 0i32;
    let mut comment = false;
    let mut previous = '\0';

    for character in input.chars() {
        if comment {
            if character == '\n' {
                comment = false;
            }
            previous = character;
            continue;
        }

        if character == '#' || (previous == '/' && character == '/') {
            comment = true;
            previous = character;
            continue;
        }

        match character {
            '(' => parens += 1,
            ')' => parens -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            _ => {}
        }
        previous = character;
    }

    parens <= 0 && brackets <= 0
}

struct RawMode;

impl RawMode {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

struct History {
    entries: Vec<String>,
    path: Option<PathBuf>,
}

impl History {
    fn load() -> Self {
        let path = history_path();
        if let Some(path) = &path {
            log::debug!("loading history from {}", path.display());
        }
        let entries = path
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| {
                text.lines()
                    .map(unescape_history)
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        log::debug!("loaded {} history entries", entries.len());
        Self { entries, path }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn remember(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() || self.entries.last().is_some_and(|last| last == line) {
            return;
        }
        self.entries.push(line.to_string());
        if self.entries.len() > 1000 {
            self.entries.remove(0);
        }
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = self
            .entries
            .iter()
            .map(|line| escape_history(line))
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(error) = std::fs::write(path, format!("{text}\n")) {
            log::warn!("failed to save history to {}: {error}", path.display());
        } else {
            log::debug!(
                "saved {} history entries to {}",
                self.entries.len(),
                path.display()
            );
        }
    }
}

fn history_path() -> Option<PathBuf> {
    if let Ok(state) = env::var("XDG_STATE_HOME")
        && !state.is_empty()
    {
        return Some(PathBuf::from(state).join(CARGO_PKG_NAME).join("history"));
    }
    env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(format!(".{CARGO_PKG_NAME}-history")))
}

pub fn log_path() -> Option<PathBuf> {
    history_path().and_then(|history| {
        history
            .parent()
            .map(|parent| parent.join(format!("{CARGO_PKG_NAME}.log")))
    })
}

fn escape_history(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    for character in line.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            _ => output.push(character),
        }
    }
    output
}

fn unescape_history(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => output.push('\n'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_complete_input() {
        assert!(is_complete("x + y"));
        assert!(!is_complete("(x + y"));
        assert!(!is_complete("[1, [2, 3]] + ["));
        assert!(is_complete("x + y # (unclosed"));
    }

    #[test]
    fn completes_unique_prefix() {
        let buffer = ":im".chars().collect::<Vec<_>>();
        match complete(
            &buffer,
            buffer.len(),
            &[":import".to_string(), ":help".to_string()],
        ) {
            Completion::Unique { replacement, .. } => assert_eq!(replacement, ":import"),
            _ => panic!("expected a unique completion"),
        }
    }
}
