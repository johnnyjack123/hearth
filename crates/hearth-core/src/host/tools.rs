//! The file tools of the direct mode, run inside hearth without a shell:
//! read, write, edit, list, find and grep. Paths here are already checked
//! by the policy; the texts go to the model, so they are English.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use super::ToolError;
use super::policy::Policy;

/// Lines `read` returns at most.
pub const READ_LINES: usize = 2_000;
/// Bytes `read` returns at most.
pub const READ_BYTES: usize = 50_000;
/// Characters of one line `read` and `grep` show.
const LINE_CHARS: usize = 2_000;
/// Largest file `write` and `edit` produce.
pub const WRITE_BYTES: usize = 2_000_000;
/// Largest file `edit` and `grep` read.
const SCAN_BYTES: u64 = 4_000_000;
/// Entries `list` returns at most.
const LIST_ENTRIES: usize = 500;
/// Paths `find` returns at most.
const FIND_RESULTS: usize = 1_000;
/// Matching lines `grep` returns at most.
const GREP_MATCHES: usize = 200;

/// Whether bytes look like a binary file: a NUL in the first 8 KB.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8_192).any(|&byte| byte == 0)
}

fn shorten_line(line: &str) -> String {
    if line.chars().count() <= LINE_CHARS {
        return line.to_owned();
    }
    let cut: String = line.chars().take(LINE_CHARS).collect();
    format!("{cut} [… line cut]")
}

/// Reads a text file from line `offset` (1-based) on, at most `limit` lines.
///
/// # Errors
///
/// Missing file, folder, unreadable file.
pub fn read(
    path: &Path,
    shown: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, ToolError> {
    let meta = fs::metadata(path).map_err(|error| ToolError::io(shown, &error))?;
    if meta.is_dir() {
        return Err(ToolError::new(format!("{shown} is a folder; use list.")));
    }
    let bytes = fs::read(path).map_err(|error| ToolError::io(shown, &error))?;
    if is_binary(&bytes) {
        return Ok(format!(
            "{shown} is a binary file ({} bytes); it is not shown.",
            bytes.len()
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return Ok(format!("{shown} is empty."));
    }
    let start = offset.unwrap_or(1).max(1);
    if start > total {
        return Err(ToolError::new(format!(
            "{shown} has only {total} lines; offset {start} is past the end."
        )));
    }
    let limit = limit.unwrap_or(READ_LINES).clamp(1, READ_LINES);
    let mut out = String::new();
    let mut last = start - 1;
    for line in lines.iter().skip(start - 1).take(limit) {
        let line = shorten_line(line);
        if !out.is_empty() && out.len() + line.len() > READ_BYTES {
            break;
        }
        out.push_str(&line);
        out.push('\n');
        last += 1;
    }
    if last < total {
        let _ = write!(
            out,
            "\n[Showing lines {start}-{last} of {total}. Use offset={} to continue.]",
            last + 1
        );
    }
    Ok(out)
}

/// Creates or replaces a file, with missing folders.
///
/// # Errors
///
/// Too large, or the disk refuses.
pub fn write(path: &Path, shown: &str, content: &str) -> Result<String, ToolError> {
    if content.len() > WRITE_BYTES {
        return Err(ToolError::new(format!(
            "The content is larger than {WRITE_BYTES} bytes."
        )));
    }
    if path.is_dir() {
        return Err(ToolError::new(format!("{shown} is a folder.")));
    }
    let existed = path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| ToolError::io(shown, &error))?;
    }
    fs::write(path, content).map_err(|error| ToolError::io(shown, &error))?;
    let lines = content.lines().count();
    Ok(format!(
        "{} {shown} ({lines} lines, {} bytes).",
        if existed { "Replaced" } else { "Created" },
        content.len()
    ))
}

/// One replacement of [`edit`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Replacement {
    /// Text that must occur exactly once in the file.
    #[serde(rename = "oldText")]
    pub old_text: String,
    /// What replaces it.
    #[serde(rename = "newText")]
    pub new_text: String,
}

