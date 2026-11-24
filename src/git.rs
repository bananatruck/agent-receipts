use std::path::PathBuf;
use std::process::{Command, Output};

use crate::report::Claim;

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

pub struct Repository {
    path: PathBuf,
}

impl Repository {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let repository = Self { path };
        let output = repository.git(&["rev-parse", "--is-inside-work-tree"])?;
        if !output.status.success() || output.stdout != b"true\n" {
            return Err(format!(
                "{} is not an accessible working Git repository: {}",
                repository.path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(repository)
    }

    fn git(&self, args: &[&str]) -> Result<Output, String> {
        Command::new("git")
            .args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-C"])
            .arg(&self.path)
            .args(args)
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
        let output = self.git(&[
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ])?;
        let status = successful_text(output)?;
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
