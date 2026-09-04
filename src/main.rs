use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRAM: &str = "neondeck";
const FORMAT_VERSION: &str = "neondeck/v1";

fn main() {
    let args: Vec<String> = env::args().collect();
    let code = run(&args);
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    if args.len() < 2 {
        print_help();
        return 0;
    }

    match args[1].as_str() {
        "--help" | "-h" | "help" => {
            print_help();
            0
        }
        "--version" | "-V" | "version" => {
            println!("{} {}", PROGRAM, VERSION);
            0
        }
        "scan" => cmd_scan(&args[2..]),
        "report" => cmd_report(&args[2..]),
        other => {
            eprintln!("error: unknown command '{other}'");
            eprintln!("run '{PROGRAM} --help' for usage");
            2
        }
    }
}

fn print_help() {
    println!(
        "{PROGRAM} {VERSION}
A terminal fleet report for every repo on the grid.

USAGE:
    {PROGRAM} <COMMAND> [ARGS]

COMMANDS:
    scan <PATH> [--todos] [--format table|lines]
                           Scan a directory for git repositories and print a
                           fleet report (sorted by last commit, newest first).
    report <PATH> --out <FILE> [--todos]
                           Write a Markdown fleet report to <FILE>.
    help                     Print this help.
    version                  Print version.

OPTIONS:
    --todos                  Count TODO/FIXME/HACK markers per repo.
    --format table|lines     Output format for scan (default: table).
                             'lines' emits {FORMAT_VERSION} records, one repo
                             per line, pipe-separated, for scripting.

WHAT'S REPORTED:
    repo name, path, current branch, last commit date + subject,
    dirty/clean status, ahead/behind vs upstream, line counts by
    language (file-extension heuristic), optional marker counts.

EXAMPLES:
    {PROGRAM} scan ~/Projects/active
    {PROGRAM} scan ~/Projects/active --todos
    {PROGRAM} scan ~/Projects/active --format lines
    {PROGRAM} report ~/Projects/active --out FLEET_STATUS.md

EXIT CODES:
    0  success
    1  runtime error (bad path, unreadable directory, write failure)
    2  usage error (missing/unknown arguments)

Made by synth with synthclaw 🎹🦞"
    );
}

// ---------- data model ----------

/// Everything we know about one repository on the grid.
#[derive(Debug, Clone, Default)]
pub struct RepoInfo {
    pub path: PathBuf,
    pub name: String,
    pub branch: String,
    pub last_commit_epoch: i64,
    pub last_commit_date: String,
    pub last_commit_subject: String,
    pub dirty: bool,
    pub dirty_files: u32,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub languages: Vec<LangCount>,
    pub todos: Option<MarkerCounts>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangCount {
    pub lang: &'static str,
    pub lines: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkerCounts {
    pub todo: u32,
    pub fixme: u32,
    pub hack: u32,
}

impl RepoInfo {
    /// Sort key: newest commit first. Repos with no commits sink to the bottom.
    fn sort_key(&self) -> i64 {
        -self.last_commit_epoch
    }
}

// ---------- errors ----------

#[derive(Debug)]
pub enum NdError {
    Io(std::io::Error),
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    Git(String),
}

impl fmt::Display for NdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NdError::Io(e) => write!(f, "io error: {e}"),
            NdError::NotFound(p) => write!(f, "path does not exist: {}", p.display()),
            NdError::NotADirectory(p) => write!(f, "path is not a directory: {}", p.display()),
            NdError::Git(m) => write!(f, "git error: {m}"),
        }
    }
}

impl From<std::io::Error> for NdError {
    fn from(e: std::io::Error) -> Self {
        NdError::Io(e)
    }
}

// ---------- scan command ----------

