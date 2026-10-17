//! Git operations on the host repository.
//!
//! Everything that arrives from the container is untrusted: the agent controls
//! the bundle contents and every file of the merged tree. Therefore every git
//! call here runs with hooks disabled (a repository whose `core.hooksPath`
//! points into the working tree, e.g. husky, would otherwise run agent-written
//! scripts right after a merge), without fsmonitor and submodule recursion,
//! and with object checks on fetch (ARCHITECTURE.md §3.6).

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use super::bundle::BundleHeader;
use crate::paths::{find_on_path, local_data_dir};
use crate::{Error, Result};

/// A git repository on the host.
#[derive(Debug, Clone)]
pub struct HostRepo {
    path: PathBuf,
    git: PathBuf,
    no_hooks: PathBuf,
}

/// Result of [`HostRepo::import_bundle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// The branch was created or moved to the bundle's commit.
    Updated {
        /// Previous commit, `None` if the branch is new.
        from: Option<String>,
        /// New commit.
        to: String,
        /// Commits on the branch that no other local branch has.
        commits: u32,
    },
    /// The branch already pointed at the bundle's commit.
    Unchanged,
    /// The bundle builds on commits the host does not have (e.g. after the
    /// user rewrote history); the container must send a full bundle.
    NeedsFullBundle,
}

/// Result of [`HostRepo::merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Everything on the agent branch is already in the current branch.
    UpToDate,
    /// The current branch was fast-forwarded.
    FastForward {
        /// Commits gained.
        commits: u32,
    },
    /// A merge commit was created.
    Merged {
        /// Commits gained from the agent branch.
        commits: u32,
    },
    /// The merge would conflict; nothing was changed.
    Conflict {
        /// Files that would conflict.
        files: Vec<String>,
    },
}

/// Result of [`HostRepo::incoming`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Incoming {
    /// Changed files.
    pub files: Vec<IncomingFile>,
    /// Whether the patch was longer than allowed and cut off.
    pub truncated: bool,
}

/// A file a merge would change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncomingFile {
    /// Path in the repository, `/`-separated.
    pub path: String,
    /// git's status letter: `A`dded, `M`odified, `D`eleted, `T`ype changed.
    pub status: char,
    /// Mode before, e.g. `100644`; `000000` for new files.
    pub old_mode: String,
    /// Mode after: `100755` executable, `120000` symlink, `160000`
    /// submodule; `000000` for deleted files.
    pub new_mode: String,
    /// Whether git treats the content as binary.
    pub binary: bool,
    /// Added lines with their line number in the new file. For a symlink
    /// the one line is its target.
    pub added: Vec<(u32, String)>,
}

/// Parses `git diff --raw -z --no-renames`.
fn parse_raw_diff(raw: &str) -> Vec<IncomingFile> {
    let mut files = Vec::new();
    let mut parts = raw.split('\0');
    while let Some(header) = parts.next() {
        let Some(header) = header.strip_prefix(':') else {
            continue;
        };
        let Some(path) = parts.next() else {
            break;
        };
        let fields: Vec<&str> = header.split(' ').collect();
        if fields.len() < 5 {
            continue;
        }
        files.push(IncomingFile {
            path: path.to_owned(),
            status: fields[4].chars().next().unwrap_or('M'),
            old_mode: fields[0].to_owned(),
            new_mode: fields[1].to_owned(),
            ..IncomingFile::default()
        });
    }
    files
}

