use std::path::PathBuf;
use std::process::{Command, Output};

use crate::report::{Claim, DiffClaim, PathAction};

pub enum Verdict {
    Confirmed,
    Contradicted,
    NotCheckable,
}

impl Verdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Contradicted => "contradicted",
            Self::NotCheckable => "not checkable",
        }
    }
}

pub struct Finding {
    pub verdict: Verdict,
    pub evidence: String,
}

struct Change {
    status: String,
    paths: Vec<String>,
}

pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let mut repository = Self { path };
        let output = repository.git(&["rev-parse", "--is-inside-work-tree"])?;
        if !output.status.success() || output.stdout != b"true\n" {
            return Err(format!(
                "{} is not an accessible working Git repository: {}",
                repository.path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let root = successful_text(repository.git(&["rev-parse", "--show-toplevel"])?)?;
        repository.path = PathBuf::from(root.strip_suffix('\n').ok_or("missing repository root")?);
        Ok(repository)
    }

    fn git(&self, args: &[&str]) -> Result<Output, String> {
        Command::new("git")
            .args([
                "--no-optional-locks",
                "--no-replace-objects",
                "--literal-pathspecs",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-C",
            ])
            .arg(&self.path)
            .args(args)
            // git(1): an empty allowlist denies every transport, including
            // direct promisor prefetches that bypass the lazy-fetch guard.
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_COUNT", "0")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env_remove("GIT_CONFIG")
            .env_remove("GIT_REPLACE_REF_BASE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_NAMESPACE")
            .output()
            .map_err(|error| format!("cannot run git: {error}; install Git and check --repo"))
    }

    pub fn verify(&self, claim: &Claim) -> Finding {
        let result = match claim {
            Claim::Commit(hash) => self.commit(hash),
            Claim::File(path) => self.file(path),
            Claim::CleanTree => self.clean_tree(),
            Claim::Diff(_) => Err("no base commit evidence".into()),
            Claim::Pushed { .. } => Err("no tracking ref evidence".into()),
            Claim::Tests { .. } => Err("no supplied log".into()),
            Claim::Unsupported(reason) => Ok(Finding {
                verdict: Verdict::NotCheckable,
                evidence: reason.to_owned(),
            }),
        };
        result.unwrap_or_else(|evidence| Finding {
            verdict: Verdict::NotCheckable,
            evidence,
        })
    }

    fn changes(&self, base: Option<&str>) -> Result<(String, String, Vec<Change>), String> {
        let base = base.ok_or("supply --base COMMIT to inspect the committed diff through HEAD")?;
        let base = successful_text(self.git(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ])?)?
        .trim()
        .to_owned();
        let head = successful_text(self.git(&["rev-parse", "--verify", "HEAD^{commit}"])?)?
            .trim()
            .to_owned();
        let text = successful_text(self.git(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-relative",
            "--ignore-submodules=none",
            "--name-status",
            "-z",
            "--find-renames=50%",
            // git-diff(1): zero disables the exhaustive rename limit.
            "-l0",
            &base,
            &head,
            "--",
        ])?)?;
        let mut fields = text.split_terminator('\0');
        let mut changes = Vec::new();
        while let Some(status) = fields.next() {
            let count = if status.starts_with(['R', 'C']) { 2 } else { 1 };
            let paths: Vec<_> = (0..count)
                .map(|_| {
                    fields
                        .next()
                        .map(str::to_owned)
                        .ok_or("incomplete diff record")
                })
                .collect::<Result<_, _>>()?;
            changes.push(Change {
                status: status.to_owned(),
                paths,
            });
        }
        Ok((base, head, changes))
    }

    fn entry_mode(&self, revision: &str, path: &str) -> Result<Option<String>, String> {
        let text = successful_text(self.git(&[
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "--full-tree",
            revision,
            "--",
            path,
        ])?)?;
        Ok(text.split_once(' ').map(|(mode, _)| mode.to_owned()))
    }

    fn blob_entry(&self, revision: &str, path: &str) -> Result<bool, String> {
        Ok(matches!(
            self.entry_mode(revision, path)?.as_deref(),
            Some("100644" | "100755" | "120000")
        ))
    }

    pub fn verify_diff(&self, claim: &DiffClaim, base: Option<&str>) -> Finding {
        let result = self.changes(base).and_then(|(base, head, changes)| {
            match claim {
                DiffClaim::DidNotModify(path) => {
                    if !self.blob_entry(&base, path)? && !self.blob_entry(&head, path)? {
                        return Err("exclusion requires a literal file present at a diff endpoint; directories, missing paths and submodule contents are not checkable".into());
                    }
                }
                DiffClaim::Action(action) => {
                    let paths = match action {
                        PathAction::Added(path) | PathAction::Deleted(path) => vec![path],
                        PathAction::Renamed { from, to } => from.iter().chain(std::iter::once(to)).collect(),
                    };
                    for path in paths {
                        if self.entry_mode(&base, path)?.as_deref() == Some("160000")
                            || self.entry_mode(&head, path)?.as_deref() == Some("160000") {
                            return Err("file actions do not check submodule entries".into());
                        }
                    }
                }
                _ => {}
            }
            let paths: Vec<_> = changes.iter().flat_map(|change| change.paths.iter()).collect();
            let matches = match claim {
                DiffClaim::OnlyTouched(allowed) => paths.iter().all(|p| allowed.contains(p)),
                DiffClaim::DidNotModify(path) => !paths.contains(&path),
                DiffClaim::Action(action) => changes.iter().any(|change| match action {
                    PathAction::Added(path) => (change.status == "A" || change.status.starts_with('R')) && change.paths.last() == Some(path),
                    PathAction::Deleted(path) => (change.status == "D" || change.status.starts_with('R')) && change.paths.first() == Some(path),
                    PathAction::Renamed { from, to } => change.status.starts_with('R') && change.paths.last() == Some(to) && from.as_ref().is_none_or(|from| change.paths.first() == Some(from)),
                }),
            };
            Ok(Finding {
                verdict: if matches { Verdict::Confirmed } else { Verdict::Contradicted },
                evidence: format!("committed diff {base}..{head}: {}; excludes working-tree changes and intermediate edits", changes.iter().map(|c| format!("{} {}", c.status, c.paths.join(" -> "))).collect::<Vec<_>>().join("; ")),
            })
        });
        result.unwrap_or_else(|evidence| Finding {
            verdict: Verdict::NotCheckable,
            evidence,
        })
    }

    pub fn pushed(&self, pushed: bool, commit: Option<&str>, remote: Option<&str>) -> Finding {
        let result = self.pushed_evidence(pushed, commit, remote);
        result.unwrap_or_else(|evidence| Finding {
            verdict: Verdict::NotCheckable,
            evidence,
        })
    }

    fn pushed_evidence(
        &self,
        pushed: bool,
        commit: Option<&str>,
        remote: Option<&str>,
    ) -> Result<Finding, String> {
        let remote =
            remote.ok_or("supply --remote-ref refs/remotes/NAME/BRANCH; no network is used")?;
        if !remote.starts_with("refs/remotes/")
            || !self.git(&["check-ref-format", remote])?.status.success()
        {
            return Err("--remote-ref must name a full local refs/remotes/ ref".into());
        }
        let target = successful_text(self.git(&["show-ref", "--verify", "--hash", remote])?)?;
        let target = target.trim();
        successful_text(self.git(&["cat-file", "-e", &format!("{target}^{{commit}}")])?)?;
        let resolved = if let Some(hash) = commit {
            let objects =
                successful_text(self.git(&["rev-parse", &format!("--disambiguate={hash}")])?)?;
            let objects: Vec<_> = objects.lines().collect();
            let [object] = objects.as_slice() else {
                return Err("explicit hash must identify exactly one local commit object".into());
            };
            if successful_text(self.git(&["cat-file", "-t", object])?)?.trim() != "commit" {
                return Err("explicit hash does not identify a commit object".into());
            }
            (*object).to_owned()
        } else {
            successful_text(self.git(&["rev-parse", "--verify", "HEAD^{commit}"])?)?
                .trim()
                .to_owned()
        };
        let resolved = resolved.as_str();
        let mut evidence = Vec::new();
        let mut matches = true;
        {
            let output = self.git(&["merge-base", "--is-ancestor", resolved, target])?;
            let present = match output.status.code() {
                Some(0) => true,
                Some(1) => {
                    if successful_text(self.git(&["rev-parse", "--is-shallow-repository"])?)?.trim()
                        == "true"
                    {
                        return Err(
                            "shallow history cannot establish absence from tracking-ref ancestry"
                                .into(),
                        );
                    }
                    false
                }
                _ => {
                    return Err(format!(
                        "cannot compare ancestry: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
            };
            matches &= present == pushed;
            evidence.push(format!(
                "{resolved} {} reachable",
                if present { "is" } else { "is not" }
            ));
        }
        Ok(Finding {
            verdict: if matches {
                Verdict::Confirmed
            } else {
                Verdict::Contradicted
            },
            evidence: format!(
                "{} from {remote} at {target}; local snapshot only, not current server state or push history",
                evidence.join("; ")
            ),
        })
    }

    fn commit(&self, hash: &str) -> Result<Finding, String> {
        let output = self.git(&["rev-parse", &format!("--disambiguate={hash}")])?;
        let text = successful_text(output)?;
        let objects: Vec<_> = text.lines().collect();
        match objects.as_slice() {
            [] => Ok(Finding {
                verdict: Verdict::Contradicted,
                evidence: format!("no local object matches {hash}"),
            }),
            [object] => {
                let kind = successful_text(self.git(&["cat-file", "-t", object])?)?;
                Ok(Finding {
                    verdict: if kind.trim() == "commit" {
                        Verdict::Confirmed
                    } else {
                        Verdict::Contradicted
                    },
                    evidence: format!("local object {object} has type {}", kind.trim()),
                })
            }
            _ => Ok(Finding {
                verdict: Verdict::NotCheckable,
                evidence: format!("{hash} is ambiguous across {} local objects", objects.len()),
            }),
        }
    }

    fn file(&self, path: &str) -> Result<Finding, String> {
        let head = self.git(&["rev-parse", "--verify", "HEAD^{commit}"])?;
        if !head.status.success() {
            return Err(
                "HEAD does not resolve to a readable commit; file presence is unknown".into(),
            );
        }
        let object = format!("HEAD:{path}");
        let output = self.git(&["cat-file", "-t", &object])?;
        if output.status.success() {
            let kind = String::from_utf8_lossy(&output.stdout);
            Ok(Finding {
                verdict: if kind.trim() == "blob" {
                    Verdict::Confirmed
                } else {
                    Verdict::Contradicted
                },
                evidence: format!("{object} has type {}; checks presence only", kind.trim()),
            })
        } else {
            // ls-tree distinguishes an absent entry from unreadable object data.
            let listing = self.git(&["ls-tree", "-z", "--full-tree", "HEAD", "--", path])?;
            let text = successful_text(listing)?;
            if text.is_empty() {
                Ok(Finding {
                    verdict: Verdict::Contradicted,
                    evidence: format!("no file entry at {object}"),
                })
            } else {
                Err(format!("cannot read object at {object}"))
            }
        }
    }

    fn clean_tree(&self) -> Result<Finding, String> {
        let filters = self.git(&[
            "config",
            "--name-only",
            "--get-regexp",
            r"^filter\..*\.(clean|process)$",
        ])?;
        match filters.status.code() {
            Some(1) => {}
            Some(0) => {
                return Err(
                    "configured clean or process filters prevent a safe working-tree check".into(),
                );
            }
            _ => return Err("cannot inspect repository filter configuration".into()),
        }
        self.check_index_visibility()?;

        let output = self.git(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])?;
        let status = successful_text(output)?;
        // Status can ignore entries whose flags changed after preflight.
        // Recheck before trusting it; repository reads are not atomic.
        self.check_index_visibility()?;
        Ok(if status.is_empty() {
            Finding {
                verdict: Verdict::Confirmed,
                evidence: "git status --porcelain=v1 --untracked-files=all --ignore-submodules=none returned no changes; ignored files excluded".into(),
            }
        } else {
            Finding {
                verdict: Verdict::Contradicted,
                evidence: format!("git status reports changes: {}", status.trim_end()),
            }
        })
    }

    fn check_index_visibility(&self) -> Result<(), String> {
        let stages = successful_text(self.git(&["ls-files", "--stage", "-z"])?)?;
        if stages
            .split_terminator('\0')
            .any(|entry| entry.starts_with("160000 "))
        {
            return Err("working-tree cleanliness inside submodules is not checkable".into());
        }
        let entries = successful_text(self.git(&["ls-files", "-v", "-z"])?)?;
        if entries.split_terminator('\0').any(|entry| {
            entry
                .as_bytes()
                .first()
                .is_some_and(|tag| tag.is_ascii_lowercase() || *tag == b'S')
        }) {
            return Err("index has assume-unchanged or skip-worktree entries; working-tree cleanliness is not checkable".into());
        }

        Ok(())
    }
}

fn successful_text(output: Output) -> Result<String, String> {
    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|error| format!("invalid Git output: {error}"))
    } else {
        Err(format!(
            "Git evidence unavailable: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
