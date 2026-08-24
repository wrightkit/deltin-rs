#[path = "cli/mod.rs"]
mod cli;

use clap::{error::ErrorKind, Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use del_rs::diagnostics::Severity;
use del_rs::matrix;
use del_rs::syntax::parse_source;
use del_rs::{Diagnostic, SourceMap};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "del-rs",
    version,
    about = "Workshop-independent OSTW/DeltinScript implementation"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a DEL/OSTW file or project through the full parsing and semantic pipeline.
    Check(PathArgs),
    /// Query semantic identity, type, and resolution at a source position.
    Inspect(InspectArgs),
    /// Show or validate the declared DEL/OSTW support surface.
    Support(SupportArgs),
    /// Generate a static shell completion script from this command model.
    Completion(CompletionArgs),
    /// Developer/debug inspection commands; these are not stable language UX.
    Dev {
        #[command(subcommand)]
        command: DeveloperCommand,
    },
    /// Maintainer and CI evidence commands.
    Maintainer {
        #[command(subcommand)]
        command: MaintainerCommand,
    },
    #[command(hide = true)]
    Parse(PathArgs),
    #[command(hide = true)]
    Hir(PathArgs),
    #[command(name = "matrix", hide = true)]
    LegacyMatrix(SupportArgs),
    #[command(name = "compatibility", hide = true)]
    LegacyCompatibility {
        #[command(flatten)]
        output: cli::OutputArgs,
    },
}

#[derive(Debug, Subcommand)]
enum DeveloperCommand {
    /// Lex and parse one source file and report an AST/token summary.
    Parse(PathArgs),
    /// Lower a source file or project to typed HIR and validate it.
    Hir(PathArgs),
}

#[derive(Debug, Subcommand)]
enum MaintainerCommand {
    /// Run the evidence-driven DEL/OSTW corpus report.
    Compatibility {
        #[command(flatten)]
        output: cli::OutputArgs,
    },
}

#[derive(Debug, Args)]
struct PathArgs {
    #[arg(value_name = "FILE_OR_DIR")]
    path: PathBuf,
    #[command(flatten)]
    output: cli::OutputArgs,
}

#[derive(Debug, Args)]
struct InspectArgs {
    #[arg(value_name = "FILE")]
    file: PathBuf,
    #[arg(value_name = "LINE:COL", allow_hyphen_values = true)]
    position: String,
    #[command(flatten)]
    output: cli::OutputArgs,
}

#[derive(Debug, Args)]
struct SupportArgs {
    /// Validate the embedded support matrix instead of listing its counts.
    #[arg(long)]
    check: bool,
    #[command(flatten)]
    output: cli::OutputArgs,
}

#[derive(Debug, Args)]
struct CompletionArgs {
    #[arg(value_enum, value_name = "SHELL")]
    shell: Shell,
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    Input(String),
    Internal(String),
}

type CliResult = Result<u8, CliError>;

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code() as u8;
            let text = error.to_string();
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                print!("{text}");
            } else {
                eprint!("{text}");
            }
            return ExitCode::from(code);
        }
    };

    if cli.command.is_none() {
        if let Err(error) = print_help() {
            eprintln!("del-rs: internal error: {error}");
            return ExitCode::from(3);
        }
        return ExitCode::SUCCESS;
    }

    match std::panic::catch_unwind(|| execute(cli)) {
        Ok(Ok(code)) => ExitCode::from(code),
        Ok(Err(CliError::Usage(message))) => {
            eprintln!("del-rs: {message}");
            ExitCode::from(2)
        }
        Ok(Err(CliError::Input(message))) => {
            eprintln!("del-rs: {message}");
            ExitCode::from(4)
        }
        Ok(Err(CliError::Internal(message))) => {
            eprintln!("del-rs: internal error: {message}");
            ExitCode::from(3)
        }
        Err(_) => {
            eprintln!("del-rs: internal error: unexpected panic");
            ExitCode::from(3)
        }
    }
}

