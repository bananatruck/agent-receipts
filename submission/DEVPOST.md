# agent-receipts

## What it does

A final coding report can sound convincing while naming missing files or reporting the wrong test count.
This offline command-line program checks explicit statements against local repository evidence and caller-supplied command logs.
Each statement receives confirmed, contradicted, or not checkable, with the evidence shown.
The table is readable in a terminal, and versioned JSON plus exit codes support CI checks.

It checks commit existence, files at HEAD, working-tree cleanliness, test summaries, reachability from a named local tracking ref, and path changes since a base commit.
It preserves uncertainty when the required evidence is missing.
The README includes a clearly labelled synthetic report checked against this project's real history and shows the captured output.

## How it was built

The project was built by AI coding agents directed by the author.
Work began on November 24, 2026, after the event's stated start date.
Implementation followed failing tests for each statement family and used isolated temporary repositories for integration tests.
The implementation uses Rust, local Git subprocesses, clap for command-line arguments, regex for fixed grammar, serde and serde_json for JSON, and tempfile in tests.
Cargo.toml and Cargo.lock list the dependencies and resolved versions.

Author disclosure: [List the AI coding tools used before submitting.]

## Limits

Supplied logs establish only what their text records, not whether an authentic command ran in this checkout.
Incomplete additional summaries stay not checkable, and matching failure counts cannot account for runner execution errors.
Pytest collection errors also prevent confirmation when the reported failure count matches.
Cargo announcements require their complete padded line shapes, and ordinary output such as `Running query (cached)` does not start a suite.
Malformed, whitespace-altered, unfinished, or conflicting announcement-shaped lines remain not checkable.
A local tracking ref can be stale and cannot establish the server's present state or prove a push happened.
Diff checks compare committed endpoint trees and do not establish everything edited during a session.
The parser accepts documented statement forms rather than arbitrary prose.
Headings and unsupported assertions remain visible as not checkable and prevent a successful CI exit.
Cleanliness stays not checkable for configured filters, hidden index flags, or submodule worktrees.
Git execution precautions do not establish authenticity of a repository or make Git a sandbox.
Log mappings split at the final equals sign; log paths containing equals signs are unsupported.
The program cannot prove code quality, authorship, or completeness.

## Before submission

The source is published at https://github.com/bananatruck/agent-receipts.
The author must confirm student and age eligibility, register, complete the disclosure above, and record and upload a video of at most three minutes.
The intended track is AI, Agentic Systems, Cybersecurity, and Dev Tools.
No video upload, registration, or submission is claimed here.