/// Undoes git's C-style quoting of unusual paths (`"a\tb"`).
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_owned();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            continue;
        }
        match chars.next() {
            Some('n') => bytes.push(b'\n'),
            Some('t') => bytes.push(b'\t'),
            Some(d @ '0'..='7') => {
                let mut value = d.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    if let Some(next) = chars.peek().and_then(|c| c.to_digit(8)) {
                        value = value * 8 + next;
                        chars.next();
                    }
                }
                bytes.push(value as u8);
            }
            Some(other) => {
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
            }
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Adds the added lines and binary flags of a `-U0` patch to `files`.
fn apply_patch(files: &mut [IncomingFile], patch: &str) {
    let mut current: Option<usize> = None;
    let mut line_no = 0u32;
    let find = |files: &[IncomingFile], path: &str| files.iter().position(|f| f.path == path);
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            current = None;
        } else if let Some(path) = line.strip_prefix("+++ ") {
            // git appends a tab when the name contains a space.
            let path = unquote(path.trim_end_matches('\t'));
            current = path.strip_prefix("b/").and_then(|p| find(files, p));
        } else if let Some(rest) = line.strip_prefix("Binary files ") {
            // "Binary files a/x and b/y differ" (or /dev/null on one side).
            if let Some(path) = rest
                .rsplit_once(" and ")
                .and_then(|(_, new)| new.strip_suffix(" differ"))
                .map(unquote)
                .and_then(|p| p.strip_prefix("b/").map(str::to_owned))
                && let Some(index) = find(files, &path)
            {
                files[index].binary = true;
            }
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            // "@@ -a,b +c,d @@": the new file's lines start at c.
            line_no = hunk
                .split(' ')
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse().ok())
                .unwrap_or(0);
        } else if let (Some(index), Some(text)) = (current, line.strip_prefix('+')) {
            files[index].added.push((line_no, text.to_owned()));
            line_no += 1;
        }
    }
}

impl HostRepo {
    /// Opens the repository at `path` using git from `HARNESS_GIT` or `PATH`.
    pub fn open(path: &Path) -> Result<Self> {
        let git = find_git()?;
        let no_hooks = local_data_dir()?.join("git-no-hooks");
        fs::create_dir_all(&no_hooks)
            .map_err(|e| Error::io(format!("{} anlegen", no_hooks.display()), e))?;
        let repo = HostRepo {
            path: path.to_owned(),
            git,
            no_hooks,
        };
        if !repo.output(&["rev-parse", "--git-dir"])?.status.success() {
            return Err(Error::Sync(format!(
                "{} ist kein Git-Repository",
                path.display()
            )));
        }
        Ok(repo)
    }

    /// Root of the repository.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes all local branches, tags and `HEAD` into a bundle for the
    /// container. Remote-tracking refs and the stash stay on the host.
    pub fn export_bundle(&self, target: &Path) -> Result<()> {
        if self.rev_parse("HEAD")?.is_none() {
            return Err(Error::Sync(format!(
                "{} hat noch keine Commits. Der Agent braucht mindestens einen Commit als Ausgangspunkt",
                self.path.display()
            )));
        }
        let target = target.to_string_lossy();
        self.run(
            "bundle create",
            &[
                "bundle",
                "create",
                "--quiet",
                &target,
                "HEAD",
                "--branches",
                "--tags",
            ],
        )?;
        Ok(())
    }

