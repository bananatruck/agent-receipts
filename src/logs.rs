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
    pub fn read(values: &[String], files: &[String]) -> Result<Self, String> {
        let mut logs = BTreeMap::new();
        let mappings = values
            .iter()
            .map(|value| {
                value
                    .rsplit_once('=')
                    .ok_or("--log requires COMMAND=PATH, for example --log 'cargo test=run.txt'")
            })
            .chain(
                files
                    .chunks_exact(2)
                    .map(|pair| Ok((pair[0].as_str(), pair[1].as_str()))),
            );
        for mapping in mappings {
            let (command, path) = mapping?;
            if command.is_empty() || path.is_empty() {
                return Err("log command and path must both be nonempty".into());
            }
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
                let runner = Runner::of(command).ok_or("unsupported command family")?;
                let diagnostic = body
                    .lines()
                    .filter_map(|l| diagnostic(runner, l))
                    .max()
                    .max(runner.unaccounted_failures(body));
                match diagnostic {
                    Some(Diagnostic::Abnormal) => return Ok(None),
                    Some(Diagnostic::Failure) if failed.unwrap_or(0) == 0 => return Ok(None),
                    _ => {}
                }
                runner
                    .summary(body)
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
            || (c[1].eq("ok") != (failed == 0))
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
    totaled_items(&items, total, separator)
}

// All categories, including skipped and todo, must add up before counts
// can be used as either a summary or a diagnostic.
fn totaled_items(items: &str, total: u64, separator: &str) -> Option<Counts> {
    let counts = count_items(items, &["passed", "failed", "skipped", "todo"], separator)?;
    let sum = items.split(separator).try_fold(0u64, |sum, item| {
        sum.checked_add(item.trim().split_once(' ')?.0.parse().ok()?)
    })?;
    (sum == total).then_some(counts)
}

// How a supplied log becomes a verdict, for every supported runner.
//
// 1. The command decides the runner. Each runner reads only its own lines.
// 2. A diagnostic line from the table below is either Abnormal or Failure.
//    Abnormal: the runner reports an error that its test counts do not
//    contain (a compile error, a test binary that could not be launched or
//    exited abnormally, a suite that failed to load or set up, a collection,
//    internal or unhandled error). No count can account for it, so the
//    statement is contradicted. One Abnormal rule spans two lines and sits
//    beside the table: a Vitest failed-file total above the failed-test total.
//    Failure: an ordinary failed-test marker. Its tests are inside the
//    counts, so it contradicts only a statement that reports no failures.
// 3. Otherwise the runner's parser must find one complete summary: one
//    final count line for pytest, Vitest and Jest, and for Cargo every
//    announced suite completed with a matching header and summary. Its
//    counts confirm or contradict the statement.
// 4. Missing, partial, conflicting or malformed runner lines are not
//    checkable. A diagnostic look-alike in ordinary output can only stop a
//    confirmation, never cause one, so diagnostics match the trimmed line.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Runner {
    Cargo,
    Pytest,
    Vitest,
    Jest,
}

