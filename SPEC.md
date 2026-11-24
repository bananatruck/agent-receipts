# Specification

## Purpose

`agent-receipts` checks explicit statements in a coding report against local repository evidence and supplied command logs.
The user is a developer or reviewer who needs to check a final report before accepting work.
Every statement gets a verdict and the evidence behind it.
Missing evidence stays not checkable.

## Report grammar

Reports are UTF-8 plain text or Markdown from stdin or a file, with one statement per line.
Keywords are case insensitive, but command records and paths match exactly.
An optional Markdown bullet or numbered-list prefix and an optional final period are accepted.
Hashes and individual paths may have one pair of backticks.
Paths with spaces require backticks.
Paths are literal repository-root-relative entries, without absolute roots, empty components, parent components, backslashes, control characters, or Git revision syntax.
Directory prefixes and globs do not stand for sets of paths.
Blank lines and Markdown ATX headings produce no verdict.
Fenced code and lines indented by four spaces or a tab are quoted material and cannot confirm statements.
Fence delimiter lines produce no verdict.
Other unsupported lines retain their source line number and receive not checkable.
Negations, conditionals, or statements embedded in longer prose are unsupported unless listed below.
No report text is executed.

| Statement | Check |
| --- | --- |
| `Committed as <hash>` or `Commit <hash> exists` | A unique local object has type commit |
| `File <path> exists` | A blob exists at `HEAD:<path>` |
| `Tree is clean` or `The tree is clean` | Git porcelain status has no changes |
| ``Ran `<command>`, N passed`` | Matching command record and completed summary in a supplied log |
| ``Ran `<command>`, N passed, M failed`` | The same count check, including the stated failure count |
| `Pushed` | All named commits, or HEAD, are reachable from the local tracking ref |
| `Not pushed` or `Did not push` | All named commits, or HEAD, are absent from that ancestry |
| `Pushed <hash>`, `Not pushed <hash>`, or `Did not push <hash>` | The explicit commit is checked instead |
| `Only touched <path>, <path>` | Every changed path in the committed diff is in the allowlist |
| `Did not modify <path>` | The committed diff contains no change at that exact path |
| `Added <path>` or `Created <path>` | The committed diff reports status A at that path |
| `Deleted <path>` | The committed diff reports status D at that path |
| `Renamed <path>` | The committed diff reports status R with that destination |
| `Renamed <old> to <new>` | The committed diff reports status R with both endpoints |

The allowlist can contain one path or several paths separated by comma and space outside backticks.
An empty diff satisfies an allowlist or an exclusion, but cannot confirm an addition, deletion, or rename.
A bare test count such as `77 tests passed` lacks a command and stays not checkable.

## Repository evidence

Commit hashes contain 7 to 64 hexadecimal characters and may be abbreviated if they identify exactly one local object.
Missing objects contradict local existence, while ambiguous abbreviations stay not checkable.
A directory or submodule is not a file blob.
A symbolic link counts as its stored blob without following the link.
File existence concerns HEAD, not an uncommitted working copy.
Missing HEAD makes file, diff, and default-HEAD ancestry evidence unavailable.
Clean-tree checks include staged, unstaged, untracked, and reported submodule changes, and exclude ignored files.
They describe the observation time rather than a past state.

`--base COMMIT` resolves a local commit and compares its tree with HEAD's tree.
This is the net committed diff, excluding staged, unstaged, untracked, and intermediate changes that were later undone.
The base need not be an ancestor of HEAD.
Checks show both resolved endpoint hashes and Git's name-status records.
Rename detection uses Git's 50 percent similarity threshold.
Both endpoints of renames count as touched paths for allowlists and exclusions.
A heavily edited rename that Git classifies as an addition and a deletion cannot confirm a rename statement.
Paths remain root relative even if `--repo` selects a subdirectory or Git configuration requests relative diffs.
Unavailable objects, invalid base revisions, or unreadable diff records produce not checkable.

`--remote-ref refs/remotes/NAME/BRANCH` names an existing local tracking ref.
No fetch, push, or network request occurs.
A bare pushed-state statement uses every hash in supported commit-existence statements in the report, or HEAD if none are named.
An explicit hash on the pushed-state statement overrides that set.
Each selected commit must satisfy the asserted ancestry relation.
The evidence includes resolved commits and the tracking ref's current local hash.
This confirms only reachability in the local snapshot, not the server's present state, whether a push command ran, or whether a commit was ever pushed elsewhere.
Missing refs, invalid names, and unresolved commits stay not checkable.
Shallow history cannot confirm absence from ancestry.

## Supplied command logs

