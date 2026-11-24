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
                if runner_failure(command, body, counts.as_ref().is_some_and(|c| c.failed > 0)) {
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
    let mut announced = false;
    let mut running = None;
    for line in text.lines().map(str::trim) {
        if line.starts_with("Running ") || line.starts_with("Doc-tests ") {
            if announced || running.is_some() {
                return None;
            }
            announced = true;
        }
        if let Some(c) = RUNNING.captures(line) {
            if running.is_some() {
                return None;
            }
            running = Some(c[1].parse::<u64>().ok()?);
        }
        if !line.starts_with("test result:") {
            continue;
        }
        let expected = running.take()?;
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
        announced = false;
    }
    (found && !announced && running.is_none()).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

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

fn runner_failure(command: &str, text: &str, counted_failures: bool) -> bool {
    static FATAL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:error: (?:could not compile|process didn't exit successfully)|(?:Type )?Errors\s+[1-9][0-9]* errors?\b|Unhandled Errors\b|INTERNALERROR>|ERROR collecting )").unwrap()
    });
    static FAILED: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:error: (?:test failed|[0-9]+ targets? failed)|test result: FAILED\.|(?:Test Suites:|Test Files)\s+.*\b[1-9][0-9]* failed\b)").unwrap()
    });
    let javascript = matches!(
        command.split_whitespace().next(),
        Some("vitest" | "jest" | "npx")
    );
    text.lines().map(str::trim).any(|line| {
        FATAL.is_match(line)
            || (!counted_failures
                && (FAILED.is_match(line) || (javascript && line.starts_with("FAIL "))))
    })
}