fn cmd_scan(args: &[String]) -> i32 {
    let mut root: Option<PathBuf> = None;
    let mut todos = false;
    let mut format = "table";

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--todos" => todos = true,
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --format requires a value (table|lines)");
                    return 2;
                }
                format = &args[i];
                if format != "table" && format != "lines" {
                    eprintln!("error: unknown format '{format}' (expected table|lines)");
                    return 2;
                }
            }
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown flag '{flag}'");
                eprintln!("run '{PROGRAM} scan --help' via '{PROGRAM} help' for usage");
                return 2;
            }
            positional => {
                if root.is_some() {
                    eprintln!("error: unexpected extra argument '{positional}'");
                    return 2;
                }
                root = Some(PathBuf::from(positional));
            }
        }
        i += 1;
    }

    let root = match root {
        Some(r) => r,
        None => {
            eprintln!("error: scan requires a path");
            eprintln!("usage: {PROGRAM} scan <PATH> [--todos] [--format table|lines]");
            return 2;
        }
    };

    let repos = match collect_fleet(&root, todos) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    if repos.is_empty() {
        println!("no git repositories found under {}", root.display());
        return 0;
    }

    match format {
        "lines" => print!("{}", render_lines(&repos)),
        _ => print!("{}", render_table(&repos, todos)),
    }
    0
}

// ---------- report command ----------

fn cmd_report(args: &[String]) -> i32 {
    let mut root: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut todos = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("error: --out requires a filename");
                    return 2;
                }
                out = Some(PathBuf::from(&args[i]));
            }
            "--todos" => todos = true,
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown flag '{flag}'");
                return 2;
            }
            positional => {
                if root.is_some() {
                    eprintln!("error: unexpected extra argument '{positional}'");
                    return 2;
                }
                root = Some(PathBuf::from(positional));
            }
        }
        i += 1;
    }

    let root = match root {
        Some(r) => r,
        None => {
            eprintln!("error: report requires a path");
            eprintln!("usage: {PROGRAM} report <PATH> --out <FILE> [--todos]");
            return 2;
        }
    };
    let out = match out {
        Some(o) => o,
        None => {
            eprintln!("error: report requires --out <FILE>");
            eprintln!("usage: {PROGRAM} report <PATH> --out <FILE> [--todos]");
            return 2;
        }
    };

    let repos = match collect_fleet(&root, todos) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    let report = render_markdown(&root, &repos, todos);
    if let Err(e) = fs::write(&out, &report) {
        eprintln!("error: failed to write {}: {e}", out.display());
        return 1;
    }

    println!("wrote {} ({} repositories)", out.display(), repos.len());
    0
}

// ---------- fleet collection ----------

fn collect_fleet(root: &Path, todos: bool) -> Result<Vec<RepoInfo>, NdError> {
    if !root.exists() {
        return Err(NdError::NotFound(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(NdError::NotADirectory(root.to_path_buf()));
    }

    let paths = discover_repos(root)?;
    let mut repos: Vec<RepoInfo> = paths
        .iter()
        .map(|p| inspect_repo(p, todos))
        .collect::<Result<_, _>>()?;

    // deterministic: newest commit first, ties broken by name
    repos.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()).then(a.name.cmp(&b.name)));
    Ok(repos)
}

/// Discover git repositories directly under `root` (one level deep).
/// A directory counts as a repo if it contains a `.git` entry (dir or file —
/// worktrees and submodules use a `.git` file).
pub fn discover_repos(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut repos = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(root)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() && path.join(".git").exists() {
            repos.push(path);
        }
    }
    Ok(repos)
}

/// Gather everything about one repo. Git failures degrade gracefully:
/// a repo we can't inspect still appears, marked with what we know.
fn inspect_repo(path: &Path, todos: bool) -> Result<RepoInfo, NdError> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());

    let mut info = RepoInfo {
        path: path.to_path_buf(),
        name,
        branch: String::from("(unknown)"),
        ..Default::default()
    };

    if let Some(branch) = git_branch(path) {
        info.branch = branch;
    }
    if let Some((epoch, date, subject)) = git_last_commit(path) {
        info.last_commit_epoch = epoch;
        info.last_commit_date = date;
        info.last_commit_subject = subject;
    }
    let (dirty, dirty_files) = git_dirty(path);
    info.dirty = dirty;
    info.dirty_files = dirty_files;

    if let Some((upstream, ahead, behind)) = git_ahead_behind(path) {
        info.upstream = Some(upstream);
        info.ahead = ahead;
        info.behind = behind;
    }

    info.languages = count_languages(path);
    if todos {
        info.todos = Some(count_markers(path));
    }
    Ok(info)
}

// ---------- git plumbing (effects isolated here) ----------