    /// Moves `refs/heads/<branch>` to the commit in the bundle.
    ///
    /// Only fast-forwards unless `force` is set, and never while the branch is
    /// checked out, because the working tree would silently go stale. The
    /// update is a compare-and-swap against the value read before the fetch.
    pub fn import_bundle(&self, bundle: &Path, branch: &str, force: bool) -> Result<ImportOutcome> {
        let header = BundleHeader::read(bundle)?;
        if !self.missing_objects(&header.prerequisites)?.is_empty() {
            return Ok(ImportOutcome::NeedsFullBundle);
        }
        let new = header
            .branch(branch)
            .ok_or_else(|| Error::Sync(format!("Das Bundle enthält den Branch {branch} nicht")))?
            .to_owned();
        let reference = format!("refs/heads/{branch}");
        let old = self.rev_parse(&reference)?;
        if old.as_deref() == Some(new.as_str()) {
            return Ok(ImportOutcome::Unchanged);
        }
        if self.checked_out(&reference)? {
            return Err(Error::Sync(format!(
                "{branch} ist auf dem Host ausgecheckt. Bitte erst auf einen anderen Branch wechseln"
            )));
        }

        // Fetch objects only; the ref is updated below after the checks.
        let bundle_path = bundle.to_string_lossy();
        self.run(
            "fetch",
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--no-write-fetch-head",
                &bundle_path,
                &reference,
            ],
        )?;
        if let Some(old) = &old
            && !force
            && !self.is_ancestor(old, &new)?
        {
            return Err(Error::Sync(format!(
                "{branch} wurde im Container umgeschrieben (kein Fast-Forward von {} auf {}). \
                 `harness sync --force` übernimmt den Stand des Containers",
                short(old),
                short(&new)
            )));
        }
        self.run(
            "update-ref",
            &[
                "update-ref",
                "-m",
                "harness sync",
                &reference,
                &new,
                old.as_deref().unwrap_or(""),
            ],
        )?;
        // With --branches, --exclude takes the name without refs/heads/.
        let others = format!("--exclude={branch}");
        let commits = self.count(&[&reference, "--not", &others, "--branches"])?;
        Ok(ImportOutcome::Updated {
            from: old,
            to: new,
            commits,
        })
    }

    /// Merges `branch` into the current branch.
    ///
    /// Refuses (with [`Error::Sync`]) on a detached HEAD, a merge or rebase in
    /// progress, or uncommitted changes to tracked files; the user's work is
    /// never touched. Conflicts are detected with a dry run (`merge-tree`)
    /// first, so a conflicting merge leaves the repository as it was.
    pub fn merge(&self, branch: &str) -> Result<MergeOutcome> {
        let current = self
            .run(
                "symbolic-ref",
                &["symbolic-ref", "--quiet", "--short", "HEAD"],
            )
            .map_err(|_| {
                Error::Sync(
                    "HEAD ist kein Branch (detached). Bitte einen Branch auschecken".to_owned(),
                )
            })?
            .trim()
            .to_owned();
        if current == branch {
            return Err(Error::Sync(format!(
                "{branch} ist selbst ausgecheckt. Bitte den Ziel-Branch auschecken"
            )));
        }
        let reference = format!("refs/heads/{branch}");
        if self.rev_parse(&reference)?.is_none() {
            return Err(Error::Sync(format!(
                "{branch} existiert noch nicht. Erst `harness sync` ausführen"
            )));
        }
        for pending in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
            if self.rev_parse(pending)?.is_some() {
                return Err(Error::Sync(format!(
                    "Im Repository läuft noch eine Operation ({pending}). Bitte erst abschließen"
                )));
            }
        }
        for dir in ["rebase-merge", "rebase-apply"] {
            let path = self.run("rev-parse", &["rev-parse", "--git-path", dir])?;
            if self.path.join(path.trim()).exists() {
                return Err(Error::Sync(
                    "Im Repository läuft noch ein Rebase. Bitte erst abschließen".to_owned(),
                ));
            }
        }
        let dirty = self.run("status", &["status", "--porcelain", "--untracked-files=no"])?;
        if !dirty.trim().is_empty() {
            return Err(Error::Sync(format!(
                "Uncommittete Änderungen an {} Datei(en). Bitte erst committen oder stashen, \
                 die Harness fasst sie nicht an",
                dirty.lines().count()
            )));
        }

        if self.is_ancestor(&reference, "HEAD")? {
            return Ok(MergeOutcome::UpToDate);
        }
        let commits = self.count(&[&format!("HEAD..{reference}")])?;
        if self.is_ancestor("HEAD", &reference)? {
            self.run("merge", &["merge", "--quiet", "--ff-only", &reference])?;
            return Ok(MergeOutcome::FastForward { commits });
        }

        let dry_run = self.output(&[
            "merge-tree",
            "--write-tree",
            "--name-only",
            "--no-messages",
            "HEAD",
            &reference,
        ])?;
        match dry_run.status.code() {
            Some(0) => {
                self.run("merge", &["merge", "--quiet", "--no-edit", branch])?;
                Ok(MergeOutcome::Merged { commits })
            }
            // Exit code 1: conflicts. Line 1 is the tree id, then file names.
            Some(1) => Ok(MergeOutcome::Conflict {
                files: String::from_utf8_lossy(&dry_run.stdout)
                    .lines()
                    .skip(1)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect(),
            }),
            _ => Err(git_error("merge-tree", &dry_run)),
        }
    }

    /// Whether the local branch exists.
    pub fn branch_exists(&self, branch: &str) -> Result<bool> {
        Ok(self.rev_parse(&format!("refs/heads/{branch}"))?.is_some())
    }

    /// What merging `branch` would bring in: everything changed on it since
    /// its merge base with HEAD, with the added lines of text files. At most
    /// `max_patch_bytes` of patch are read. Diff drivers and textconv stay
    /// off, so a `.gitattributes` from the agent cannot choose programs to
    /// run here.
    pub fn incoming(&self, branch: &str, max_patch_bytes: usize) -> Result<Incoming> {
        let range = format!("HEAD...refs/heads/{branch}");
        let common = [
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
        ];
        let mut raw_args = common.to_vec();
        raw_args.extend(["--raw", "-z", "--abbrev=40", &range]);
        let raw = self.run("diff --raw", &raw_args)?;
        let mut files = parse_raw_diff(&raw);

        let mut patch_args = common.to_vec();
        patch_args.extend(["-U0", &range]);
        let mut child = self
            .command()
            .args(&patch_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| Error::Spawn {
                program: self.git.clone(),
                source,
            })?;
        let mut patch = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            let _ = stdout
                .take(max_patch_bytes as u64 + 1)
                .read_to_end(&mut patch);
        }
        let truncated = patch.len() > max_patch_bytes;
        if truncated {
            let _ = child.kill();
            patch.truncate(max_patch_bytes);
        }
        let _ = child.wait();
        apply_patch(&mut files, &String::from_utf8_lossy(&patch));
        Ok(Incoming { files, truncated })
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.git);
        cmd.arg("-C")
            .arg(&self.path)
            .arg("-c")
            .arg(format!("core.hooksPath={}", self.no_hooks.display()))
            .args([
                "-c",
                "core.fsmonitor=false",
                "-c",
                "submodule.recurse=false",
                "-c",
                "fetch.fsckObjects=true",
                "-c",
                "transfer.fsckObjects=true",
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            // These would redirect git to another repository or index.
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .stdin(Stdio::null());
        cmd
    }

    fn output(&self, args: &[&str]) -> Result<Output> {
        self.command()
            .args(args)
            .output()
            .map_err(|source| Error::Spawn {
                program: self.git.clone(),
                source,
            })
    }

    /// Runs git and returns stdout; a non-zero exit is an error.
    fn run(&self, action: &str, args: &[&str]) -> Result<String> {
        let output = self.output(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(git_error(action, &output))
        }
    }

    fn rev_parse(&self, rev: &str) -> Result<Option<String>> {
        let spec = format!("{rev}^{{commit}}");
        let output = self.output(&["rev-parse", "--verify", "--quiet", &spec])?;
        match output.status.code() {
            Some(0) => Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            )),
            Some(1) => Ok(None),
            _ => Err(git_error("rev-parse", &output)),
        }
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let output = self.output(&["merge-base", "--is-ancestor", ancestor, descendant])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(git_error("merge-base", &output)),
        }
    }

    fn count(&self, revs: &[&str]) -> Result<u32> {
        let mut args = vec!["rev-list", "--count"];
        args.extend_from_slice(revs);
        let text = self.run("rev-list", &args)?;
        text.trim()
            .parse()
            .map_err(|_| Error::Sync(format!("Unerwartete Ausgabe von git rev-list: {text}")))
    }

    fn checked_out(&self, reference: &str) -> Result<bool> {
        let list = self.run("worktree list", &["worktree", "list", "--porcelain"])?;
        let wanted = format!("branch {reference}");
        Ok(list.lines().any(|line| line == wanted))
    }

    /// Object ids from `oids` that the repository does not have.
    fn missing_objects(&self, oids: &[String]) -> Result<Vec<String>> {
        let mut missing = Vec::new();
        for oid in oids {
            let spec = format!("{oid}^{{commit}}");
            if !self.output(&["cat-file", "-e", &spec])?.status.success() {
                missing.push(oid.clone());
            }
        }
        Ok(missing)
    }
}

/// Locates `git`: `HARNESS_GIT`, then `PATH`.
pub fn find_git() -> Result<PathBuf> {
    if let Some(path) = env::var_os("HARNESS_GIT") {
        return Ok(PathBuf::from(path));
    }
    let exe = if cfg!(windows) { "git.exe" } else { "git" };
    find_on_path(exe).ok_or_else(|| {
        Error::Unsupported("git nicht gefunden. Pfad über HARNESS_GIT setzen".to_owned())
    })
}

fn git_error(action: &str, output: &Output) -> Error {
    Error::Git {
        action: action.to_owned(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(10)]
}