/// Replaces exact passages of a text file. Every `old_text` must occur
/// exactly once in the file as it was, and they must not overlap. A file
/// with CRLF line ends matches passages written with LF and keeps CRLF.
///
/// # Errors
///
/// A passage missing or ambiguous, overlaps, a binary or too large file.
pub fn edit(path: &Path, shown: &str, edits: &[Replacement]) -> Result<String, ToolError> {
    if edits.is_empty() {
        return Err(ToolError::new("No edits given."));
    }
    let meta = fs::metadata(path).map_err(|error| ToolError::io(shown, &error))?;
    if meta.len() > SCAN_BYTES {
        return Err(ToolError::new(format!(
            "{shown} is too large to edit ({} bytes).",
            meta.len()
        )));
    }
    let bytes = fs::read(path).map_err(|error| ToolError::io(shown, &error))?;
    if is_binary(&bytes) {
        return Err(ToolError::new(format!("{shown} is a binary file.")));
    }
    let original = String::from_utf8(bytes)
        .map_err(|_| ToolError::new(format!("{shown} is not UTF-8 text.")))?;
    let crlf = original.contains("\r\n");
    let text = if crlf {
        original.replace("\r\n", "\n")
    } else {
        original.clone()
    };

    let mut spans: Vec<(usize, usize, &str)> = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        let old = edit.old_text.replace("\r\n", "\n");
        if old.is_empty() {
            return Err(ToolError::new(format!(
                "Edit {}: oldText is empty.",
                index + 1
            )));
        }
        let mut found = text.match_indices(&old);
        let Some((at, _)) = found.next() else {
            return Err(ToolError::new(format!(
                "Edit {}: oldText was not found in {shown}. It must match the file exactly, including spaces and indentation; read the file again.",
                index + 1
            )));
        };
        if found.next().is_some() {
            return Err(ToolError::new(format!(
                "Edit {}: oldText occurs more than once in {shown}; add surrounding lines so it is unique.",
                index + 1
            )));
        }
        spans.push((at, at + old.len(), edit.new_text.as_str()));
    }
    spans.sort_by_key(|span| span.0);
    if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(ToolError::new("Two edits overlap; combine them into one."));
    }

    let mut result = String::with_capacity(text.len());
    let mut summary = String::new();
    let mut cursor = 0;
    for (start, end, new) in &spans {
        result.push_str(&text[cursor..*start]);
        let line = text[..*start].matches('\n').count() + 1;
        let new = new.replace("\r\n", "\n");
        let _ = writeln!(
            summary,
            "line {line}: -{} +{} lines",
            text[*start..*end].lines().count().max(1),
            new.lines().count()
        );
        result.push_str(&new);
        cursor = *end;
    }
    result.push_str(&text[cursor..]);
    let result = if crlf {
        result.replace('\n', "\r\n")
    } else {
        result
    };
    if result.len() > WRITE_BYTES {
        return Err(ToolError::new(format!(
            "The result would be larger than {WRITE_BYTES} bytes."
        )));
    }
    fs::write(path, &result).map_err(|error| ToolError::io(shown, &error))?;
    Ok(format!("Edited {shown}:\n{summary}"))
}

/// Lists a folder: folders with `/`, files with their size.
///
/// # Errors
///
/// Missing or unreadable folder.
pub fn list(path: &Path, shown: &str) -> Result<String, ToolError> {
    let entries = fs::read_dir(path).map_err(|error| ToolError::io(shown, &error))?;
    let mut rows: Vec<(bool, String, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            rows.push((true, name.to_lowercase(), format!("{name}/")));
        } else if kind.is_symlink() {
            rows.push((false, name.to_lowercase(), format!("{name} (link)")));
        } else {
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            rows.push((false, name.to_lowercase(), format!("{name}  {size} B")));
        }
    }
    if rows.is_empty() {
        return Ok(format!("{shown} is empty."));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let total = rows.len();
    let mut out: String = rows
        .iter()
        .take(LIST_ENTRIES)
        .map(|row| format!("{}\n", row.2))
        .collect();
    if total > LIST_ENTRIES {
        let _ = write!(out, "[… {} more entries]", total - LIST_ENTRIES);
    }
    Ok(out)
}