fn git_output(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn git_branch(repo: &Path) -> Option<String> {
    // symbolic-ref fails on detached HEAD; fall back to short rev
    if let Some(b) = git_output(repo, &["symbolic-ref", "--short", "HEAD"]) {
        let b = b.trim().to_string();
        if !b.is_empty() {
            return Some(b);
        }
    }
    git_output(repo, &["rev-parse", "--short", "HEAD"]).map(|s| format!("(detached {})", s.trim()))
}

fn git_last_commit(repo: &Path) -> Option<(i64, String, String)> {
    // %ct = committer epoch, %cs = short date, %s = subject
    let out = git_output(repo, &["log", "-1", "--format=%ct|%cs|%s"])?;
    parse_last_commit(out.trim())
}

/// Pure parser for `git log -1 --format=%ct|%cs|%s` output.
pub fn parse_last_commit(line: &str) -> Option<(i64, String, String)> {
    let mut parts = line.splitn(3, '|');
    let epoch: i64 = parts.next()?.trim().parse().ok()?;
    let date = parts.next()?.trim().to_string();
    let subject = parts.next().unwrap_or("").trim().to_string();
    if date.is_empty() {
        return None;
    }
    Some((epoch, date, subject))
}

fn git_dirty(repo: &Path) -> (bool, u32) {
    match git_output(repo, &["status", "--porcelain"]) {
        Some(out) => {
            let n = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
            (n > 0, n)
        }
        None => (false, 0),
    }
}

fn git_ahead_behind(repo: &Path) -> Option<(String, u32, u32)> {
    let upstream = git_output(
        repo,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )?
    .trim()
    .to_string();
    if upstream.is_empty() {
        return None;
    }
    let counts = git_output(
        repo,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )?;
    let (ahead, behind) = parse_ahead_behind(counts.trim())?;
    Some((upstream, ahead, behind))
}

/// Pure parser for `git rev-list --left-right --count` output ("A\tB").
pub fn parse_ahead_behind(line: &str) -> Option<(u32, u32)> {
    let mut parts = line.split_whitespace();
    let ahead: u32 = parts.next()?.parse().ok()?;
    let behind: u32 = parts.next()?.parse().ok()?;
    Some((ahead, behind))
}

// ---------- language line counts ----------

/// Map a file extension to a language name. Pure and total.
pub fn lang_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "mjs" | "cjs" | "jsx" => "JavaScript",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "cs" => "C#",
        "go" => "Go",
        "rb" => "Ruby",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "dart" => "Dart",
        "gd" => "GDScript",
        "lua" => "Lua",
        "zig" => "Zig",
        "odin" => "Odin",
        "sh" | "bash" | "fish" | "zsh" => "Shell",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" => "CSS",
        "md" | "markdown" => "Markdown",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "json" => "JSON",
        "xml" => "XML",
        "sql" => "SQL",
        "vim" => "Vim",
        "el" => "Elisp",
        "ex" | "exs" => "Elixir",
        "erl" | "hrl" => "Erlang",
        "hs" => "Haskell",
        "ml" | "mli" => "OCaml",
        "fs" | "fsx" => "F#",
        "clj" | "cljs" => "Clojure",
        "r" => "R",
        "php" => "PHP",
        "pl" | "pm" => "Perl",
        "vue" => "Vue",
        "svelte" => "Svelte",
        _ => return None,
    })
}

/// Directories that never count toward line totals.
pub fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "target"
            | "node_modules"
            | "dist"
            | "build"
            | "out"
            | ".next"
            | ".cache"
            | "__pycache__"
            | ".venv"
            | "venv"
            | "vendor"
            | "Pods"
            | ".dart_tool"
            | ".idea"
            | ".vscode"
    )
}

/// Walk a repo and count lines per language. Deterministic: entries are
/// visited in sorted order, and results come back sorted by line count.
pub fn count_languages(repo: &Path) -> Vec<LangCount> {
    use std::collections::BTreeMap;
    let mut totals: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut stack: Vec<PathBuf> = vec![repo.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = match fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            Err(_) => continue,
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();
            if path.is_dir() {
                if !is_ignored_dir(&fname) {
                    stack.push(path);
                }
            } else if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if let Some(lang) = lang_for_ext(&ext.to_lowercase()) {
                        if let Some(n) = count_lines(&path) {
                            *totals.entry(lang).or_insert(0) += n;
                        }
                    }
                }
            }
        }
    }

    let mut langs: Vec<LangCount> = totals
        .into_iter()
        .map(|(lang, lines)| LangCount { lang, lines })
        .collect();
    langs.sort_by(|a, b| b.lines.cmp(&a.lines).then(a.lang.cmp(b.lang)));
    langs
}

