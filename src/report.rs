use std::sync::LazyLock;

use regex::Regex;

static PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]\s+|[0-9]+[.)]\s+)").unwrap());
static COMMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:committed as (`?[0-9a-f]{7,64}`?)|commit (`?[0-9a-f]{7,64}`?) exists)$")
        .unwrap()
});
static FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^file (.+) exists$").unwrap());

static TESTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^ran `([^`]+)`, ([0-9]+) passed(?:, ([0-9]+) failed)?$").unwrap()
});

static PUSHED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(not pushed|did not push|pushed)(?: (`?[0-9a-f]{7,64}`?))?$").unwrap()
});

static SCOPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(only touched|did not modify) (.+)$").unwrap());

static ACTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(added|created|deleted|renamed) (.+)$").unwrap());

pub enum PathAction {
    Added(String),
    Deleted(String),
    Renamed { from: Option<String>, to: String },
}

pub enum DiffClaim {
    OnlyTouched(Vec<String>),
    DidNotModify(String),
    Action(PathAction),
}

pub enum Claim {
    Diff(DiffClaim),
    Pushed {
        pushed: bool,
        commit: Option<String>,
    },
    Tests {
        command: String,
        passed: u64,
        failed: Option<u64>,
    },
    Commit(String),
    File(String),
    CleanTree,
    Unsupported(String),
}

impl Claim {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Diff(DiffClaim::Action(_)) => "committed path action",
            Self::Diff(_) => "committed diff scope",
            Self::Pushed { .. } => "local tracking ref ancestry",
            Self::Tests { .. } => "test counts in supplied log",
            Self::Commit(_) => "commit exists locally",
            Self::File(_) => "file at HEAD",
            Self::CleanTree => "working tree clean",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

pub struct Statement {
    pub line: usize,
    pub source: String,
    pub claim: Claim,
}

#[derive(Clone, Copy)]
struct Fence {
    marker: char,
    length: usize,
    container_indent: usize,
}

pub fn parse(text: &str) -> Vec<Statement> {
    let mut fence: Option<Fence> = None;
    let mut list_indents = Vec::new();
    text.lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            let source = raw.trim();
            let columns = indentation(raw);
            let indented = columns >= 4;
            // CommonMark 0.31.2 section 4.5: an unclosed fence ends at its
            // container boundary. Blank lines alone do not end a list item.
            if !source.is_empty() && fence.is_some_and(|f| columns < f.container_indent) {
                fence = None;
            }
            let content = PREFIX.replace(source, "");
            let prefix = &source[..source.len() - content.len()];
            let marker_width = prefix.trim_end().len();
            let marker_end = indentation(raw) + marker_width;
            let padding = prefix[marker_width..]
                .chars()
                .fold(marker_end, |column, c| {
                    if c == '\t' {
                        column + 4 - column % 4
                    } else {
                        column + 1
                    }
                })
                - marker_end;
            let list_code = padding >= 5;
            if fence.is_none() && !source.is_empty() {
                while list_indents.last().is_some_and(|&indent| columns < indent) {
                    list_indents.pop();
                }
                if !prefix.is_empty() && !list_code {
                    list_indents.push(marker_end + padding);
                }
            }
            let delimiter = if fence.is_none() {
                content.as_ref()
            } else {
                // CommonMark 0.31.2 section 4.5: closing fences permit
                // ASCII spaces and tabs, not Unicode whitespace.
                raw.trim_start_matches([' ', '\t'])
            };
            let container_indent = fence.map_or(list_indents.last().copied().unwrap_or(0), |f| {
                f.container_indent
            });
            if columns.saturating_sub(container_indent) < 4 && !list_code {
                if let Some((marker, length)) = fence_marker(delimiter) {
                    match fence {
                        None => {
                            fence = Some(Fence {
                                marker,
                                length,
                                container_indent,
                            });
                            // Keep inline-looking tilde openers visible as
                            // unsupported rows while preserving their code context.
                            if marker != '~' || !delimiter[length..].contains("~~") {
                                return None;
                            }
                        }
                        Some(opening)
                            if marker == opening.marker
                                && length >= opening.length
                                && delimiter[length..].trim_matches([' ', '\t']).is_empty() =>
                        {
                            fence = None;
                            return None;
                        }
                        _ => {}
                    }
                }
            }
            if source.is_empty() {
                return None;
            }
            let content = PREFIX.replace(source, "");
            let content = content.strip_suffix('.').unwrap_or(&content);
            let claim = if fence.is_some() || indented || list_code {
                Claim::Unsupported(
                    "quoted code is not an assertion; no evidence check performed".into(),
                )
            } else {
                parse_claim(content)
            };
            Some(Statement {
                line: index + 1,
                source: source.to_owned(),
                claim,
            })
        })
        .collect()
}

fn indentation(line: &str) -> usize {
    line.chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .fold(0, |column, c| {
            if c == '\t' {
                column + 4 - column % 4
            } else {
                column + 1
            }
        })
}

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = line.chars().take_while(|&value| value == marker).count();
    let info = &line[length..];
    // CommonMark 0.31.2 section 4.5 restricts backticks only. Tilde info
    // strings may contain any number of tildes.
    let valid_info = match marker {
        '`' => !info.contains('`'),
        '~' => true,
        _ => unreachable!(),
    };
    (length >= 3 && valid_info).then_some((marker, length))
}

fn argument(value: &str) -> Option<&str> {
    if let Some(quoted) = value.strip_prefix('`') {
        let contents = quoted.strip_suffix('`')?;
        (!contents.contains('`')).then_some(contents)
    } else {
        (!value.contains('`') && !value.contains(char::is_whitespace)).then_some(value)
    }
}

