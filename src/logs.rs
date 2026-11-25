use regex::Regex;
use std::collections::BTreeMap;
use std::fs;
use std::sync::LazyLock;

use crate::git::{Finding, Verdict};

#[derive(Debug, PartialEq)]
pub struct Counts {
    pub passed: u64,
    pub failed: u64,
}

pub struct Logs(BTreeMap<String, String>);

impl Logs {
    pub fn read(values: &[String]) -> Result<Self, String> {
        let mut logs = BTreeMap::new();
        for value in values {
            let (command, path) = value
                .rsplit_once('=')
                .filter(|(c, p)| !c.is_empty() && !p.is_empty())
                .ok_or("--log requires COMMAND=PATH, for example --log 'cargo test=run.txt'")?;
            let contents =
                fs::read_to_string(path).map_err(|e| format!("cannot read log {path}: {e}"))?;
            if logs
                .insert(command.to_owned(), contents.replace("\r\n", "\n"))
                .is_some()
            {
                return Err(format!(
                    "duplicate log for {command}; supply one captured run per command"
                ));
            }
        }
        Ok(Self(logs))
    }

    pub fn verify(&self, command: &str, passed: u64, failed: Option<u64>) -> Finding {
        let result = self
            .0
            .get(command)
            .ok_or("no supplied log for this command")
            .and_then(|text| {
                let records: Vec<_> = text
                    .split_inclusive('\n')
                    .scan(0, |offset, line| {
                        let start = *offset;
                        *offset += line.len();
                        Some((start, line))
                    })
                    .filter_map(|(offset, line)| {
                        line.trim_end_matches('\n')
                            .strip_prefix("$ ")
                            .map(|record| (offset + line.len(), record))
                    })
                    .collect();
                if records.len() != 1 || records[0].1 != command {
                    return Err("log must contain exactly one matching $ COMMAND record");
                }
                let body = &text[records[0].0..];
                let parser: fn(&str) -> Option<Counts> =
                    match command.split_whitespace().collect::<Vec<_>>().as_slice() {
                        ["cargo", "test", ..] => cargo_summary,
                        ["pytest", ..] => pytest_summary,
                        ["vitest" | "jest", ..] | ["npx", "vitest" | "jest", ..] => {
                            javascript_summary
                        }
                        _ => return Err("unsupported command family"),
                    };
                let counts = parser(body);
                if runner_failure(command, body, failed.unwrap_or(0) > 0) {
                    return Ok(None);
                }
                counts
                    .map(Some)
                    .ok_or("no unambiguous supported completed test summary")
            });
        match result {
            Ok(None) => Finding {
                verdict: Verdict::Contradicted,
                evidence: format!(
                    "{command}: log records a runner failure; evidence covers only what the supplied log contains, not execution or authenticity"
                ),
            },
            Ok(Some(counts)) => Finding {
                verdict: if counts.passed == passed && failed.unwrap_or(0) == counts.failed {
                    Verdict::Confirmed
                } else {
                    Verdict::Contradicted
                },
                evidence: format!(
                    "{command}: {} passed, {} failed; evidence covers only what the supplied log contains, not execution or authenticity",
                    counts.passed, counts.failed
                ),
            },
            Err(reason) => Finding {
                verdict: Verdict::NotCheckable,
                evidence: reason.into(),
            },
        }
    }
}

#[derive(Debug, PartialEq)]
enum CargoLine {
    SuiteStart,
    Ambiguous,
    Output,
}