/// Walks files below `dir` as git would see them: `.gitignore` and
/// `.git/info/exclude` apply (also without a repository), `.git` itself is
/// skipped, links are not followed.
fn walk(dir: &Path) -> impl Iterator<Item = ignore::DirEntry> {
    ignore::WalkBuilder::new(dir)
        .hidden(false)
        .git_global(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
}

/// Where a search starts: its path relative to the project folder if
/// inside it. Walks do not follow links, so everything they find below a
/// folder in the project is in the project too.
fn base_of(policy: &Policy, dir: &Path) -> Option<String> {
    policy
        .resolve(&dir.to_string_lossy())
        .ok()
        .and_then(|resolved| resolved.relative)
}

/// How a found file is shown and whether it is in the project: relative
/// to the project folder there, else absolute.
fn shown_path(base: Option<&str>, dir: &Path, path: &Path) -> (String, bool) {
    let relative = path
        .strip_prefix(dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    match base {
        Some("") => (relative, true),
        Some(base) if relative.is_empty() => (base.to_owned(), true),
        Some(base) => (format!("{base}/{relative}"), true),
        None => (path.display().to_string(), false),
    }
}

/// Finds files whose path below `dir` matches a glob (`**/*.rs`,
/// `src/*.ts`); a pattern without `/` matches the file name anywhere.
///
/// # Errors
///
/// An invalid pattern.
pub fn find(policy: &Policy, dir: &Path, pattern: &str) -> Result<String, ToolError> {
    let pattern = pattern.trim();
    let full = if pattern.contains('/') {
        pattern.to_owned()
    } else {
        format!("**/{pattern}")
    };
    let matcher = globset::GlobBuilder::new(&full)
        .literal_separator(true)
        .case_insensitive(cfg!(any(windows, target_os = "macos")))
        .build()
        .map_err(|error| ToolError::new(format!("Invalid pattern: {error}")))?
        .compile_matcher();
    let base = base_of(policy, dir);
    let mut found = Vec::new();
    let mut more = 0usize;
    for entry in walk(dir) {
        let Ok(relative) = entry.path().strip_prefix(dir) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if matcher.is_match(&relative) {
            if found.len() < FIND_RESULTS {
                found.push(shown_path(base.as_deref(), dir, entry.path()).0);
            } else {
                more += 1;
            }
        }
    }
    if found.is_empty() {
        return Ok(format!("No files match {pattern}."));
    }
    found.sort();
    let mut out = found.join("\n");
    if more > 0 {
        let _ = write!(out, "\n[… {more} more; narrow the pattern]");
    }
    Ok(out)
}

/// Options of [`grep`].
#[derive(Debug, Clone, Default)]
pub struct GrepOptions {
    /// Only files whose name or path matches this glob.
    pub glob: Option<String>,
    /// Ignore case.
    pub ignore_case: bool,
    /// The pattern is plain text, not a regular expression.
    pub literal: bool,
}

/// Searches text files below `dir` (or one file) for a regular expression
/// and returns matching lines as `path:line: text`. Secrets in the project
/// are skipped; the regex engine has no backtracking, so no pattern can
/// hang it.
///
/// # Errors
///
/// An invalid pattern or glob.
pub fn grep(
    policy: &Policy,
    target: &Path,
    pattern: &str,
    options: &GrepOptions,
) -> Result<String, ToolError> {
    let source = if options.literal {
        regex::escape(pattern)
    } else {
        pattern.to_owned()
    };
    let regex = regex::RegexBuilder::new(&source)
        .case_insensitive(options.ignore_case)
        .size_limit(1 << 22)
        .build()
        .map_err(|error| ToolError::new(format!("Invalid regular expression: {error}")))?;
    let filter = match &options.glob {
        Some(glob) => {
            let full = if glob.contains('/') {
                glob.clone()
            } else {
                format!("**/{glob}")
            };
            Some(
                globset::GlobBuilder::new(&full)
                    .literal_separator(true)
                    .build()
                    .map_err(|error| ToolError::new(format!("Invalid glob: {error}")))?
                    .compile_matcher(),
            )
        }
        None => None,
    };
    let (dir, files): (&Path, Vec<std::path::PathBuf>) = if target.is_file() {
        (target.parent().unwrap_or(target), vec![target.to_owned()])
    } else {
        let files = walk(target)
            .filter(|entry| {
                let relative = entry.path().strip_prefix(target).unwrap_or(entry.path());
                let relative = relative.to_string_lossy().replace('\\', "/");
                filter
                    .as_ref()
                    .is_none_or(|filter| filter.is_match(&relative))
            })
            .map(ignore::DirEntry::into_path)
            .collect();
        (target, files)
    };
    let base = base_of(policy, dir);
    let mut out = String::new();
    let (mut matches, mut skipped_secrets) = (0usize, 0usize);
    let mut truncated = false;
    'files: for file in files {
        let (shown, inside) = shown_path(base.as_deref(), dir, &file);
        // Inside the project, secrets stay out of searches; outside, the
        // user approved the folder.
        if inside && super::policy::is_secret(&shown) {
            skipped_secrets += 1;
            continue;
        }
        if fs::metadata(&file).map_or(true, |meta| meta.len() > SCAN_BYTES) {
            continue;
        }
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        if is_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (number, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                if matches >= GREP_MATCHES {
                    truncated = true;
                    break 'files;
                }
                let line: String = line.trim_end().chars().take(300).collect();
                let _ = writeln!(out, "{shown}:{}: {line}", number + 1);
                matches += 1;
            }
        }
    }
    if matches == 0 {
        out.push_str("No matches.");
    }
    if truncated {
        let _ = write!(
            out,
            "[Stopped after {GREP_MATCHES} matches; narrow the pattern or the path.]"
        );
    }
    if skipped_secrets > 0 {
        let _ = write!(
            out,
            "\n[{skipped_secrets} file(s) that look like secrets (.env, keys) were not searched; read them one by one if needed, which asks the user.]"
        );
    }
    Ok(out)
}
