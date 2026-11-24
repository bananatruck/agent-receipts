mod git;
mod logs;
mod report;

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use git::{Repository, Verdict};

#[derive(Parser)]
#[command(version, about = "Check report statements against local Git evidence")]
#[command(
    after_help = "Examples:\n  agent-receipts --repo . --report examples/synthetic-report.md\n  printf 'Tree is clean\\n' | agent-receipts --repo .\n\nExit codes: 0 all confirmed, 1 contradicted, 2 input error, 3 not checkable."
)]
struct Args {
    /// Working repository to inspect
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Captured UTF-8 log associated with COMMAND, repeatable
    #[arg(long = "log", value_name = "COMMAND=PATH")]
    logs: Vec<String>,
    /// UTF-8 report file, or - for stdin
    #[arg(long, default_value = "-")]
    report: PathBuf,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
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
    let logs = logs::Logs::read(&args.logs)?;
    let mut output = io::BufWriter::new(io::stdout().lock());
    writeln!(output, "LINE\tSTATEMENT\tCHECK\tVERDICT\tEVIDENCE")
        .map_err(|error| error.to_string())?;
    let mut confirmed = 0;
    let mut contradicted = 0;
    let mut unknown = 0;
    for statement in report::parse(&report) {
        let finding = match &statement.claim {
            report::Claim::Tests {
                command,
                passed,
                failed,
            } => logs.verify(command, *passed, *failed),
            _ => repository.verify(&statement.claim),
        };
        match finding.verdict {
            Verdict::Confirmed => confirmed += 1,
            Verdict::Contradicted => contradicted += 1,
            Verdict::NotCheckable => unknown += 1,
        }
        writeln!(
            output,
            "{}\t{}\t{}\t{}\t{}",
            statement.line,
            cell(&statement.source),
            statement.claim.label(),
            finding.verdict.label(),
            cell(&finding.evidence)
        )
        .map_err(|error| error.to_string())?;
    }
    writeln!(
        output,
        "Summary: {confirmed} confirmed, {contradicted} contradicted, {unknown} not checkable"
    )
    .map_err(|error| error.to_string())?;
    output.flush().map_err(|error| error.to_string())?;
    Ok(if contradicted > 0 {
        1
    } else if unknown > 0 || confirmed == 0 {
        3
    } else {
        0
    })
}

fn cell(text: &str) -> String {
    text.chars()
        .flat_map(|character| {
            if character == '\\' || character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}