fn rename_arguments(value: &str) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (index, c) in value.char_indices() {
        if c == '`' {
            quoted = !quoted;
        }
        if !quoted
            && value
                .get(index..index + 4)
                .is_some_and(|s| s.eq_ignore_ascii_case(" to "))
        {
            return Some((&value[..index], &value[index + 4..]));
        }
    }
    None
}

fn parse_claim(content: &str) -> Claim {
    if let Some(c) = ACTION.captures(content) {
        let action = match c[1].to_ascii_lowercase().as_str() {
            "added" | "created" => path_argument(&c[2]).map(PathAction::Added),
            "deleted" => path_argument(&c[2]).map(PathAction::Deleted),
            "renamed" => {
                if let Some(to) = path_argument(&c[2]) {
                    Some(PathAction::Renamed { from: None, to })
                } else if let Some((from, to)) = rename_arguments(&c[2]) {
                    path_argument(from)
                        .zip(path_argument(to))
                        .map(|(from, to)| PathAction::Renamed {
                            from: Some(from),
                            to,
                        })
                } else {
                    None
                }
            }
            _ => None,
        };
        return action
            .map(|a| Claim::Diff(DiffClaim::Action(a)))
            .unwrap_or_else(|| {
                Claim::Unsupported("expected literal repository-relative path arguments".into())
            });
    }
    if let Some(c) = SCOPE.captures(content) {
        if c[1].eq_ignore_ascii_case("did not modify") {
            if let Some(path) = path_argument(&c[2]) {
                return Claim::Diff(DiffClaim::DidNotModify(path));
            }
        } else if let Some(paths) = path_list(&c[2]) {
            return Claim::Diff(DiffClaim::OnlyTouched(paths));
        }
        return Claim::Unsupported(
            "expected literal repository-relative paths, separated by comma and space".into(),
        );
    }
    if let Some(c) = PUSHED.captures(content) {
        let commit = match c.get(2) {
            Some(value) => match argument(value.as_str()) {
                Some(hash) => Some(hash.to_ascii_lowercase()),
                None => return Claim::Unsupported("unpaired commit quoting".into()),
            },
            None => None,
        };
        return Claim::Pushed {
            pushed: c[1].eq_ignore_ascii_case("pushed"),
            commit,
        };
    }
    if let Some(c) = TESTS.captures(content) {
        if let Ok(passed) = c[2].parse() {
            let failed = c.get(3).map(|v| v.as_str().parse::<u64>());
            if !matches!(failed, Some(Err(_))) {
                return Claim::Tests {
                    command: c[1].to_owned(),
                    passed,
                    failed: failed.map(Result::unwrap),
                };
            }
        }
    }
    if content.eq_ignore_ascii_case("tree is clean")
        || content.eq_ignore_ascii_case("the tree is clean")
    {
        return Claim::CleanTree;
    }
    if let Some(captures) = COMMIT.captures(content) {
        let value = captures
            .get(1)
            .or_else(|| captures.get(2))
            .unwrap()
            .as_str();
        if let Some(hash) = argument(value) {
            return Claim::Commit(hash.to_ascii_lowercase());
        }
    }
    if let Some(captures) = FILE.captures(content) {
        let value = captures
            .get(1)
            .or_else(|| captures.get(2))
            .unwrap()
            .as_str();
        if let Some(path) = argument(value) {
            if !path.contains([':', '\\'])
                && !path.contains(char::is_control)
                && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
            {
                return Claim::File(path.to_owned());
            }
            return Claim::Unsupported(
                "path must be a normal repository-relative file path".into(),
            );
        }
    }
    Claim::Unsupported("no supported assertion grammar; no evidence check performed".into())
}

fn path_argument(value: &str) -> Option<String> {
    let path = argument(value)?;
    (!path.contains([':', '\\'])
        && !path.contains(char::is_control)
        && path.split('/').all(|part| !matches!(part, "" | "." | "..")))
    .then(|| path.to_owned())
}

fn path_list(value: &str) -> Option<Vec<String>> {
    let mut quoted = false;
    let mut start = 0;
    let mut paths = Vec::new();
    for (index, c) in value.char_indices() {
        if c == '`' {
            quoted = !quoted;
        }
        if c == ',' && !quoted {
            paths.push(path_argument(&value[start..index])?);
            if !value[index..].starts_with(", ") {
                return None;
            }
            start = index + 2;
        }
    }
    paths.push(path_argument(&value[start..])?);
    Some(paths)
}
