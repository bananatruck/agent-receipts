use std::sync::LazyLock;

use regex::Regex;

static PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]\s+|[0-9]+[.)]\s+)").unwrap());
static COMMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:committed as (`?[0-9a-f]{7,64}`?)|commit (`?[0-9a-f]{7,64}`?) exists)$")
        .unwrap()
});
static FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:file (.+) exists|added (.+))$").unwrap());

pub enum Claim {
    Commit(String),
    File(String),
    CleanTree,
    Unsupported(String),
}

impl Claim {
    pub fn label(&self) -> &'static str {
        match self {
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

pub fn parse(text: &str) -> Vec<Statement> {
    let mut fence = None;
    text.lines()
        .enumerate()
        .filter_map(|(index, raw)| {
            let source = raw.trim();
            if let Some((marker, length)) = fence_marker(source) {
                match fence {
                    None => {
                        fence = Some((marker, length));
                        return None;
                    }
                    Some((opening, minimum))
                        if marker == opening
                            && length >= minimum
                            && source[length..].trim().is_empty() =>
                    {
                        fence = None;
                        return None;
                    }
                    _ => {}
                }
            }
            if source.is_empty() || (fence.is_none() && is_heading(source)) {
                return None;
            }
            let content = PREFIX.replace(source, "");
            let content = content.strip_suffix('.').unwrap_or(&content);
            let claim = if fence.is_some() || raw.starts_with("    ") || raw.starts_with('\t') {
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

fn fence_marker(line: &str) -> Option<(char, usize)> {
    let marker = line.chars().next()?;
    if !matches!(marker, '`' | '~') {
        return None;
    }
    let length = line.chars().take_while(|&value| value == marker).count();
    (length >= 3).then_some((marker, length))
}

fn is_heading(line: &str) -> bool {
    let hashes = line
        .chars()
        .take_while(|&character| character == '#')
        .count();
    (1..=6).contains(&hashes)
        && (line.len() == hashes || line[hashes..].starts_with(char::is_whitespace))
}

fn argument(value: &str) -> Option<&str> {
    if let Some(quoted) = value.strip_prefix('`') {
        let contents = quoted.strip_suffix('`')?;
        (!contents.contains('`')).then_some(contents)
    } else {
        (!value.contains('`') && !value.contains(char::is_whitespace)).then_some(value)
    }
}

fn parse_claim(content: &str) -> Claim {
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