fn count_lines(path: &Path) -> Option<u64> {
    let bytes = fs::read(path).ok()?;
    // skip likely-binary files cheaply: NUL byte in first 8k
    let probe = &bytes[..bytes.len().min(8192)];
    if probe.contains(&0) {
        return None;
    }
    if bytes.is_empty() {
        return Some(0);
    }
    let mut n = bytes.iter().filter(|&&b| b == b'\n').count() as u64;
    if !bytes.ends_with(b"\n") {
        n += 1;
    }
    Some(n)
}

// ---------- TODO/FIXME/HACK markers ----------

/// Count marker occurrences across recognized source files.
pub fn count_markers(repo: &Path) -> MarkerCounts {
    let mut counts = MarkerCounts::default();
    let mut stack: Vec<PathBuf> = vec![repo.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let mut entries: Vec<_> = match fs::read_dir(&dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
            Err(_) => continue,
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let fname = entry.file_name();
            let fname = fname.to_string_lossy();
            if path.is_dir() {
                if !is_ignored_dir(&fname) {
                    stack.push(path);
                }
            } else if path.is_file() {
                let dominated = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| lang_for_ext(&e.to_lowercase()).is_some())
                    .unwrap_or(false);
                if dominated {
                    if let Ok(text) = fs::read_to_string(&path) {
                        tally_markers(&text, &mut counts);
                    }
                }
            }
        }
    }
    counts
}

/// Pure tally of TODO/FIXME/HACK occurrences in one text.
pub fn tally_markers(text: &str, counts: &mut MarkerCounts) {
    for line in text.lines() {
        let mut rest = line;
        loop {
            let todo = rest.find("TODO");
            let fixme = rest.find("FIXME");
            let hack = rest.find("HACK");
            let (idx, kind, len) = match (todo, fixme, hack) {
                (Some(t), Some(f), Some(h)) => {
                    if t <= f && t <= h {
                        (t, 0, 4)
                    } else if f <= h {
                        (f, 1, 5)
                    } else {
                        (h, 2, 4)
                    }
                }
                (Some(t), Some(f), None) => {
                    if t <= f {
                        (t, 0, 4)
                    } else {
                        (f, 1, 5)
                    }
                }
                (Some(t), None, Some(h)) => {
                    if t <= h {
                        (t, 0, 4)
                    } else {
                        (h, 2, 4)
                    }
                }
                (None, Some(f), Some(h)) => {
                    if f <= h {
                        (f, 1, 5)
                    } else {
                        (h, 2, 4)
                    }
                }
                (Some(t), None, None) => (t, 0, 4),
                (None, Some(f), None) => (f, 1, 5),
                (None, None, Some(h)) => (h, 2, 4),
                (None, None, None) => break,
            };
            match kind {
                0 => counts.todo += 1,
                1 => counts.fixme += 1,
                _ => counts.hack += 1,
            }
            rest = &rest[idx + len..];
        }
    }
}

// ---------- rendering: table ----------