fn print_help() -> io::Result<()> {
    let mut command = Cli::command();
    let mut stdout = io::stdout().lock();
    command.write_help(&mut stdout)?;
    stdout.flush()
}

fn execute(cli: Cli) -> CliResult {
    let command = cli.command.expect("command checked in main");
    match command {
        Command::Check(args) => cmd_check(args),
        Command::Inspect(args) => cmd_inspect(args),
        Command::Support(args) => cmd_support(args, false),
        Command::Completion(args) => cmd_completion(args),
        Command::Dev { command } => match command {
            DeveloperCommand::Parse(args) => cmd_parse(args),
            DeveloperCommand::Hir(args) => cmd_hir(args),
        },
        Command::Maintainer { command } => match command {
            MaintainerCommand::Compatibility { output } => cmd_compatibility(output),
        },
        Command::Parse(args) => cmd_parse(args),
        Command::Hir(args) => cmd_hir(args),
        Command::LegacyMatrix(args) => cmd_support(args, true),
        Command::LegacyCompatibility { output } => cmd_compatibility(output),
    }
}

fn preflight_file(path: &std::path::Path) -> Result<String, CliError> {
    let metadata = fs::metadata(path).map_err(|error| input_error(path, error))?;
    if !metadata.is_file() {
        return Err(CliError::Input(format!(
            "input is not a readable file: {}",
            path.display()
        )));
    }
    fs::read_to_string(path).map_err(|error| input_error(path, error))
}

fn preflight_project_input(path: &std::path::Path) -> Result<(), CliError> {
    let metadata = fs::metadata(path).map_err(|error| input_error(path, error))?;
    if metadata.is_dir() {
        let mut entries = fs::read_dir(path).map_err(|error| input_error(path, error))?;
        entries
            .next()
            .transpose()
            .map_err(|error| input_error(path, error))?;
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(CliError::Input(format!(
            "input is not a readable file or directory: {}",
            path.display()
        )));
    }
    fs::read_to_string(path)
        .map(|_| ())
        .map_err(|error| input_error(path, error))
}

fn input_error(path: &std::path::Path, error: impl std::fmt::Display) -> CliError {
    CliError::Input(format!("cannot read {}: {error}", path.display()))
}

fn parse_position(position: &str) -> Result<(u32, u32), CliError> {
    let (line, col) = position
        .split_once(':')
        .ok_or_else(|| CliError::Usage("position must be <line>:<col>".into()))?;
    let line: u32 = line
        .parse()
        .map_err(|_| CliError::Usage("line must be a positive integer".into()))?;
    let col: u32 = col
        .parse()
        .map_err(|_| CliError::Usage("column must be a positive integer".into()))?;
    if line == 0 || col == 0 {
        return Err(CliError::Usage(
            "line and column must be positive integers".into(),
        ));
    }
    Ok((line, col))
}

fn position_offset(
    sources: &SourceMap,
    file: del_rs::FileId,
    line: u32,
    col: u32,
) -> Result<u32, CliError> {
    let source = sources.get(file);
    let wanted_line = line as usize - 1;
    let mut line_start = 0usize;
    for (index, line_text) in source.text.split('\n').enumerate() {
        if index == wanted_line {
            let char_count = line_text.chars().count();
            let col_index = col as usize - 1;
            if col_index > char_count {
                return Err(CliError::Usage(format!(
                    "column {col} is outside line {line} (maximum {})",
                    char_count + 1
                )));
            }
            let byte_in_line = if col_index == char_count {
                line_text.len()
            } else {
                line_text
                    .char_indices()
                    .nth(col_index)
                    .map(|(offset, _)| offset)
                    .expect("column index was checked against the character count")
            };
            let offset = u32::try_from(line_start + byte_in_line)
                .map_err(|_| CliError::Usage("position is beyond the file limit".into()))?;
            let mapped = source.line_col(offset);
            if mapped.line != line || mapped.col != col {
                return Err(CliError::Usage(format!(
                    "position {line}:{col} is outside the file"
                )));
            }
            return Ok(offset);
        }
        line_start += line_text.len() + 1;
    }
    Err(CliError::Usage(format!(
        "line {line} is outside the file (maximum {})",
        source.text.split('\n').count()
    )))
}