fn cargo_line(line: &str) -> CargoLine {
    // Cargo writes these padded status labels to stderr. Preserve their spaces
    // in merged logs, where test output can also contain Running or Doc-tests.
    if let Some(name) = line.strip_prefix("   Doc-tests ") {
        return if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            CargoLine::SuiteStart
        } else {
            CargoLine::Ambiguous
        };
    }
    if line.trim_start().starts_with("Doc-tests ") {
        return CargoLine::Ambiguous;
    }
    let (payload, padded) = if let Some(payload) = line.strip_prefix("     Running ") {
        (payload, true)
    } else if let Some(payload) = line.trim_start().strip_prefix("Running ") {
        (payload, false)
    } else {
        return CargoLine::Output;
    };
    let source = payload.strip_prefix("unittests ").unwrap_or(payload);
    if let Some((source, executable)) = source.strip_suffix(')').and_then(|s| s.split_once(" (")) {
        // Target directories can be absolute, renamed, or contain spaces.
        // A bare parenthesized word, such as (cached), is not a target path.
        if !source.is_empty()
            && source.trim() == source
            && executable.trim() == executable
            && executable.contains(['/', '\\'])
        {
            // A path-shaped line with altered padding is ambiguous evidence,
            // not an announcement or safely ignorable ordinary output.
            return if padded {
                CargoLine::SuiteStart
            } else {
                CargoLine::Ambiguous
            };
        }
    }
    if payload.is_empty()
        || payload == "unittests"
        || payload.starts_with("unittests ")
        || payload.contains(['/', '\\'])
        || payload
            .split(" (")
            .next()
            .is_some_and(|s| s.ends_with(".rs"))
    {
        CargoLine::Ambiguous
    } else {
        CargoLine::Output
    }
}

enum CargoSuite {
    Idle,
    Announced,
    Running(u64),
}

pub fn cargo_summary(text: &str) -> Option<Counts> {
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; [0-9]+ filtered out; finished in [0-9.]+s$").unwrap()
    });
    static RUNNING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^running ([0-9]+) tests?$").unwrap());
    let mut total = Counts {
        passed: 0,
        failed: 0,
    };
    let mut found = false;
    let mut suite = CargoSuite::Idle;
    for raw in text.lines() {
        match cargo_line(raw) {
            CargoLine::SuiteStart => {
                if !matches!(suite, CargoSuite::Idle) {
                    return None;
                }
                suite = CargoSuite::Announced;
            }
            CargoLine::Ambiguous => return None,
            CargoLine::Output => {}
        }
        let line = raw.trim();
        if let Some(c) = RUNNING.captures(line) {
            if matches!(suite, CargoSuite::Running(_)) {
                return None;
            }
            suite = CargoSuite::Running(c[1].parse::<u64>().ok()?);
        }
        if !line.starts_with("test result:") {
            continue;
        }
        let CargoSuite::Running(expected) = suite else {
            return None;
        };
        let c = SUMMARY.captures(line)?;
        let passed: u64 = c[2].parse().ok()?;
        let failed: u64 = c[3].parse().ok()?;
        let ignored: u64 = c[4].parse().ok()?;
        let measured: u64 = c[5].parse().ok()?;
        if passed
            .checked_add(failed)?
            .checked_add(ignored)?
            .checked_add(measured)?
            != expected
            || (c[1].eq("ok") && failed != 0)
        {
            return None;
        }
        total.passed = total.passed.checked_add(passed)?;
        total.failed = total.failed.checked_add(failed)?;
        found = true;
        suite = CargoSuite::Idle;
    }
    (found && matches!(suite, CargoSuite::Idle)).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_announcements_require_complete_padded_status_lines() {
        for (line, expected) in [
            (
                "     Running unittests src/lib.rs (target/debug/build/fixture/5a77203016da995e/out/fixture-5a77203016da995e)",
                CargoLine::SuiteStart,
            ),
            (
                "     Running unittests src/main.rs (target/debug/deps/fixture-1234)",
                CargoLine::SuiteStart,
            ),
            (
                "     Running tests/cli.rs (target/debug/build/fixture/075ffe448785b417/out/cli-075ffe448785b417)",
                CargoLine::SuiteStart,
            ),
            (
                "     Running checks/custom (/tmp/custom target/debug/deps/custom-1234)",
                CargoLine::SuiteStart,
            ),
            (
                r"     Running tests\cli.rs (C:\custom target\debug\deps\cli-1234.exe)",
                CargoLine::SuiteStart,
            ),
            ("   Doc-tests agent_receipts", CargoLine::SuiteStart),
            ("Running query (cached)", CargoLine::Output),
            ("     Running query (cached)", CargoLine::Output),
            ("Running initialization (phase one)", CargoLine::Output),
            (
                "Running unittests src/lib.rs (target/debug/deps/fixture-1234)",
                CargoLine::Ambiguous,
            ),
            (
                "    Running tests/cli.rs (target/debug/deps/cli-1234)",
                CargoLine::Ambiguous,
            ),
            (
                "      Running tests/cli.rs (target/debug/deps/cli-1234)",
                CargoLine::Ambiguous,
            ),
            (
                "\tRunning tests/cli.rs (target/debug/deps/cli-1234)",
                CargoLine::Ambiguous,
            ),
            ("Doc-tests agent_receipts", CargoLine::Ambiguous),
            ("    Doc-tests agent_receipts", CargoLine::Ambiguous),
            ("   Doc-tests agent_receipts extra", CargoLine::Ambiguous),
            ("   Doc-tests agent_receipts ", CargoLine::Ambiguous),
            ("   Doc-tests ", CargoLine::Ambiguous),
            ("     Running unittests src/lib.rs", CargoLine::Ambiguous),
            (
                "     Running tests/cli.rs (target/debug/deps/cli-1234",
                CargoLine::Ambiguous,
            ),
            (
                "     Running tests/cli.rs (target/debug/deps/cli-1234) trailing",
                CargoLine::Ambiguous,
            ),
            (
                "     Running tests/cli.rs (target/debug/deps/cli-1234) ",
                CargoLine::Ambiguous,
            ),
            ("     Running tests/cli.rs (", CargoLine::Ambiguous),
            ("     Running ", CargoLine::Ambiguous),
        ] {
            assert_eq!(cargo_line(line), expected, "{line:?}");
        }
    }

    #[test]
    fn cargo_summaries_add_completed_suites_and_preserve_failures() {
        assert_eq!(
            cargo_summary(
                "running 3 tests\ntest result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\nrunning 2 tests\ntest result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
            ),
            Some(Counts {
                passed: 3,
                failed: 1
            })
        );
        assert_eq!(cargo_summary("running 77 tests\n"), None);
        assert_eq!(cargo_summary("test result: ok. 77 passed;"), None);
    }
}

