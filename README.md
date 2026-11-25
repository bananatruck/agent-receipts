# agent-receipts

Check a coding report against local Git evidence and supplied command logs.
Each statement gets confirmed, contradicted, or not checkable, with the evidence shown.
The executable runs offline and never executes report text.

## Run a check

Install stable Rust and Git, then build the executable.

```sh
cargo build --locked --release
./target/release/agent-receipts --repo . --report examples/synthetic-report.md
printf 'Tree is clean\n' | ./target/release/agent-receipts --repo .
./target/release/agent-receipts --help
```

`--report -` reads stdin and is the default.
Use one statement per line, optionally with a Markdown bullet and a final period.
Blank lines and recognized fence delimiter lines are ignored.
Headings remain visible as not checkable, so a claim written in a heading cannot silently pass CI.
Quoted code and unsupported wording stay not checkable.

```text
Commit <hash> exists
File src/main.rs exists
Tree is clean
Ran `cargo test`, 77 passed
Pushed
Not pushed <hash>
Only touched src/main.rs, `notes/a b.txt`
Did not modify LICENSE
Added src/new.rs
Created src/new.rs
Deleted src/old.rs
Renamed src/new-name.rs
Renamed src/old-name.rs to src/new-name.rs
```

Paths are literal repository-root-relative entries, with backticks required for spaces.
Directory prefixes and globs do not match groups of files.
`File path exists` checks for a blob at HEAD.
`Added` and `Created` require an added file endpoint, including the destination of a detected rename.
`Deleted` also accepts the source of a detected rename.
`Did not modify` requires a literal file present at one or both diff endpoints.
Directories, missing or case-mismatched paths, and paths inside submodules stay not checkable for exclusions.
An empty endpoint diff satisfies an allowlist and an exclusion of an existing file.

## Supply evidence

Use `--base COMMIT` for path scope and action checks.
The diff compares that commit's tree with HEAD's tree and excludes working-tree changes and intermediate edits later undone.
Both endpoints of a rename count as touched paths.
Git's 50 percent similarity threshold determines rename status.
The exhaustive rename limit is disabled so repository configuration cannot skip detection.

Use `--remote-ref refs/remotes/origin/main` for pushed-state checks against an existing local tracking ref.
Bare statements check HEAD, independently of commit-existence statements elsewhere in the report.
An explicit hash on a pushed-state statement selects only that commit.
An explicit hash must identify exactly one local commit object across all object types; annotated tag objects are not commit hashes.
The evidence is a local snapshot that may be stale; it does not prove the server's present state or push history.
No fetch or other network request occurs.

Use repeatable `--log 'COMMAND=PATH'` flags for captured UTF-8 command logs.
The final equals sign separates the command from the path, so arguments such as `--maxfail=1` work.
Log paths containing equals signs are unsupported; supply the log through a path without equals signs.
Each log must contain exactly one `$ COMMAND` record on its own line, followed by the captured output.
The record, flag, and report must use exactly the same command, including its flags and spaces.
The supported families are `cargo test`, `pytest`, `vitest`, `jest`, `npx vitest`, and `npx jest`, with optional arguments.
Use plain output without ANSI color codes.
The cargo parser requires matching `running N tests` headers and completed summaries, and combines completed suites.
Missing headers, mismatched suite totals, duplicate summaries, and unfinished suites stay not checkable.
Cargo announcements require five leading spaces before `Running`, a source path and a parenthesized executable path, or three leading spaces before `Doc-tests` and a crate name.
Executable paths can use a custom target directory and must contain a path separator.
Ordinary stdout such as `Running initialization` or `Running query (cached)` does not announce a suite.
Malformed, whitespace-altered, unfinished, or conflicting announcement-shaped lines stay not checkable.
The other parsers require one unambiguous final test summary.
An incomplete additional summary makes counts not checkable.
Failures contradict test-success statements, even when the passed count matches.
A statement that explicitly reports a nonzero failure count can confirm when both counts match.
Runner errors outside those test counts, such as crashes, internal errors, or unhandled errors, contradict the statement.
Cargo's `process didn't exit successfully:` cause contradicts even matching assertion counts, including quiet runs without suite announcements or an `error:` prefix on the cause.
So does Cargo's `could not execute process` cause for a test binary that was never launched.
Pytest collection errors, including underscore-decorated headings, contradict even matching failure counts.
Vitest failed suites, unhandled errors, and source errors contradict even matching failure counts.
In a stdout-only Vitest capture without those banners, more failed test files than failed tests also contradicts.
Vitest type-test failures are already in its Tests count, so matching counts can confirm a statement that reports them.
Diagnostics are recognized only for the corresponding command family.
The output states that a supplied log is evidence only of its contents, not authentic execution in this checkout.

This captures a real run with the required command record.
The status check prevents a failed test command from being hidden by a later successful shell command.

```sh
capture=$(mktemp)
printf '$ cargo test --locked --all-targets\n' > "$capture"
CARGO_BUILD_JOBS=2 cargo test --locked --all-targets >> "$capture" 2>&1
result=$?
cat "$capture"
[ "$result" -eq 0 ] || exit "$result"
./target/release/agent-receipts --repo . --base 670dafc \
  --report examples/synthetic-history-report.md \
  --log "cargo test --locked --all-targets=$capture"