fn check_path_for_cli(path: &std::path::Path, json: bool) -> del_rs::api::CheckReport {
    let run = || del_rs::api::check_path(path, &del_rs::semantic::provider::NoopProvider::new());
    if json {
        without_debug_output(run)
    } else {
        run()
    }
}

fn without_debug_output<T>(run: impl FnOnce() -> T) -> T {
    let previous = env::var_os("DEL_DEBUG");
    if previous.is_some() {
        env::remove_var("DEL_DEBUG");
    }
    let result = run();
    if let Some(value) = previous {
        env::set_var("DEL_DEBUG", value);
    }
    result
}

fn cmd_parse(args: PathArgs) -> CliResult {
    let text = preflight_file(&args.path)?;
    let mut sources = SourceMap::new();
    let id = sources.add_file(args.path.clone(), text);
    let output = parse_source(id, sources.text(id));
    let errors = error_count(&output.diagnostics);
    let json = serde_json::json!({
        "command": "parse",
        "phase": "parse",
        "file": args.path.display().to_string(),
        "diagnostics": output.diagnostics,
        "summary": {
            "items": output.ast.items.len(),
            "tokens": output.tokens.len(),
            "errors": errors,
        },
    });
    render_report(
        args.output,
        json,
        &output.diagnostics,
        &sources,
        format!(
            "parsed {}: {} items, {} tokens, {} diagnostics ({} errors)",
            args.path.display(),
            output.ast.items.len(),
            output.tokens.len(),
            output.diagnostics.len(),
            errors
        ),
        if errors > 0 { 1 } else { 0 },
    )
}

fn cmd_check(args: PathArgs) -> CliResult {
    preflight_project_input(&args.path)?;
    let report = check_path_for_cli(&args.path, args.output.json);
    let errors = error_count(&report.diagnostics);
    let json = serde_json::json!({
        "command": "check",
        "phase": "all",
        "diagnostics": report.diagnostics,
        "summary": {
            "files": report.project.files.len(),
            "funcs": report.hir.funcs.len(),
            "rules": report.hir.rules.len(),
            "classes": report.hir.classes.len(),
            "errors": errors,
        },
    });
    render_report(
        args.output,
        json,
        &report.diagnostics,
        &report.project.sources,
        format!(
            "checked: {} files, {} funcs, {} rules, {} classes, {} diagnostics ({} errors)",
            report.project.files.len(),
            report.hir.funcs.len(),
            report.hir.rules.len(),
            report.hir.classes.len(),
            report.diagnostics.len(),
            errors
        ),
        if errors == 0 { 0 } else { 1 },
    )
}

fn cmd_hir(args: PathArgs) -> CliResult {
    preflight_project_input(&args.path)?;
    let report = check_path_for_cli(&args.path, args.output.json);
    let errors = error_count(&report.diagnostics);
    let json = serde_json::json!({
        "command": "hir",
        "phase": "hir",
        "diagnostics": report.diagnostics,
        "summary": {
            "funcs": report.hir.funcs.len(),
            "rules": report.hir.rules.len(),
            "classes": report.hir.classes.len(),
            "enums": report.hir.enums.len(),
            "vars": report.hir.vars.len(),
            "exprs": report.hir.exprs.len(),
            "errors": errors,
        },
    });
    render_report(
        args.output,
        json,
        &report.diagnostics,
        &report.project.sources,
        format!(
            "hir: {} funcs, {} rules, {} classes, {} enums, {} vars, {} exprs, {} diagnostics ({} errors)",
            report.hir.funcs.len(),
            report.hir.rules.len(),
            report.hir.classes.len(),
            report.hir.enums.len(),
            report.hir.vars.len(),
            report.hir.exprs.len(),
            report.diagnostics.len(),
            errors
        ),
        if errors == 0 { 0 } else { 1 },
    )
}

