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
Blank lines produce no verdict.
Markdown ATX headings remain unsupported rows and receive not checkable.
Fenced code, including list-item fences, and lines indented to column four or beyond using spaces or tabs are quoted material and cannot confirm statements.
List-marker padding of five or more columns is also quoted code.
Padding tabs advance to four-column stops from the marker's original column, including leading indentation.
A fence marker indented four columns or more beyond its container cannot close an active fence.
An unclosed list-item fence ends when its list item ends.
An unclosed top-level fence continues to the end of the report.
Closing fences accept only ASCII spaces and tabs around the delimiter.
Backtick fence info strings cannot contain backticks.
Tilde fence info strings accept repeated tildes.
Inline-looking tilde openers remain visible as unsupported rows, and their following content remains quoted code.
Inline multi-backtick or tilde spans remain unsupported rows.
Other fence delimiter lines produce no verdict.
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
| `Pushed` | HEAD is reachable from the local tracking ref |
| `Not pushed` or `Did not push` | HEAD is absent from that ancestry |
| `Pushed <hash>`, `Not pushed <hash>`, or `Did not push <hash>` | One unique local commit object is checked instead |
| `Only touched <path>, <path>` | Every changed path in the committed diff is in the allowlist |
| `Did not modify <path>` | A file exists at an endpoint and the diff contains no change at that exact path |
| `Added <path>` or `Created <path>` | The diff adds a file at that path, including a rename destination |
| `Deleted <path>` | The diff removes a file at that path, including a rename source |
| `Renamed <path>` | The committed diff reports status R with that destination |
| `Renamed <old> to <new>` | The committed diff reports status R with both endpoints |

The allowlist can contain one path or several paths separated by comma and space outside backticks.
An empty diff satisfies an allowlist or an exclusion of an existing file, but cannot confirm an addition, deletion, or rename.
Exclusions of directories, paths absent at both endpoints, and submodule contents are not checkable.
File actions on submodule entries are not checkable.
A bare test count such as `77 tests passed` lacks a command and stays not checkable.

## Repository evidence

Commit hashes contain 7 to 64 hexadecimal characters and may be abbreviated if they identify exactly one local object.
Missing objects contradict local existence, while ambiguous abbreviations stay not checkable.
A directory or submodule is not a file blob.
A symbolic link counts as its stored blob without following the link.
File existence concerns HEAD, not an uncommitted working copy.
Missing HEAD makes file, diff, and default-HEAD ancestry evidence unavailable.
Clean-tree checks include staged, unstaged, and untracked changes, and exclude ignored files.
Configured clean or process filters, assume-unchanged or skip-worktree index flags, and submodule worktrees make cleanliness not checkable.
Status and index visibility checks inspect a private index copy for the whole repository, even when --repo selects a subdirectory.
Changes to the live index identity, size, or timestamps during the check make cleanliness not checkable.
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
A bare pushed-state statement checks HEAD independently of other report statements.
An explicit hash checks only that commit and must identify exactly one local object whose type is commit.
Ambiguous abbreviations and annotated tag objects stay not checkable.
The evidence includes resolved commits and the tracking ref's current local hash.
This confirms only reachability in the local snapshot, not the server's present state, whether a push command ran, or whether a commit was ever pushed elsewhere.
Missing refs, invalid names, and unresolved commits stay not checkable.
Shallow history cannot confirm absence from ancestry.

## Supplied command logs

