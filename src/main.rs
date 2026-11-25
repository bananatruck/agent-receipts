mod git;
mod logs;
mod report;

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use git::{Repository, Verdict};
use serde::Serialize;

#[derive(Parser)]
#[command(version, about = "Check report statements against local Git evidence")]
#[command(
    after_help = "Examples:\n  agent-receipts --repo . --report examples/synthetic-report.md\n  printf 'Tree is clean\\n' | agent-receipts --repo .\n  agent-receipts --log-file 'pytest --maxfail=1' 'build=debug/run.log' --report report.md\n\nExit codes: 0 all confirmed, 1 contradicted, 2 input error, 3 not checkable."
)]
struct Args {
    /// Working repository to inspect
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Captured UTF-8 log associated with COMMAND, repeatable
    #[arg(long = "log", value_name = "COMMAND=PATH")]
    logs: Vec<String>,
    /// Captured log as two arguments, allowing equals signs in PATH, repeatable
    #[arg(long = "log-file", num_args = 2, value_names = ["COMMAND", "PATH"])]
    log_files: Vec<String>,
    /// Existing local tracking ref, for example refs/remotes/origin/main
    #[arg(long)]
    remote_ref: Option<String>,
    /// Base commit for the committed diff ending at HEAD
    #[arg(long)]
    base: Option<String>,
    /// Emit schema version 1 as JSON
    #[arg(long)]
    json: bool,
    /// UTF-8 report file, or - for stdin
    #[arg(long, default_value = "-")]
    report: PathBuf,
}

#[derive(Serialize)]
struct Row<'a> {
    line: usize,
    statement: &'a str,
    check: &'static str,
    verdict: &'static str,
    evidence: String,
}

#[derive(Serialize)]
struct Summary {
    confirmed: usize,
    contradicted: usize,
    not_checkable: usize,
}

#[derive(Serialize)]
struct Document<'a> {
    schema_version: u8,
    statements: Vec<Row<'a>>,
    summary: Summary,
    exit_code: u8,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {}", cell(&error));
            ExitCode::from(2)
        }
    }
}

fn run(args: Args) -> Result<u8, String> {
    let report = if args.report.as_os_str() == "-" {
        if io::stdin().is_terminal() {
            return Err(
                "provide a report with --report PATH or pipe it on stdin; see --help".into(),
            );
        }
        let mut text = String::new();
        io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read UTF-8 report from stdin: {error}"))?;
        text
    } else {
        fs::read_to_string(&args.report)
            .map_err(|error| format!("cannot read report {}: {error}", args.report.display()))?
    };
    let repository = Repository::open(args.repo)?;
    let logs = logs::Logs::read(&args.logs, &args.log_files)?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut rows = Vec::new();
    let mut confirmed = 0;
    let mut contradicted = 0;
    let mut unknown = 0;
    let statements = report::parse(&report);
    for statement in &statements {
        let finding = match &statement.claim {
            report::Claim::Tests {
                command,
                passed,
                failed,
            } => logs.verify(command, *passed, *failed),
            report::Claim::Pushed { pushed, commit } => {
                repository.pushed(*pushed, commit.as_deref(), args.remote_ref.as_deref())
            }
            report::Claim::Diff(claim) => repository.verify_diff(claim, args.base.as_deref()),
            _ => repository.verify(&statement.claim),
        };
        match finding.verdict {
            Verdict::Confirmed => confirmed += 1,
            Verdict::Contradicted => contradicted += 1,
            Verdict::NotCheckable => unknown += 1,
        }
        rows.push(Row {
            line: statement.line,
            statement: &statement.source,
            check: statement.claim.label(),
            verdict: finding.verdict.label(),
            evidence: finding.evidence,
        });
    }
    let exit_code = if contradicted > 0 {
        1
    } else if unknown > 0 || confirmed == 0 {
        3
    } else {
        0
    };
    let document = Document {
        schema_version: 1,
        statements: rows,
        summary: Summary {
            confirmed,
            contradicted,
            not_checkable: unknown,
        },
        exit_code,
    };
    if args.json {
        serde_json::to_writer(&mut output, &document).map_err(|error| error.to_string())?;
        writeln!(output).map_err(|error| error.to_string())?;
    } else {
        writeln!(output, "LINE\tSTATEMENT\tCHECK\tVERDICT\tEVIDENCE")
            .map_err(|error| error.to_string())?;
        for row in &document.statements {
            writeln!(
                output,
                "{}\t{}\t{}\t{}\t{}",
                row.line,
                cell(row.statement),
                row.check,
                row.verdict,
                cell(&row.evidence)
            )
            .map_err(|error| error.to_string())?;
        }
        writeln!(
            output,
            "Summary: {confirmed} confirmed, {contradicted} contradicted, {unknown} not checkable"
        )
        .map_err(|error| error.to_string())?;
    }
    output.flush().map_err(|error| error.to_string())?;
    Ok(exit_code)
}

fn cell(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character == '\\' || character.is_control() || is_format_control(character) {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn is_format_control(c: char) -> bool {
    matches!(c, '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}' |
        '\u{890}'..='\u{891}' | '\u{8e2}' | '\u{180e}' | '\u{200b}'..='\u{200f}' |
        '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' |
        '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}' | '\u{110cd}' |
        '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}' |
        '\u{e0001}' | '\u{e0020}'..='\u{e007f}')
}