fn cmd_inspect(args: InspectArgs) -> CliResult {
    let (line, col) = parse_position(&args.position)?;
    let text = preflight_file(&args.file)?;
    let mut input_sources = SourceMap::new();
    let input_file = input_sources.add_file(args.file.clone(), text);
    let offset = position_offset(&input_sources, input_file, line, col)?;
    let report = check_path_for_cli(&args.file, args.output.json);
    let Some(file) = report.project.sources.files().find(|file| {
        file.name
            .ends_with(args.file.file_name().unwrap_or_default())
    }) else {
        eprintln!("inspect: file not part of the project");
        return Ok(4);
    };
    let fid = file.id;
    let symbol = del_rs::api::symbol_at(&report.semantic, fid, offset);
    let ty = del_rs::api::type_at(&report.semantic, fid, offset);
    let resolution = del_rs::api::resolution_at(&report.semantic, fid, offset);
    let errors = error_count(&report.diagnostics);
    let json = serde_json::json!({
        "command": "inspect",
        "phase": "query",
        "diagnostics": report.diagnostics,
        "symbol": symbol.map(|symbol| {
            let item = report.semantic.tables.symbol(symbol);
            serde_json::json!({ "name": item.name, "id": symbol })
        }),
        "type": ty.as_ref().map(|ty| ty.describe()),
        "resolution": resolution.map(|resolution| format!("{resolution:?}")),
        "summary": {
            "diagnostics": report.diagnostics.len(),
            "errors": errors,
        },
    });
    if args.output.json {
        return emit_json(json);
    }
    let renderer = cli::Renderer::new(&args.output);
    renderer
        .emit_diagnostics(&report.diagnostics, &report.project.sources)
        .map_err(internal_error)?;
    match symbol {
        Some(symbol) => renderer
            .emit_text(&format!(
                "symbol: {} (id {symbol})",
                report.semantic.tables.symbol(symbol).name
            ))
            .map_err(internal_error)?,
        None => renderer.emit_text("symbol: none").map_err(internal_error)?,
    }
    match ty {
        Some(ty) => renderer
            .emit_text(&format!("type: {}", ty.describe()))
            .map_err(internal_error)?,
        None => renderer.emit_text("type: none").map_err(internal_error)?,
    }
    renderer
        .emit_summary(&format!(
            "inspect: {} diagnostics ({} errors); query exit remains successful",
            report.diagnostics.len(),
            errors
        ))
        .map_err(internal_error)?;
    Ok(0)
}

fn cmd_support(args: SupportArgs, legacy: bool) -> CliResult {
    let command = if legacy { "matrix" } else { "support" };
    match matrix::load_and_validate() {
        Ok(matrix) => {
            let counts = matrix::state_counts(&matrix);
            let states: serde_json::Value = counts
                .iter()
                .map(|(state, count)| (format!("{state:?}"), *count))
                .collect();
            let json = serde_json::json!({
                "command": command,
                "phase": "matrix",
                "valid": true,
                "entries": matrix.entries.len(),
                "states": states,
            });
            if args.output.json {
                return emit_json(json).map(|_| support_exit_code(true));
            }
            let renderer = cli::Renderer::new(&args.output);
            if args.check {
                renderer
                    .emit_summary(&format!(
                        "support matrix valid: {} entries",
                        matrix.entries.len()
                    ))
                    .map_err(internal_error)?;
            } else {
                renderer
                    .emit_summary(&format!(
                        "del-rs support matrix (upstream pin: {})",
                        matrix.meta.upstream_pin
                    ))
                    .map_err(internal_error)?;
                for (state, count) in &counts {
                    renderer
                        .emit_text(&format!("  {state:?}: {count}"))
                        .map_err(internal_error)?;
                }
            }
            Ok(support_exit_code(true))
        }
        Err(problems) => {
            let json = serde_json::json!({
                "command": command,
                "phase": "matrix",
                "valid": false,
                "problems": problems,
            });
            if args.output.json {
                emit_json(json).map(|_| support_exit_code(false))
            } else {
                let renderer = cli::Renderer::new(&args.output);
                for problem in problems {
                    renderer
                        .emit_message(&format!("matrix problem: {problem}"), Severity::Error)
                        .map_err(internal_error)?;
                }
                Ok(support_exit_code(false))
            }
        }
    }
}