Repeat `--log 'COMMAND=PATH'` for distinct commands.
Paths resolve from the invocation directory, independently of `--repo`.
Each log is UTF-8 text and must contain exactly one command record, written as `$ COMMAND` on its own line before the output.
The record, flag, and report command must match exactly, including flags and spaces.
CRLF line endings are normalized.
Duplicate flags for the same command, unreadable logs, and malformed flag values are input errors.
Logs with missing or multiple command records, unsupported commands, or missing or ambiguous summaries stay not checkable.
The supported families are `cargo test`, `pytest`, `vitest`, `jest`, `npx vitest`, and `npx jest`, followed by optional arguments.
Other wrappers and ANSI-colored summaries are unsupported.

The small cargo parser sums completed `test result: ok.` or `test result: FAILED.` summaries across suites.
A summary must include passed, failed, ignored, measured, filtered-out, and elapsed-seconds fields.
A later suite-start line without a completed summary makes the counts not checkable.
The pytest parser accepts exactly one final count list followed by `in <seconds>s`, optionally surrounded by equals signs or followed by a duration annotation.
It recognizes passed, failed, errors, skipped, deselected, expected-failure, unexpected-pass, and warning categories using their standard summary tokens.
Errors contribute to the failure count.
The JavaScript parser accepts one `Tests` line with pipe-separated categories and a parenthesized total, or one `Tests:` line with comma-separated categories and a final total.
It recognizes passed, failed, skipped, and todo categories and checks their sum against the total.
Suite and file totals do not count as test totals.
Recognized runner failures, including failed suites or compilation failures, contradict a test-success statement even if its passed count matches.
Otherwise matching passed counts and zero failures confirm the statement, while differing counts or any test failures contradict it.
An optional reported failure count must also match.
All log evidence describes only what the supplied text contains, not execution, authenticity, repository identity, or freshness.
Truncation between completed suites without a visible next-suite start cannot be detected.

## Command line and output

```text
agent-receipts [--repo PATH] [--report PATH|-]
               [--base COMMIT] [--remote-ref refs/remotes/NAME/BRANCH]
               [--log COMMAND=PATH]... [--json]
agent-receipts --help
agent-receipts --version
```

`--repo` defaults to the current directory and accepts a subdirectory of a working repository.
`--report` defaults to `-` for stdin until EOF.
An interactive terminal without a report fails with an actionable error instead of waiting.
Report file paths resolve from the invocation directory.
Default stdout is a tab-separated verdict table with `LINE`, `STATEMENT`, `CHECK`, `VERDICT`, and `EVIDENCE` columns and a summary.
Cell escaping makes tabs, carriage returns, backslashes, and control characters visible.
`--json` emits one JSON document with stable schema version 1, documented in [README.md](README.md#json-schema).
JSON retains source text using normal JSON escaping.
Verdicts are `confirmed`, `contradicted`, and `not checkable` in both formats.
Operational errors go to stderr, with no successful-results JSON document.
Help and version exit 0 without opening evidence or reading stdin.

| Exit | Meaning |
| --- | --- |
| 0 | At least one statement, all confirmed |
| 1 | At least one contradiction, including mixed reports |
| 2 | Invalid invocation, unreadable input, unavailable Git, or invalid working repository |
| 3 | No contradictions, but at least one uncheckable statement or no statements |

Per-statement evidence failures stay not checkable.
Contradictions take precedence over uncheckable statements.

## Implementation and limits

This project started on November 24, 2026.
Git subprocesses define repository facts without adding a Git library.
Arguments are passed without a shell, optional index locks and filesystem monitor hooks are disabled, and lazy object fetching is disabled.
Repository-location environment variables are removed so `--repo` chooses the checked repository.
External diff drivers and text conversion are disabled for diff checks.
The command-line parser, fixed grammar, and JSON serializer use established libraries listed in Cargo.toml.
Integration tests create isolated repositories under the system temp directory, with local fixture identity, signing disabled, and hooks disabled.
No model calls, credentials, network requests, or report-command execution are needed.
The tool cannot prove intent, authorship, code quality, report completeness, or evidence authenticity.
It trusts the local Git executable and object store.
Repository evidence is not an atomic snapshot when another process changes the repository during a check.

## Validation and demo

Each new statement family starts with a failing executable test, and each log parser has focused tests with literal summaries.
Coverage includes matching and mismatched counts, missing evidence, failed runners, incomplete suites, stale local refs, renamed paths, invalid inputs, JSON, CI exits, and subdirectory selection.
Checks run formatting, lint with warnings denied, the full suite, a release build, and the executable demos.
The README records a synthetic report checked against this repository's real history, with captured output.
The [video script](submission/demo-script.md) fits within three minutes.
The [submission draft](submission/DEVPOST.md) describes the build and remaining author actions honestly.
Eligibility, registration, public code publication, a recorded video, tool disclosure, and final submission remain the author's responsibility.
