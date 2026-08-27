use clap::{Args, ValueEnum};
use deltin_rs::diagnostics::{Diagnostic, Severity};
use deltin_rs::SourceMap;
use std::env;
use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};

#[derive(Clone, Debug, Args)]
pub struct OutputArgs {
    /// Emit one machine-readable JSON document on stdout.
    #[arg(long)]
    pub json: bool,

    /// Select the human-output presentation boundary.
    #[arg(long, value_enum, default_value_t = Presentation::Auto)]
    pub presentation: Presentation,

    /// Select ANSI color handling for human output.
    #[arg(long, value_enum, default_value_t = ColorPolicy::Auto)]
    pub color: ColorPolicy,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Presentation {
    #[default]
    Auto,
    Terminal,
    Plain,
    GithubActions,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ColorPolicy {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationMode {
    Terminal,
    Plain,
    GithubActions,
}

pub struct Renderer {
    mode: PresentationMode,
    color: bool,
}

impl Renderer {
    pub fn new(options: &OutputArgs) -> Renderer {
        let mode = match options.presentation {
            Presentation::Terminal => PresentationMode::Terminal,
            Presentation::Plain => PresentationMode::Plain,
            Presentation::GithubActions => PresentationMode::GithubActions,
            Presentation::Auto => {
                if github_actions_enabled() {
                    PresentationMode::GithubActions
                } else if io::stdout().is_terminal() && !ci_enabled() {
                    PresentationMode::Terminal
                } else {
                    PresentationMode::Plain
                }
            }
        };
        let color = match options.color {
            ColorPolicy::Always => true,
            ColorPolicy::Never => false,
            ColorPolicy::Auto => {
                env::var_os("NO_COLOR").is_none()
                    && mode == PresentationMode::Terminal
                    && io::stderr().is_terminal()
            }
        } && mode != PresentationMode::GithubActions;
        Renderer { mode, color }
    }

    pub fn emit_diagnostics(
        &self,
        diagnostics: &[Diagnostic],
        sources: &SourceMap,
    ) -> io::Result<()> {
        let mut stderr = io::stderr().lock();
        for diagnostic in diagnostics {
            let Some(file) = sources
                .files()
                .find(|file| file.id == diagnostic.primary.file)
            else {
                writeln!(
                    stderr,
                    "{}[{}]: {}",
                    severity_name(diagnostic.severity),
                    diagnostic.code,
                    diagnostic.message
                )?;
                continue;
            };
            let start = file.line_col(diagnostic.primary.start);
            let end = file.line_col(diagnostic.primary.end);
            match self.mode {
                PresentationMode::GithubActions => {
                    let level = match diagnostic.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                        Severity::Info => "notice",
                    };
                    let properties = format!(
                        "file={},line={},col={},endLine={},endColumn={},title={}",
                        escape_property(&file.name.display().to_string()),
                        start.line,
                        start.col,
                        end.line,
                        end.col.max(start.col + 1),
                        escape_property(&diagnostic.code),
                    );
                    writeln!(
                        stderr,
                        "::{level} {properties}::{}",
                        escape_message(&diagnostic.message)
                    )?;
                }
                PresentationMode::Terminal | PresentationMode::Plain => {
                    let label = format!(
                        "{}[{}]",
                        severity_name(diagnostic.severity),
                        diagnostic.code
                    );
                    let label = if self.color {
                        colorize(label.as_str(), diagnostic.severity)
                    } else {
                        label
                    };
                    writeln!(
                        stderr,
                        "{label}: {}:{}:{}: {}",
                        file.name.display(),
                        start.line,
                        start.col,
                        diagnostic.message
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn emit_message(&self, message: &str, severity: Severity) -> io::Result<()> {
        let mut stderr = io::stderr().lock();
        if self.mode == PresentationMode::GithubActions {
            let level = match severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "notice",
            };
            writeln!(
                stderr,
                "::{level} title=deltin-rs::{}",
                escape_message(message)
            )
        } else {
            writeln!(stderr, "{message}")
        }
    }

    pub fn emit_text(&self, text: &str) -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{text}")?;
        stdout.flush()
    }

    pub fn emit_summary(&self, summary: &str) -> io::Result<()> {
        self.emit_text(summary)?;
        if self.mode == PresentationMode::GithubActions {
            if let Some(path) = env::var_os("GITHUB_STEP_SUMMARY") {
                let mut file = OpenOptions::new().create(true).append(true).open(path)?;
                writeln!(file, "### deltin-rs\n\n{summary}")?;
            }
        }
        Ok(())
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

fn colorize(text: &str, severity: Severity) -> String {
    let code = match severity {
        Severity::Error => 31,
        Severity::Warning => 33,
        Severity::Info => 36,
    };
    format!("\x1b[{code}m{text}\x1b[0m")
}

fn github_actions_enabled() -> bool {
    env::var("GITHUB_ACTIONS")
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn ci_enabled() -> bool {
    env::var("CI")
        .map(|value| value.eq_ignore_ascii_case("true") || value == "1")
        .unwrap_or(false)
}

fn escape_property(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn escape_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

#[cfg(test)]
mod tests {
    use super::escape_property;

    #[test]
    fn escape_property_encodes_github_actions_delimiters() {
        assert_eq!(escape_property("cli,source:case"), "cli%2Csource%3Acase");
    }
}