```

The synthetic history report deliberately contains false statements, so this last command exits 1.
Keep the capture file if you want to inspect its evidence, then remove it when finished.
A bare count such as `77 tests passed` lacks a command and stays not checkable.

## Real history example

The input in [examples/synthetic-history-report.md](examples/synthetic-history-report.md) is synthetic.
The repository history and the test output used to check it are real.
The run below was captured with HEAD at `2ae7b07` and base `670dafc`, before committing the fourth-pass fix and refreshed example.
The captured test run reported 4 parser tests and 53 CLI tests passed, with no failures.
Re-running after later commits can change the endpoint hash and diff records, and adding tests can change the count verdicts.

```text
LINE	STATEMENT	CHECK	VERDICT	EVIDENCE
1	# Synthetic report about this repository's history	unsupported	not checkable	no supported assertion grammar; no evidence check performed
3	Commit 2ae7b07 exists	commit exists locally	confirmed	local object 2ae7b072e373f97b6bbe3dc2adae186d5b23255c has type commit
4	File src/missing.rs exists	file at HEAD	contradicted	no file entry at HEAD:src/missing.rs
5	Added src/logs.rs	committed path action	confirmed	committed diff 670dafc0136bcf3dd3f1e0443bd34619ed88945e..2ae7b072e373f97b6bbe3dc2adae186d5b23255c: M Cargo.lock; M Cargo.toml; M README.md; M SPEC.md; M examples/demo.sh; A examples/history-output.tsv; A examples/synthetic-history-report.md; M src/git.rs; A src/logs.rs; M src/main.rs; M src/report.rs; A submission/DEVPOST.md; A submission/demo-script.md; M tests/cli.rs; excludes working-tree changes and intermediate edits
6	Did not modify LICENSE	committed diff scope	confirmed	committed diff 670dafc0136bcf3dd3f1e0443bd34619ed88945e..2ae7b072e373f97b6bbe3dc2adae186d5b23255c: M Cargo.lock; M Cargo.toml; M README.md; M SPEC.md; M examples/demo.sh; A examples/history-output.tsv; A examples/synthetic-history-report.md; M src/git.rs; A src/logs.rs; M src/main.rs; M src/report.rs; A submission/DEVPOST.md; A submission/demo-script.md; M tests/cli.rs; excludes working-tree changes and intermediate edits
7	Ran `cargo test --locked --all-targets`, 57 passed	test counts in supplied log	confirmed	cargo test --locked --all-targets: 57 passed, 0 failed; evidence covers only what the supplied log contains, not execution or authenticity
8	Ran `cargo test --locked --all-targets`, 58 passed	test counts in supplied log	contradicted	cargo test --locked --all-targets: 57 passed, 0 failed; evidence covers only what the supplied log contains, not execution or authenticity
9	Not pushed	local tracking ref ancestry	not checkable	supply --remote-ref refs/remotes/NAME/BRANCH; no network is used
Summary: 4 confirmed, 2 contradicted, 2 not checkable
Exit: 1
```

The missing file and the count of 58 are contradicted.
The report heading is retained as not checkable.
The addition and LICENSE exclusion match the committed diff.
`Not pushed` stays not checkable because this run supplies no tracking ref.
The full table is also saved in [examples/history-output.tsv](examples/history-output.tsv).

## JSON schema

Add `--json` to emit one JSON object followed by a newline, without a table or extra stdout text.
Operational errors still go to stderr and exit 2 without a results document.
Schema version 1 has these required fields.
JSON object key order is not part of the contract.

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | integer | Always `1` for this schema |
| `statements` | array | Results in source order, empty for an empty report |
| `statements[].line` | positive integer | Original one-based source line |
| `statements[].statement` | string | Trimmed source line, including its bullet if present |
| `statements[].check` | string | Check label, such as `file at HEAD` or `committed diff scope` |
| `statements[].verdict` | string | `confirmed`, `contradicted`, or `not checkable` |
| `statements[].evidence` | string | Human-readable observed evidence or reason it is unavailable |
| `summary.confirmed` | nonnegative integer | Confirmed statement count |
| `summary.contradicted` | nonnegative integer | Contradicted statement count |
| `summary.not_checkable` | nonnegative integer | Uncheckable statement count |
| `exit_code` | integer | Same CI code returned by the process |

Source and evidence strings use JSON escaping rather than the table's visible cell escaping.
Evidence prose is explanatory text, not a machine-parseable sub-schema.

| Exit | Meaning |
| --- | --- |
| 0 | At least one statement and all confirmed |
| 1 | Any contradiction, including reports with missing evidence too |
| 2 | Invalid invocation, unreadable input, unavailable Git, or invalid working repository |
| 3 | No contradiction, but some missing evidence or no statements |

## Check and demo

```sh
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --locked --all-targets -- -D warnings
CARGO_BUILD_JOBS=2 cargo test --locked --all-targets
CARGO_BUILD_JOBS=2 cargo build --locked --release
sh examples/demo.sh
```

On shared machines, install `flock` and wrap each heavy command once with the same lock file.
This optional procedure requires `flock` in addition to Rust and Git.

```sh
mkdir -p ~/overnight-2026-11-17/logs
CARGO_BUILD_JOBS=2 flock -w 1800 ~/overnight-2026-11-17/logs/heavy.lock cargo build --locked --release
```

Tests create isolated repositories under the system temp directory.
The runnable demo changes a throwaway repository and checks the expected exit codes.
See [SPEC.md](SPEC.md) for exact grammar and evidence limits, [submission/DEVPOST.md](submission/DEVPOST.md) for the submission draft, and [submission/demo-script.md](submission/demo-script.md) for the video plan.

## Limits

The program uses fixed grammar rather than arbitrary natural-language interpretation.
It cannot prove code quality, intent, authorship, report completeness, evidence authenticity, or when tests ran.
An endpoint diff cannot reveal every path edited during a session.
A clean-tree check excludes ignored files and describes only its observation time.
Index visibility is checked before and after status.
This catches flags left set during status but cannot detect every transient concurrent change.
Configured clean or process filters, hidden index entries marked assume-unchanged or skip-worktree, and submodule worktrees make cleanliness not checkable.
File actions on submodule entries are also not checkable.
File checks inspect stored HEAD entries without following intermediate symlinks.
The Markdown parser supports the documented line grammar, code fences, indentation, and list prefixes; it is not a general Markdown renderer.
Terminal tables and errors raised while reading input or evidence escape controls and Unicode format characters, including bidirectional overrides.
JSON preserves the original strings and is intended for consumers that handle display escaping.

The program trusts the installed Git executable, its configuration parser, and the local object store as evidence.
It reads repository configuration but disables replacement objects, filesystem monitor hooks, external diff drivers, text conversion, and lazy fetching.
An empty Git protocol allowlist also blocks direct promisor fetches.
Global and system Git configuration and inherited configuration overrides are excluded.
It checks filter configuration before status and refuses filtered repositories or submodule worktrees instead of running their configured programs.
These precautions do not make Git a sandbox for corrupted repositories or prove that local objects and refs are authentic.
Some truncation between completed test suites is indistinguishable from a shorter complete log.
Repository evidence is not atomic if another process changes it during verification.
The [MIT license](LICENSE) applies.