pub fn render_table(repos: &[RepoInfo], todos: bool) -> String {
    let mut s = String::new();

    // column widths from data
    let name_w = repos.iter().map(|r| r.name.len()).max().unwrap_or(4).max(4);
    let branch_w = repos
        .iter()
        .map(|r| r.branch.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let header = if todos {
        format!(
            "{:<nw$}  {:<bw$}  {:<10}  {:<5}  {:<9}  {:<11}  {:<9}  {}\n",
            "REPO",
            "BRANCH",
            "COMMIT",
            "DIRTY",
            "AHEAD/UP",
            "LANGS",
            "T/F/H",
            "LAST COMMIT",
            nw = name_w,
            bw = branch_w
        )
    } else {
        format!(
            "{:<nw$}  {:<bw$}  {:<10}  {:<5}  {:<9}  {:<11}  {}\n",
            "REPO",
            "BRANCH",
            "COMMIT",
            "DIRTY",
            "AHEAD/UP",
            "LANGS",
            "LAST COMMIT",
            nw = name_w,
            bw = branch_w
        )
    };
    s.push_str(&header);
    s.push_str(&"-".repeat(header.trim_end().len().max(40)));
    s.push('\n');

    for r in repos {
        let dirty = if r.dirty {
            format!("+{}", r.dirty_files)
        } else {
            "clean".to_string()
        };
        let ab = match &r.upstream {
            Some(_) => format!("+{}/-{}", r.ahead, r.behind),
            None => "-".to_string(),
        };
        let langs = summarize_langs(&r.languages);
        let subject = truncate(&r.last_commit_subject, 48);
        if todos {
            let m = r.todos.unwrap_or_default();
            s.push_str(&format!(
                "{:<nw$}  {:<bw$}  {:<10}  {:<5}  {:<9}  {:<11}  {:<9}  {}\n",
                r.name,
                r.branch,
                r.last_commit_date,
                dirty,
                ab,
                langs,
                format!("{}/{}/{}", m.todo, m.fixme, m.hack),
                subject,
                nw = name_w,
                bw = branch_w
            ));
        } else {
            s.push_str(&format!(
                "{:<nw$}  {:<bw$}  {:<10}  {:<5}  {:<9}  {:<11}  {}\n",
                r.name,
                r.branch,
                r.last_commit_date,
                dirty,
                ab,
                langs,
                subject,
                nw = name_w,
                bw = branch_w
            ));
        }
    }
    s.push_str(&format!("\n{} repositories\n", repos.len()));
    s
}

/// "Rust:12.3k C++:4.1k" style summary, top 3 languages.
pub fn summarize_langs(langs: &[LangCount]) -> String {
    if langs.is_empty() {
        return "-".to_string();
    }
    langs
        .iter()
        .take(3)
        .map(|l| format!("{}:{}", l.lang, human_lines(l.lines)))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn human_lines(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ---------- rendering: lines (scripting format) ----------

/// Versioned line-oriented format, one record per repo:
/// `neondeck/v1|name|path|branch|epoch|date|dirty|dirty_files|ahead|behind|langs|todos|fixmes|hacks|subject`
/// Fields are only ever appended at the end; bump FORMAT_VERSION otherwise.
pub fn render_lines(repos: &[RepoInfo]) -> String {
    let mut s = String::new();
    for r in repos {
        let langs = r
            .languages
            .iter()
            .map(|l| format!("{}:{}", l.lang, l.lines))
            .collect::<Vec<_>>()
            .join(",");
        let m = r.todos.unwrap_or_default();
        s.push_str(&format!(
            "{v}|{name}|{path}|{branch}|{epoch}|{date}|{dirty}|{dirty_files}|{ahead}|{behind}|{langs}|{todo}|{fixme}|{hack}|{subject}\n",
            v = FORMAT_VERSION,
            name = escape_field(&r.name),
            path = escape_field(&r.path.display().to_string()),
            branch = escape_field(&r.branch),
            epoch = r.last_commit_epoch,
            date = r.last_commit_date,
            dirty = r.dirty,
            dirty_files = r.dirty_files,
            ahead = r.ahead,
            behind = r.behind,
            langs = escape_field(&langs),
            todo = m.todo,
            fixme = m.fixme,
            hack = m.hack,
            subject = escape_field(&r.last_commit_subject),
        ));
    }
    s
}

/// Keep the line format single-line and pipe-safe.
pub fn escape_field(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

// ---------- rendering: markdown ----------

pub fn render_markdown(root: &Path, repos: &[RepoInfo], todos: bool) -> String {
    let mut s = String::new();
    s.push_str("# Fleet Status\n\n");
    s.push_str(&format!("Scanned: `{}`\n\n", root.display()));

    if todos {
        s.push_str("| Repository | Branch | Last Commit | Dirty | Ahead/Behind | Languages | T/F/H | Subject |\n");
        s.push_str("|---|---|---|---|---|---|---|---|\n");
    } else {
        s.push_str(
            "| Repository | Branch | Last Commit | Dirty | Ahead/Behind | Languages | Subject |\n",
        );
        s.push_str("|---|---|---|---|---|---|---|\n");
    }

    for r in repos {
        let dirty = if r.dirty {
            format!("+{}", r.dirty_files)
        } else {
            "clean".to_string()
        };
        let ab = match &r.upstream {
            Some(u) => format!("+{}/-{} ({u})", r.ahead, r.behind),
            None => "-".to_string(),
        };
        let langs = summarize_langs(&r.languages);
        let subject = r.last_commit_subject.replace('|', "\\|");
        if todos {
            let m = r.todos.unwrap_or_default();
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {}/{}/{} | {} |\n",
                r.name,
                r.branch,
                r.last_commit_date,
                dirty,
                ab,
                langs,
                m.todo,
                m.fixme,
                m.hack,
                subject
            ));
        } else {
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} |\n",
                r.name, r.branch, r.last_commit_date, dirty, ab, langs, subject
            ));
        }
    }

    s.push_str(&format!("\n**{} repositories**\n", repos.len()));
    s.push_str("\n---\nMade by synth with synthclaw 🎹🦞\n");
    s
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all};
    use std::io::Write;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("neondeck-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        create_dir_all(&dir).unwrap();
        dir
    }

    // --- discovery ---

    #[test]
    fn discovers_git_repos() {
        let root = tmpdir("discover");
        let repo_a = root.join("alpha");
        let repo_b = root.join("beta");
        let not_repo = root.join("gamma");
        create_dir_all(repo_a.join(".git")).unwrap();
        create_dir_all(repo_b.join(".git")).unwrap();
        create_dir_all(&not_repo).unwrap();

        let repos = discover_repos(&root).unwrap();
        assert_eq!(repos.len(), 2);
        assert!(repos.contains(&repo_a));
        assert!(repos.contains(&repo_b));
        assert!(!repos.contains(&not_repo));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discovery_is_deterministic() {
        let root = tmpdir("deterministic");
        for name in ["zeta", "alpha", "omega"] {
            create_dir_all(root.join(name).join(".git")).unwrap();
        }
        let first = discover_repos(&root).unwrap();
        let second = discover_repos(&root).unwrap();
        assert_eq!(first, second);
        let names: Vec<_> = first
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["alpha", "omega", "zeta"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_dir_yields_no_repos() {
        let root = tmpdir("empty");
        let repos = discover_repos(&root).unwrap();
        assert!(repos.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gitfile_worktree_counts_as_repo() {
        // worktrees/submodules have a .git *file*, not a directory
        let root = tmpdir("gitfile");
        let repo = root.join("worktree");
        create_dir_all(&repo).unwrap();
        let mut f = File::create(repo.join(".git")).unwrap();
        writeln!(f, "gitdir: /elsewhere/.git/worktrees/x").unwrap();
        let repos = discover_repos(&root).unwrap();
        assert_eq!(repos.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    // --- git output parsers ---

    #[test]
    fn parses_last_commit_line() {
        let (epoch, date, subject) =
            parse_last_commit("1757000000|2026-09-04|feat: add scan command").unwrap();
        assert_eq!(epoch, 1757000000);
        assert_eq!(date, "2026-09-04");
        assert_eq!(subject, "feat: add scan command");
    }

    #[test]
    fn parses_last_commit_with_pipes_in_subject() {
        let (_, _, subject) = parse_last_commit("1|2026-01-01|fix a | b case").unwrap();
        assert_eq!(subject, "fix a | b case");
    }

    #[test]
    fn rejects_garbage_commit_line() {
        assert!(parse_last_commit("not-a-number|date|subj").is_none());
        assert!(parse_last_commit("").is_none());
        assert!(parse_last_commit("123||subj").is_none());
    }

    #[test]
    fn parses_ahead_behind() {
        assert_eq!(parse_ahead_behind("3\t7"), Some((3, 7)));
        assert_eq!(parse_ahead_behind("0 12"), Some((0, 12)));
        assert_eq!(parse_ahead_behind("nope"), None);
    }

    // --- languages ---

    #[test]
    fn maps_extensions_to_languages() {
        assert_eq!(lang_for_ext("rs"), Some("Rust"));
        assert_eq!(lang_for_ext("py"), Some("Python"));
        assert_eq!(lang_for_ext("ts"), Some("TypeScript"));
        assert_eq!(lang_for_ext("cpp"), Some("C++"));
        assert_eq!(lang_for_ext("cs"), Some("C#"));
        assert_eq!(lang_for_ext("xyz"), None);
    }

    #[test]
    fn ignores_build_dirs() {
        assert!(is_ignored_dir("target"));
        assert!(is_ignored_dir("node_modules"));
        assert!(is_ignored_dir(".git"));
        assert!(!is_ignored_dir("src"));
    }

    #[test]
    fn counts_lines_per_language() {
        let repo = tmpdir("langs");
        create_dir_all(repo.join("src")).unwrap();
        create_dir_all(repo.join("target")).unwrap(); // ignored
        let mut rs = File::create(repo.join("src").join("main.rs")).unwrap();
        writeln!(rs, "fn main() {{}}\n// two\n// three").unwrap();
        let mut py = File::create(repo.join("src").join("tool.py")).unwrap();
        writeln!(py, "print('hi')").unwrap();
        let mut ignored = File::create(repo.join("target").join("junk.rs")).unwrap();
        writeln!(ignored, "this should not count").unwrap();

        let langs = count_languages(&repo);
        let rust = langs.iter().find(|l| l.lang == "Rust").unwrap();
        assert_eq!(rust.lines, 3);
        let python = langs.iter().find(|l| l.lang == "Python").unwrap();
        assert_eq!(python.lines, 1);
        // target/ excluded, so only 2 languages
        assert_eq!(langs.len(), 2);
        // sorted descending by lines
        assert_eq!(langs[0].lang, "Rust");
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn counts_final_line_without_newline() {
        let repo = tmpdir("nonewline");
        let mut f = File::create(repo.join("a.rs")).unwrap();
        write!(f, "line one\nline two").unwrap();
        let langs = count_languages(&repo);
        assert_eq!(langs[0].lines, 2);
        let _ = fs::remove_dir_all(&repo);
    }

    // --- markers ---

    #[test]
    fn tallies_markers_in_text() {
        let mut c = MarkerCounts::default();
        tally_markers(
            "// TODO: fix this\n// FIXME: and this\n// HACK: yikes\n// TODO TODO",
            &mut c,
        );
        assert_eq!(c.todo, 3);
        assert_eq!(c.fixme, 1);
        assert_eq!(c.hack, 1);
    }

    #[test]
    fn markers_case_sensitive_and_word_boundaries_loose() {
        let mut c = MarkerCounts::default();
        tally_markers("todo lowercase does not count\nTODO(x) counts", &mut c);
        assert_eq!(c.todo, 1);
    }

    // --- rendering ---

    fn sample_repo(name: &str, epoch: i64) -> RepoInfo {
        RepoInfo {
            path: PathBuf::from(format!("/grid/{name}")),
            name: name.to_string(),
            branch: "main".to_string(),
            last_commit_epoch: epoch,
            last_commit_date: "2026-09-04".to_string(),
            last_commit_subject: "shiny commit".to_string(),
            dirty: false,
            dirty_files: 0,
            upstream: Some("origin/main".to_string()),
            ahead: 1,
            behind: 2,
            languages: vec![
                LangCount {
                    lang: "Rust",
                    lines: 12345,
                },
                LangCount {
                    lang: "TOML",
                    lines: 42,
                },
            ],
            todos: Some(MarkerCounts {
                todo: 2,
                fixme: 1,
                hack: 0,
            }),
        }
    }

    #[test]
    fn table_contains_all_columns() {
        let repos = vec![sample_repo("alpha", 100)];
        let out = render_table(&repos, true);
        assert!(out.contains("REPO"));
        assert!(out.contains("BRANCH"));
        assert!(out.contains("DIRTY"));
        assert!(out.contains("AHEAD/UP"));
        assert!(out.contains("T/F/H"));
        assert!(out.contains("alpha"));
        assert!(out.contains("+1/-2"));
        assert!(out.contains("Rust:12.3k"));
        assert!(out.contains("2/1/0"));
        assert!(out.contains("1 repositories"));
    }

    #[test]
    fn lines_format_is_versioned_and_escaped() {
        let mut r = sample_repo("al|pha", 100);
        r.last_commit_subject = "multi\nline|pipe".to_string();
        let out = render_lines(&[r]);
        let line = out.lines().next().unwrap();
        assert!(line.starts_with("neondeck/v1|"));
        assert!(line.contains("al\\|pha"));
        assert!(line.contains("multi\\nline\\|pipe"));
        // one record => exactly one line, no raw newline leaked from subject
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn lines_format_field_count_exact() {
        let out = render_lines(&[sample_repo("plain", 100)]);
        let line = out.lines().next().unwrap();
        // 15 fields => 14 unescaped separators
        assert_eq!(line.split('|').count(), 15);
    }

    #[test]
    fn markdown_report_contains_table() {
        let root = PathBuf::from("/grid");
        let repos = vec![sample_repo("demo", 100)];
        let md = render_markdown(&root, &repos, false);
        assert!(md.contains("# Fleet Status"));
        assert!(md.contains("| Repository | Branch |"));
        assert!(md.contains("| demo |"));
        assert!(md.contains("**1 repositories**"));
        assert!(md.contains("synthclaw"));
    }

    #[test]
    fn sort_order_newest_first() {
        let mut repos = vec![
            sample_repo("old", 100),
            sample_repo("new", 300),
            sample_repo("mid", 200),
        ];
        repos.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()).then(a.name.cmp(&b.name)));
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["new", "mid", "old"]);
    }

    #[test]
    fn human_lines_formats() {
        assert_eq!(human_lines(0), "0");
        assert_eq!(human_lines(999), "999");
        assert_eq!(human_lines(1234), "1.2k");
        assert_eq!(human_lines(2_500_000), "2.5M");
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        let long = "x".repeat(100);
        let t = truncate(&long, 10);
        assert_eq!(t.chars().count(), 10);
        assert!(t.ends_with('…'));
    }

    #[test]
    fn escape_field_handles_pipes_and_newlines() {
        assert_eq!(escape_field("a|b\nc\\d"), "a\\|b\\nc\\\\d");
    }

    #[test]
    fn summarize_langs_top_three() {
        let langs = vec![
            LangCount {
                lang: "Rust",
                lines: 5000,
            },
            LangCount {
                lang: "C++",
                lines: 3000,
            },
            LangCount {
                lang: "Shell",
                lines: 100,
            },
            LangCount {
                lang: "TOML",
                lines: 10,
            },
        ];
        assert_eq!(summarize_langs(&langs), "Rust:5.0k C++:3.0k Shell:100");
        assert_eq!(summarize_langs(&[]), "-");
    }

    // --- error paths ---

    #[test]
    fn collect_fleet_fails_closed_on_missing_path() {
        let err = collect_fleet(Path::new("/definitely/not/here"), false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("path does not exist"), "got: {msg}");
    }

    #[test]
    fn collect_fleet_fails_closed_on_file() {
        let dir = tmpdir("notadir");
        let file = dir.join("a-file");
        File::create(&file).unwrap();
        let err = collect_fleet(&file, false).unwrap_err();
        assert!(err.to_string().contains("not a directory"));
        let _ = fs::remove_dir_all(&dir);
    }

    // --- integration: real git repo in tempdir ---

    fn make_git_repo(root: &Path, name: &str) -> PathBuf {
        let repo = root.join(name);
        create_dir_all(&repo).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_AUTHOR_NAME", "test")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "test")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        run(&["init", "-q", "-b", "main"]);
        let mut f = File::create(repo.join("main.rs")).unwrap();
        writeln!(f, "fn main() {{}} // TODO: wire it").unwrap();
        drop(f);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "initial commit"]);
        repo
    }

    #[test]
    fn inspects_real_git_repo() {
        let root = tmpdir("realrepo");
        let repo = make_git_repo(&root, "demo");

        let info = inspect_repo(&repo, true).unwrap();
        assert_eq!(info.name, "demo");
        assert_eq!(info.branch, "main");
        assert_eq!(info.last_commit_subject, "initial commit");
        assert!(info.last_commit_epoch > 0);
        assert!(!info.dirty);
        assert_eq!(info.dirty_files, 0);
        assert!(info.upstream.is_none()); // no remote configured
        assert_eq!(info.languages[0].lang, "Rust");
        assert_eq!(info.todos.unwrap().todo, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_dirty_files() {
        let root = tmpdir("dirtyrepo");
        let repo = make_git_repo(&root, "demo");
        File::create(repo.join("untracked.rs")).unwrap();

        let info = inspect_repo(&repo, false).unwrap();
        assert!(info.dirty);
        assert_eq!(info.dirty_files, 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fleet_sorted_newest_first() {
        let root = tmpdir("fleet");
        make_git_repo(&root, "first");
        // ensure a later commit timestamp for the second repo
        std::thread::sleep(std::time::Duration::from_secs(1));
        make_git_repo(&root, "second");

        let fleet = collect_fleet(&root, false).unwrap();
        assert_eq!(fleet.len(), 2);
        assert_eq!(fleet[0].name, "second");
        assert_eq!(fleet[1].name, "first");
        let _ = fs::remove_dir_all(&root);
    }
}