#[cfg(test)]
mod pytest_tests {
    use super::*;
    #[test]
    fn pytest_summary_counts_passes_failures_and_errors() {
        assert_eq!(
            pytest_summary("================ 7 passed, 2 skipped in 0.10s ================\n"),
            Some(Counts {
                passed: 7,
                failed: 0
            })
        );
        assert_eq!(
            pytest_summary("2 failed, 3 passed, 1 error in 1.20s\n"),
            Some(Counts {
                passed: 3,
                failed: 3
            })
        );
        assert_eq!(pytest_summary("collecting ... 7 items\n"), None);
        assert_eq!(
            pytest_summary("7 passed in 0.10s\n7 passed in 0.10s\n"),
            None
        );
        assert_eq!(pytest_summary("7 passed, 1 mystery in 0.10s\n"), None);
    }
}
fn pytest_summary(text: &str) -> Option<Counts> {
    static START: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^[0-9]+ (?:passed|failed|errors?|skipped|deselected|xfailed|xpassed|warnings?)\b",
        )
        .unwrap()
    });
    static SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^([0-9]+ (?:passed|failed|errors?|skipped|deselected|xfailed|xpassed|warnings?)\b.*) in [0-9.]+s(?: \(.*\))?$").unwrap()
    });
    let summaries: Vec<_> = text
        .lines()
        .map(|line| line.trim().trim_matches('=').trim())
        .filter(|line| START.is_match(line))
        .collect();
    let [line] = summaries.as_slice() else {
        return None;
    };
    let summary = SUMMARY.captures(line)?;
    count_items(
        &summary[1],
        &[
            "passed",
            "failed",
            "error",
            "errors",
            "skipped",
            "deselected",
            "xfailed",
            "xpassed",
            "warning",
            "warnings",
        ],
        ", ",
    )
}