Repeat `--log 'COMMAND=PATH'` for distinct commands.
The final equals sign is the separator, allowing equals signs in command arguments.
For paths containing equals signs, repeat `--log-file COMMAND PATH` with two separate arguments instead.
For example, `--log-file 'pytest --maxfail=1' 'build=debug/run.log'`.
Both log flags can be combined.
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
Each summary requires a preceding `running N tests` header whose count equals passed plus failed plus ignored plus measured.
A summary without a header, a mismatched total, a duplicate summary, or a suite-start line without a completed summary makes counts not checkable.
Cargo suite announcements match entire raw lines, preserving their whitespace.
An executable announcement has five leading ASCII spaces, `Running `, an optional `unittests ` label, the source path, and a parenthesized executable path separated by one space.
The executable path must contain a forward or backward slash and may use a custom target directory or contain spaces.
A doc-test announcement has three leading ASCII spaces, `Doc-tests `, and a nonempty crate name containing only letters, digits, underscores, or hyphens.
Cargo writes these announcements to stderr, while the test harness writes running-count headers and summaries to stdout.
Merged logs do not preserve stream identity or establish authenticity.
Malformed or whitespace-altered announcement-shaped lines are ambiguous and make counts not checkable.
Ordinary stdout such as `Running initialization` or `Running query (cached)` does not start a suite.
The pytest parser accepts exactly one final count list followed by `in <seconds>s`, optionally surrounded by equals signs or followed by a duration annotation.
It recognizes passed, failed, errors, skipped, deselected, expected-failure, unexpected-pass, and warning categories using their standard summary tokens.
Errors contribute to the failure count.
The JavaScript parser accepts one `Tests` line with pipe-separated categories and a parenthesized total, or one `Tests:` line with comma-separated categories and a final total.
It recognizes passed, failed, skipped, and todo categories and checks their sum against the total.
Suite and file totals do not count as test totals.
Summary candidates are counted before format validation, so a truncated additional summary makes counts not checkable.
Recognized runner failures, including failed suites or compilation failures, contradict a test-success statement even if its passed count matches.
Matching passed counts and the explicitly reported failure count confirm the statement.
An omitted failure count means zero failures.
When a statement reports failures, ordinary test-failure markers alone cannot resolve missing or ambiguous counts, which stay not checkable.
Differing counts contradict the statement.
One rule table in src/logs.rs holds line diagnostics and comparisons of complete counts, with a runner source for each rule.
Diagnostics distinguish abnormal errors, inconsistent evidence, and ordinary failures.
Inconsistent evidence makes the statement not checkable.
An abnormal diagnostic reports an error that the runner's test counts do not contain, so it contradicts the statement whatever counts it reports.
An ordinary failure marker belongs to tests inside the counts, so it contradicts only a statement that reports no failures.
Crashes, compilation errors, internal errors, and unhandled errors cannot be accounted for by a test failure count and contradict the statement.
Vitest's `Errors N error` line and its `Unhandled Errors`, `Source Errors`, and `Failed Suites N` banners are abnormal, including banners inside divider characters.
A failed suite covers a test file that failed to load or a hook failure that skipped its tests, which the Tests count does not include as failures.
Vitest writes those banners to stderr, so a stdout-only capture can lack them; a `Test Files` failed total above the `Tests` failed total still shows a file that failed outside its tests, because every failed test fails its own file, and contradicts the statement.
Vitest's `Type Errors N failed` line counts type tests that are already failed tests in the Tests line, so it is an ordinary failure and matching counts can confirm.
A type-failure count above the failed-test count is inconsistent and not checkable.
Malformed or duplicate type summaries also stay not checkable.
A type error outside any test appears as a failed suite or an unhandled source error instead, and contradicts.
Cargo's `process didn't exit successfully:` cause contradicts independently of counted assertion failures, with or without an `error:` prefix.
Cargo's `could not execute process ... (never executed)` cause, printed when a test binary cannot be launched, also contradicts independently of counted assertion failures, because that target counted nothing.
Ordinary assertion failures exit with status 101 and omit that cause, so matching complete counts can confirm.
Jest suite-loading errors also contradict a statement that accurately reports assertion failure counts.
A validated Jest failed-suite count above the failed-test count proves a suite execution error and contradicts even without its banner.
Malformed or duplicate Jest suite summaries stay not checkable.
Pytest collection-error headings, plain or decorated with underscores, contradict independently of the reported failure count.
Ordinary stdout beginning with FAIL is not a Cargo runner status.
Cargo diagnostics apply only to Cargo commands, pytest diagnostics only to pytest, and JavaScript diagnostics only to Vitest or Jest.
All log evidence describes only what the supplied text contains, not execution, authenticity, repository identity, or freshness.
Truncation between completed suites without a visible next-suite start cannot be detected.

## Command line and output

```text
agent-receipts [--repo PATH] [--report PATH|-]
               [--base COMMIT] [--remote-ref refs/remotes/NAME/BRANCH]
               [--log COMMAND=PATH]... [--log-file COMMAND PATH]... [--json]
agent-receipts --help
agent-receipts --version
```

`--repo` defaults to the current directory and accepts a subdirectory of a working repository.
`--report` defaults to `-` for stdin until EOF.
An interactive terminal without a report fails with an actionable error instead of waiting.
Report file paths resolve from the invocation directory.
Default stdout is a tab-separated verdict table with `LINE`, `STATEMENT`, `CHECK`, `VERDICT`, and `EVIDENCE` columns and a summary.
Cell escaping makes tabs, carriage returns, backslashes, control characters, and Unicode format characters visible.
Errors raised while reading input or evidence use the same escaping.
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
An empty Git protocol allowlist denies all transports, including direct promisor fetches.
Replacement objects, global and system configuration, and inherited configuration overrides are disabled.
Status uses a private Git directory with resolved content and path comparison settings, copied index and metadata, and no filter definitions or configuration includes.
Configured clean or process filters and submodules make cleanliness not checkable.
Later live configuration changes cannot introduce filters into the status subprocess.
Repository-location environment variables are removed so `--repo` chooses the checked repository.
External diff drivers and text conversion are disabled for diff checks.
The command-line parser, fixed grammar, and JSON serializer use established libraries listed in Cargo.toml.
Integration tests create isolated repositories under the system temp directory, with local fixture identity, signing disabled, and hooks disabled.
No model calls, credentials, network requests, or report-command execution are needed.
The tool cannot prove intent, authorship, code quality, report completeness, or evidence authenticity.
It trusts the local Git executable, its configuration parser, and object store as evidence.
These checks do not sandbox Git against malformed repositories or establish authenticity of local objects and refs.
Repository evidence is not an atomic snapshot when another process changes the repository during a check.

## Validation and demo

Each new statement family starts with a failing executable test, and each log parser has focused tests with literal summaries.
Coverage includes matching and mismatched counts, missing evidence, failed runners, incomplete suites, stale local refs, renamed paths, invalid inputs, JSON, CI exits, and subdirectory selection.
Checks run formatting, lint with warnings denied, the full suite, a release build, and the executable demos.
The README records a synthetic report checked against this repository's real history, with captured output.
The [video script](submission/demo-script.md) fits within three minutes.
The [submission draft](submission/DEVPOST.md) describes the build and remaining author actions honestly.
Eligibility, registration, public code publication, a recorded video, tool disclosure, and final submission remain the author's responsibility.