fn cmd_compatibility(output: cli::OutputArgs) -> CliResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    match del_rs::compatibility::run(root) {
        Ok(report) => {
            if output.json {
                return emit_json(serde_json::to_value(&report).map_err(internal_error)?)
                    .map(|_| compatibility_exit_code(report.summary.unexpected_regressions));
            }
            let renderer = cli::Renderer::new(&output);
            let summary = &report.summary;
            renderer
                .emit_summary(&format!(
                    "compatibility: {} fixtures | matched {} | known gaps {} | unsupported {} | unexpected regressions {} | inconclusive {}",
                    summary.total,
                    summary.matched,
                    summary.known_gaps,
                    summary.unsupported,
                    summary.unexpected_regressions,
                    summary.inconclusive
                ))
                .map_err(internal_error)?;
            for case in &report.cases {
                if case.status != del_rs::compatibility::FixtureStatus::Matched {
                    renderer
                        .emit_text(&format!("  {:?}: {}", case.status, case.fixture.path))
                        .map_err(internal_error)?;
                }
            }
            Ok(compatibility_exit_code(summary.unexpected_regressions))
        }
        Err(problems) => {
            let json = serde_json::json!({
                "command": "compatibility",
                "valid": false,
                "problems": problems,
            });
            if output.json {
                emit_json(json).map(|_| 1)
            } else {
                let renderer = cli::Renderer::new(&output);
                for problem in problems {
                    renderer
                        .emit_message(
                            &format!("compatibility problem: {problem}"),
                            Severity::Error,
                        )
                        .map_err(internal_error)?;
                }
                Ok(1)
            }
        }
    }
}

fn cmd_completion(args: CompletionArgs) -> CliResult {
    let mut command = Cli::command();
    let mut stdout = io::stdout().lock();
    clap_complete::generate(args.shell, &mut command, "del-rs", &mut stdout);
    stdout.flush().map_err(internal_error)?;
    Ok(0)
}

fn render_report(
    output: cli::OutputArgs,
    json: serde_json::Value,
    diagnostics: &[Diagnostic],
    sources: &SourceMap,
    summary: String,
    code: u8,
) -> CliResult {
    if output.json {
        return emit_json(json).map(|_| code);
    }
    let renderer = cli::Renderer::new(&output);
    renderer
        .emit_diagnostics(diagnostics, sources)
        .map_err(internal_error)?;
    renderer.emit_summary(&summary).map_err(internal_error)?;
    Ok(code)
}

fn emit_json(value: serde_json::Value) -> CliResult {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &value).map_err(internal_error)?;
    writeln!(stdout).map_err(internal_error)?;
    stdout.flush().map_err(internal_error)?;
    Ok(0)
}

fn error_count(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .count()
}

fn support_exit_code(valid: bool) -> u8 {
    if valid {
        0
    } else {
        1
    }
}

fn compatibility_exit_code(unexpected_regressions: usize) -> u8 {
    if unexpected_regressions == 0 {
        0
    } else {
        1
    }
}

fn internal_error(error: impl std::fmt::Display) -> CliError {
    CliError::Internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{compatibility_exit_code, support_exit_code};

    #[test]
    fn json_exit_codes_follow_support_and_compatibility_results() {
        assert_eq!(support_exit_code(true), 0);
        assert_eq!(support_exit_code(false), 1);
        assert_eq!(compatibility_exit_code(0), 0);
        assert_eq!(compatibility_exit_code(1), 1);
    }
}