fn count_items(text: &str, allowed: &[&str], separator: &str) -> Option<Counts> {
    let mut seen = std::collections::BTreeSet::new();
    let mut counts = Counts {
        passed: 0,
        failed: 0,
    };
    for item in text.split(separator) {
        let (n, kind) = item.trim().split_once(' ')?;
        let n: u64 = n.parse().ok()?;
        if !allowed.contains(&kind) || !seen.insert(kind) {
            return None;
        }
        match kind {
            "passed" => counts.passed = n,
            "failed" | "error" | "errors" => counts.failed = counts.failed.checked_add(n)?,
            _ => {}
        }
    }
    Some(counts)
}

#[cfg(test)]
mod javascript_tests {
    use super::*;
    #[test]
    fn javascript_summaries_ignore_suite_counts_and_require_test_totals() {
        assert_eq!(
            javascript_summary(
                " Test Files  1 passed (1)\n      Tests  7 passed | 2 skipped (9)\n"
            ),
            Some(Counts {
                passed: 7,
                failed: 0
            })
        );
        assert_eq!(
            javascript_summary(
                "Test Suites: 1 failed, 1 total\nTests:       2 failed, 3 passed, 5 total\n"
            ),
            Some(Counts {
                passed: 3,
                failed: 2
            })
        );
        assert_eq!(javascript_summary("Tests: 3 passed, 5 total\n"), None);
        assert_eq!(
            javascript_summary("Tests 7 passed (7)\nTests 7 passed (7)\n"),
            None
        );
        assert_eq!(javascript_summary("Test Suites: 7 passed, 7 total\n"), None);
    }
}
fn javascript_summary(text: &str) -> Option<Counts> {
    static START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Tests(?::|\s+[0-9])").unwrap());
    static VITEST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^Tests\s+(.+) \(([0-9]+)\)$").unwrap());
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| START.is_match(line))
        .collect();
    let [line] = lines.as_slice() else {
        return None;
    };
    let (items, total, separator) = if let Some(body) = line.strip_prefix("Tests:") {
        let (items, total) = body.trim().rsplit_once(", ")?;
        (
            items.to_owned(),
            total.strip_suffix(" total")?.parse::<u64>().ok()?,
            ", ",
        )
    } else {
        let c = VITEST.captures(line)?;
        (c[1].to_owned(), c[2].parse::<u64>().ok()?, " | ")
    };
    let counts = count_items(&items, &["passed", "failed", "skipped", "todo"], separator)?;
    let sum = items.split(separator).try_fold(0u64, |sum, item| {
        sum.checked_add(item.trim().split_once(' ')?.0.parse().ok()?)
    })?;
    (sum == total).then_some(counts)
}

fn runner_failure(command: &str, text: &str, reported_failures: bool) -> bool {
    // Abnormal exits have an unprefixed cause even when other suites counted
    // assertion failures. Normal assertion exits use 101 and omit that cause.
    static CARGO_FATAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:error: could not compile|(?:error: )?process didn't exit successfully:)")
            .unwrap()
    });
    static CARGO_FAILED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:error: (?:test failed|[0-9]+ targets? failed)|test result: FAILED\.)")
            .unwrap()
    });
    static JAVASCRIPT_FATAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:(?:Type )?Errors\s+[1-9][0-9]* errors?\b|Unhandled Errors\b)").unwrap()
    });
    static JAVASCRIPT_FAILED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:Test Suites:|Test Files)\s+.*\b[1-9][0-9]* failed\b").unwrap()
    });
    let mut words = command.split_whitespace();
    let runner = match words.next() {
        Some("npx") => words.next(),
        runner => runner,
    };
    text.lines().map(str::trim).any(|line| match runner {
        Some("cargo") => {
            CARGO_FATAL.is_match(line) || (!reported_failures && CARGO_FAILED.is_match(line))
        }
        Some("pytest") => {
            line.starts_with("INTERNALERROR>")
                || line
                    .trim_matches('_')
                    .trim()
                    .starts_with("ERROR collecting ")
        }
        Some("vitest" | "jest") => {
            JAVASCRIPT_FATAL.is_match(line)
                || (runner == Some("jest")
                    && line.strip_prefix('●').unwrap_or(line).trim_start()
                        == "Test suite failed to run")
                || (!reported_failures
                    && (JAVASCRIPT_FAILED.is_match(line) || line.starts_with("FAIL ")))
        }
        _ => false,
    })
}