impl Runner {
    fn of(command: &str) -> Option<Self> {
        match command.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["cargo", "test", ..] => Some(Self::Cargo),
            ["pytest", ..] => Some(Self::Pytest),
            ["vitest", ..] | ["npx", "vitest", ..] => Some(Self::Vitest),
            ["jest", ..] | ["npx", "jest", ..] => Some(Self::Jest),
            _ => None,
        }
    }

    fn summary(self, body: &str) -> Option<Counts> {
        match self {
            Self::Cargo => cargo_summary(body),
            Self::Pytest => pytest_summary(body),
            Self::Vitest => {
                vitest_file_summary(body)?;
                javascript_summary(body)
            }
            Self::Jest => javascript_summary(body),
        }
    }

    // The one Abnormal rule that needs two lines; see vitest_files_outside_tests.
    fn unaccounted_failures(self, body: &str) -> Option<Diagnostic> {
        match self {
            Self::Vitest => vitest_files_outside_tests(body).then_some(Abnormal),
            Self::Cargo | Self::Pytest | Self::Jest => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Diagnostic {
    Failure,
    Abnormal,
}

use Diagnostic::{Abnormal, Failure};
use Runner::{Cargo, Jest, Pytest, Vitest};

// Vitest and Jest share one summary parser, so their diagnostics apply to
// both; a look-alike line can only stop a confirmation.
const JAVASCRIPT: &[Runner] = &[Vitest, Jest];

#[rustfmt::skip]
const RULES: &[(&[Runner], Diagnostic, &str)] = &[
    // Cargo 1.85 src/cargo/ops/cargo_test.rs. Abnormal exits print an
    // unprefixed cause even when other suites counted assertion failures;
    // ordinary assertion exits use status 101 and omit that cause.
    (&[Cargo], Abnormal, r"^error: could not compile"),
    (&[Cargo], Abnormal, r"^(?:error: )?process didn't exit successfully:"),
    // Cargo 1.85 crates/cargo-util/src/process_error.rs: a test binary that
    // could not be spawned prints `could not execute process CMD (never
    // executed)` as its cause, and its tests are in no count.
    (&[Cargo], Abnormal, r"^(?:error: )?could not execute process\b"),
    (&[Cargo], Failure, r"^error: (?:test failed|[0-9]+ targets? failed)"),
    (&[Cargo], Failure, r"^test result: FAILED\."),
    // pytest: internal errors, and collection errors whose tests were never
    // counted, plain or as an underscore-decorated heading.
    (&[Pytest], Abnormal, r"^INTERNALERROR>"),
    (&[Pytest], Abnormal, r"^_*\s*ERROR collecting "),
    // Jest: a suite that failed to run, optionally with its bullet.
    (&[Jest], Abnormal, r"^(?:●\s*)?Test suite failed to run$"),
    // Vitest 3.2.4 packages/vitest/src/node/reporters/base.ts and logger.ts.
    // Errors counts unhandled errors. Banners sit inside dividers of U+23AF:
    // Failed Suites holds suites that failed to load or whose hooks failed,
    // Unhandled Errors and Source Errors hold errors outside any test.
    (JAVASCRIPT, Abnormal, r"^Errors\s+[1-9][0-9]* errors?\b"),
    (JAVASCRIPT, Abnormal, r"^⎯*\s*(?:Unhandled Errors|Source Errors|Failed Suites [1-9][0-9]*)\b"),
    // Type Errors counts failed type tests, a subset of the Tests line's
    // failures (base.ts filters `tests` for typecheck tests with errors), so
    // it is an ordinary failure. Type errors outside a test are reported as
    // a failed suite or an unhandled source error instead.
    (JAVASCRIPT, Failure, r"^Type Errors\s+[1-9][0-9]* (?:failed|errors?)\b"),
    // A failed Test Files total is ordinary on its own. Above the Tests line's
    // failed total it is Abnormal; that compares two lines, so it is
    // Runner::unaccounted_failures, with its source in
    // vitest_files_outside_tests.
    (JAVASCRIPT, Failure, r"^(?:Test Suites:|Test Files)\s+.*\b[1-9][0-9]* failed\b"),
    (JAVASCRIPT, Failure, r"^FAIL "),
];

fn diagnostic(runner: Runner, line: &str) -> Option<Diagnostic> {
    static COMPILED: LazyLock<Vec<(&[Runner], Diagnostic, Regex)>> = LazyLock::new(|| {
        RULES
            .iter()
            .map(|&(runners, kind, pattern)| (runners, kind, Regex::new(pattern).unwrap()))
            .collect()
    });
    let line = line.trim();
    if JAVASCRIPT.contains(&runner) && line.starts_with("Test Files") {
        // The table's file-failure rule requires a valid category total.
        vitest_file_line(line)?;
    }
    COMPILED
        .iter()
        .filter(|(runners, _, pattern)| runners.contains(&runner) && pattern.is_match(line))
        .map(|&(_, kind, _)| kind)
        .max()
}

// Vitest 3.2.4 reporters/base.ts reportTestSummary prints `Test Files` and
// `Tests` with one state count over the files and the tests inside them, and
// @vitest/runner runSuite fails a file when a test in it failed or when the
// file itself errored (load, hook, no tests). Every failed test fails its own
// file, so more failed files than failed tests means a file failed outside
// its tests: the state behind a Failed Suites banner, which printErrorsSummary
// writes to stderr, so a stdout-only capture lacks it. Equal or fewer failed
// files prove nothing either way, so this catches some abnormal runs, not all.
fn vitest_files_outside_tests(body: &str) -> bool {
    let (Some(tests), Some(Some(files))) = (javascript_summary(body), vitest_file_summary(body))
    else {
        return false;
    };
    files.failed > tests.failed
}

// Vitest 3.2.4 reporters/base.ts reportTestSummary: file categories use
// the same parenthesized total as test categories. Absence is allowed,
// but a present malformed or duplicate file summary is ambiguous evidence.
fn vitest_file_summary(body: &str) -> Option<Option<Counts>> {
    let lines: Vec<_> = body
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("Test Files"))
        .collect();
    match lines.as_slice() {
        [] => Some(None),
        [line] => vitest_file_line(line).map(Some),
        _ => None,
    }
}

fn vitest_file_line(line: &str) -> Option<Counts> {
    static FILES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^Test Files\s+(.+) \(([0-9]+)\)$").unwrap());
    let c = FILES.captures(line)?;
    totaled_items(&c[1], c[2].parse().ok()?, " | ")
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn diagnostics_follow_each_runners_rules() {
        const NONE: &[Runner] = &[];
        // Each line is checked under every runner: it is a diagnostic of the
        // expected kind for the listed runners and ordinary output otherwise.
        #[rustfmt::skip]
        let rows: &[(&str, &[Runner], Option<Diagnostic>)] = &[
            ("error: could not compile `fixture` (lib test) due to 1 previous error", &[Cargo], Some(Abnormal)),
            ("error: process didn't exit successfully: `target/debug/deps/x-1` (signal: 11)", &[Cargo], Some(Abnormal)),
            ("  process didn't exit successfully: `target/debug/deps/x-1` (exit status: 4)", &[Cargo], Some(Abnormal)),
            ("  could not execute process `target/debug/deps/x-1 --quiet` (never executed)", &[Cargo], Some(Abnormal)),
            ("error: could not execute process `target/debug/deps/x-1` (never executed)", &[Cargo], Some(Abnormal)),
            ("Running: could not execute process", NONE, None),
            ("error: test failed, to rerun pass `--lib`", &[Cargo], Some(Failure)),
            ("error: 2 targets failed:", &[Cargo], Some(Failure)),
            ("test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s", &[Cargo], Some(Failure)),
            ("test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s", NONE, None),
            ("INTERNALERROR> RuntimeError: fixture", &[Pytest], Some(Abnormal)),
            ("ERROR collecting tests/test_fixture.py", &[Pytest], Some(Abnormal)),
            ("_____ ERROR collecting tests/test_fixture.py _____", &[Pytest], Some(Abnormal)),
            ("_____ test_assertion _____", NONE, None),
            ("  ● Test suite failed to run", &[Jest], Some(Abnormal)),
            ("Test suite failed to run", &[Jest], Some(Abnormal)),
            ("     Errors  1 error", JAVASCRIPT, Some(Abnormal)),
            ("Errors 2 errors", JAVASCRIPT, Some(Abnormal)),
            ("⎯⎯⎯⎯⎯⎯ Unhandled Errors ⎯⎯⎯⎯⎯⎯", JAVASCRIPT, Some(Abnormal)),
            ("Unhandled Errors", JAVASCRIPT, Some(Abnormal)),
            ("⎯⎯⎯⎯⎯⎯ Source Errors ⎯⎯⎯⎯⎯⎯", JAVASCRIPT, Some(Abnormal)),
            ("⎯⎯⎯⎯⎯⎯ Failed Suites 1 ⎯⎯⎯⎯⎯⎯⎯", JAVASCRIPT, Some(Abnormal)),
            ("⎯⎯⎯⎯⎯⎯⎯ Failed Tests 1 ⎯⎯⎯⎯⎯⎯⎯", NONE, None),
            ("Type Errors  1 failed", JAVASCRIPT, Some(Failure)),
            ("Type Errors 1 error", JAVASCRIPT, Some(Failure)),
            ("Type Errors  no errors", NONE, None),
            (" Test Files  2 failed (2)", JAVASCRIPT, Some(Failure)),
            ("Test Files 2 failed (1)", NONE, None),
            ("Test Files 1 failed | 1 skipped (1)", NONE, None),
            ("Test Files 2 failed (", NONE, None),
            ("Test Suites: 1 failed, 1 total", JAVASCRIPT, Some(Failure)),
            (" FAIL  math.test.ts > adds", JAVASCRIPT, Some(Failure)),
            ("      Tests  1 failed (1)", NONE, None),
            (" Test Files  1 passed (1)", NONE, None),
            ("Vitest caught 1 unhandled error during the test run.", NONE, None),
            ("Running query (cached)", NONE, None),
            ("FAILED to connect, retrying", NONE, None),
        ];
        for &(line, runners, expected) in rows {
            for runner in [Cargo, Pytest, Vitest, Jest] {
                let want = if runners.contains(&runner) {
                    expected
                } else {
                    None
                };
                assert_eq!(diagnostic(runner, line), want, "{runner:?} {line:?}");
            }
        }
    }

    #[test]
    fn vitest_failed_files_beyond_failed_tests_are_abnormal() {
        for (log, expected) in [
            (
                " Test Files  2 failed (2)\n      Tests  1 failed (1)\n",
                Some(Abnormal),
            ),
            (
                " Test Files  1 failed (1)\n      Tests  1 skipped (1)\n",
                Some(Abnormal),
            ),
            (
                " Test Files  1 failed (1)\n      Tests  1 passed (1)\n",
                Some(Abnormal),
            ),
            (
                " Test Files  1 failed (1)\n      Tests  1 failed (1)\n",
                None,
            ),
            (
                " Test Files  1 failed (1)\n      Tests  2 failed (2)\n",
                None,
            ),
            (
                " Test Files  1 failed | 1 passed (2)\n      Tests  1 failed | 3 passed (4)\n",
                None,
            ),
            // Without one complete Tests line the summary is not checkable.
            (" Test Files  2 failed (2)\n", None),
        ] {
            assert_eq!(Vitest.unaccounted_failures(log), expected, "{log:?}");
            assert_eq!(Jest.unaccounted_failures(log), None, "{log:?}");
        }
    }
}
