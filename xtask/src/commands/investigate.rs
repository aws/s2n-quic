// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use std::{
    ffi::OsStr,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Debug, Args)]
pub struct Investigate {
    /// Parquet file(s) to load; glob-expanded, required, repeatable
    #[arg(short, long = "input", required = true)]
    inputs: Vec<String>,

    /// Bypass the cached DuckDB file; rebuild views from scratch
    #[arg(long)]
    no_cache: bool,

    /// Override the cache directory (default: $TMPDIR/s2n-quic-investigate)
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Override the SQL queries directory (default: dc/queries relative to repo root)
    #[arg(long)]
    queries_dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Open an interactive DuckDB shell with all views loaded [default]
    Shell,
    /// Print all dashboard views sequentially, then exit
    Dashboard,
    /// Print the named view, then exit
    Query { name: String },
    /// Run custom SQL against all views, then exit (reads --sql or stdin)
    Exec {
        #[arg(long)]
        sql: Option<String>,
    },
    /// Print all compare views (requires >1 parquet matched)
    Compare,
    /// List all available view names by tier, then exit
    List,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const FNV1A_OFFSET_BASIS: u64 = 14695981039346656037;
const FNV1A_PRIME: u64 = 1099511628211;

fn fnv1a_update(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV1A_PRIME);
    }
    h
}

/// Derive a short hex cache key that covers:
///   - the canonical paths of every input file
///   - the mtime + size of every input file (invalidates on content change)
///   - the name + content of every SQL file loaded from queries_dir
fn cache_key(inputs: &[PathBuf], sql_files: &SqlFiles) -> String {
    let mut h = FNV1A_OFFSET_BASIS;
    for path in inputs {
        h = fnv1a_update(h, &[0x01]);
        h = fnv1a_update(h, path.to_string_lossy().as_bytes());
        if let Ok(meta) = path.metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(dur) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    h = fnv1a_update(h, dur.as_nanos().to_string().as_bytes());
                }
            }
            h = fnv1a_update(h, meta.len().to_string().as_bytes());
        }
    }
    for (name, content) in sql_files.all_named() {
        h = fnv1a_update(h, &[0x02]);
        h = fnv1a_update(h, name.as_bytes());
        h = fnv1a_update(h, &[0x00]);
        h = fnv1a_update(h, content.as_bytes());
    }
    format!("{h:016x}")
}

/// Resolve all `-i` glob patterns to canonical, sorted `PathBuf`s.
fn resolve_inputs(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let matches: Vec<_> = glob::glob(pattern)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?
            .collect::<std::result::Result<_, _>>()
            .with_context(|| format!("error expanding glob: {pattern}"))?;
        if matches.is_empty() {
            bail!("no files matched: {pattern}");
        }
        for p in matches {
            let canonical = p
                .canonicalize()
                .with_context(|| format!("cannot canonicalize: {}", p.display()))?;
            paths.push(canonical);
        }
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        bail!("no input files found after glob expansion");
    }
    Ok(paths)
}

// ---------------------------------------------------------------------------
// SQL file loading
// ---------------------------------------------------------------------------

struct SqlFiles {
    views: Vec<(String, String)>,
    dashboard: Vec<(String, String)>,
    compare: Vec<(String, String)>,
}

impl SqlFiles {
    fn load(queries_dir: &Path) -> Result<Self> {
        Ok(Self {
            views: load_sql_dir(&queries_dir.join("views"))?,
            dashboard: load_sql_dir(&queries_dir.join("dashboard"))?,
            compare: load_sql_dir(&queries_dir.join("compare"))?,
        })
    }

    /// Iterator of `(filename, content)` pairs across all tiers (stable order).
    fn all_named(&self) -> impl Iterator<Item = (&str, &str)> {
        self.views
            .iter()
            .chain(&self.dashboard)
            .chain(&self.compare)
            .map(|(n, c)| (n.as_str(), c.as_str()))
    }

    /// View names (without extension) in each tier.
    fn view_names(&self) -> Vec<String> {
        self.views.iter().map(|(n, _)| stem(n)).collect()
    }
    fn dashboard_names(&self) -> Vec<String> {
        self.dashboard.iter().map(|(n, _)| stem(n)).collect()
    }
    fn compare_names(&self) -> Vec<String> {
        self.compare.iter().map(|(n, _)| stem(n)).collect()
    }
}

/// Load all `*.sql` files in `dir`, sorted by filename, returning `(filename, content)` pairs.
/// Prefixes like `010_` can be used to make load order explicit.
fn load_sql_dir(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut files: Vec<(String, String)> = Vec::new();
    if !dir.exists() {
        return Ok(files);
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read directory: {}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension() == Some(OsStr::new("sql")))
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read: {}", path.display()))?;
        files.push((name, content));
    }
    Ok(files)
}

/// Return the stem (filename without `.sql`) for a SQL filename.
fn stem(filename: &str) -> String {
    let stem = filename.strip_suffix(".sql").unwrap_or(filename);
    if let Some((prefix, rest)) = stem.split_once('_') {
        if !rest.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            return rest.to_owned();
        }
    }
    stem.to_owned()
}

fn resolve_query_view_name(sql_files: &SqlFiles, name: &str) -> Option<String> {
    if sql_files.view_names().iter().any(|n| n == name) {
        return Some(name.to_owned());
    }
    if sql_files.dashboard_names().iter().any(|n| n == name) {
        return Some(format!("dashboard_{name}"));
    }
    if sql_files.compare_names().iter().any(|n| n == name) {
        return Some(format!("compare_{name}"));
    }
    None
}

// ---------------------------------------------------------------------------
// DuckDB path / init SQL construction
// ---------------------------------------------------------------------------

fn default_cache_dir() -> PathBuf {
    std::env::temp_dir().join("s2n-quic-investigate")
}

fn quote_sql_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Build the SQL script that creates all views from scratch.
fn build_init_sql(inputs: &[PathBuf], sql_files: &SqlFiles) -> String {
    let mut sql = String::new();

    // Base metrics view covering all input files.
    let path_list = inputs
        .iter()
        .map(|p| quote_sql_string(&p.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(", ");
    sql.push_str(&format!(
        "CREATE OR REPLACE VIEW metrics AS SELECT * FROM read_parquet([{path_list}]);\n"
    ));

    // Multi-run comparison view (only when >1 input).
    if inputs.len() > 1 {
        let unions = inputs
            .iter()
            .map(|p| {
                let path = p.to_string_lossy();
                let quoted_path = quote_sql_string(&path);
                format!(
                    "SELECT *, {quoted_path} AS label FROM read_parquet({quoted_path})"
                )
            })
            .collect::<Vec<_>>()
            .join("\nUNION ALL\n");
        sql.push_str(&format!("CREATE OR REPLACE VIEW runs AS\n{unions};\n"));
    }

    // All SQL view definitions, in tier order.
    for (_, content) in sql_files.all_named() {
        sql.push_str(content);
        if !content.ends_with('\n') {
            sql.push('\n');
        }
    }

    sql
}

/// Build the query portion for the `dashboard` subcommand.
fn build_dashboard_sql(sql_files: &SqlFiles) -> String {
    let mut sql = String::new();
    for name in sql_files.dashboard_names() {
        let view = format!("dashboard_{name}");
        sql.push_str(&format!(
            "SELECT '{name}' AS view;\nSELECT * FROM {view};\n"
        ));
    }
    sql
}

/// Build the query portion for the `compare` subcommand.
fn build_compare_sql(sql_files: &SqlFiles) -> String {
    let mut sql = String::new();
    for name in sql_files.compare_names() {
        let view = format!("compare_{name}");
        sql.push_str(&format!(
            "SELECT '{name}' AS view;\nSELECT * FROM {view};\n"
        ));
    }
    sql
}

// ---------------------------------------------------------------------------
// DuckDB execution helpers
// ---------------------------------------------------------------------------

/// Write `content` to a named temp file and return its path.
/// The file is placed in `std::env::temp_dir()` so it survives a process exec.
fn write_temp_sql(content: &str) -> Result<PathBuf> {
    let path =
        std::env::temp_dir().join(format!("s2n-quic-investigate-{}.sql", std::process::id()));
    std::fs::write(&path, content)
        .with_context(|| format!("cannot write temp file: {}", path.display()))?;
    Ok(path)
}

/// Run `sql` (piped to stdin) against `db` and forward output to the terminal.
fn run_duckdb(db: &str, sql: &str) -> Result<()> {
    let mut child = Command::new("duckdb")
        .arg(db)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn duckdb — is it installed and on PATH?")?;
    child
        .stdin
        .as_mut()
        .expect("stdin was configured as piped")
        .write_all(sql.as_bytes())
        .context("failed to write to duckdb stdin")?;
    let status = child.wait().context("failed to wait for duckdb")?;
    if !status.success() {
        bail!("duckdb exited with {status}");
    }
    Ok(())
}

/// Replace the current process with an interactive `duckdb` shell.
/// If `init_sql` is provided it is written to a temp file and passed via `-init`.
fn exec_duckdb_shell(db: &str, init_sql: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("duckdb");
    cmd.arg(db);

    if let Some(sql) = init_sql {
        let path = write_temp_sql(sql)?;
        cmd.arg("-init").arg(path);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        return Err(anyhow::anyhow!("failed to exec duckdb: {}", cmd.exec()));
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().context("failed to spawn duckdb")?;
        if !status.success() {
            bail!("duckdb exited with {status}");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

impl Investigate {
    pub fn run(self, _sh: &xshell::Shell) -> Result<()> {
        // 1. Resolve input files.
        let inputs = resolve_inputs(&self.inputs)?;

        // 2. Locate the queries directory.
        let queries_dir = self
            .queries_dir
            .clone()
            .unwrap_or_else(find_queries_dir);

        // 3. Load SQL files.
        let sql_files = SqlFiles::load(&queries_dir)?;

        // 4. Dispatch the list subcommand early (no DuckDB needed).
        if let Some(Cmd::List) = &self.cmd {
            return self.cmd_list(&sql_files);
        }

        // 5. Validate compare requires >1 file.
        if let Some(Cmd::Compare) = &self.cmd {
            if inputs.len() < 2 {
                bail!(
                    "the `compare` subcommand requires more than one input file; \
                     pass multiple -i flags"
                );
            }
        }

        // 6. Determine database path and whether init is needed.
        let (db_path, need_init) = if self.no_cache {
            // --no-cache: always use a fresh temp db so views are available in shell.
            let tmp = std::env::temp_dir().join(format!(
                "s2n-quic-investigate-{}.duckdb",
                std::process::id()
            ));
            (tmp.display().to_string(), true)
        } else {
            let cache_dir = self.cache_dir.clone().unwrap_or_else(default_cache_dir);
            std::fs::create_dir_all(&cache_dir)
                .with_context(|| format!("cannot create cache dir: {}", cache_dir.display()))?;
            let key = cache_key(&inputs, &sql_files);
            let db = cache_dir.join(format!("{key}.duckdb"));
            let exists = db.exists();
            (db.display().to_string(), !exists)
        };

        // 7. Build init SQL if needed.
        let init_sql = if need_init {
            Some(build_init_sql(&inputs, &sql_files))
        } else {
            None
        };

        // 8. Dispatch subcommand.
        match self.cmd.unwrap_or(Cmd::Shell) {
            Cmd::Shell => {
                exec_duckdb_shell(&db_path, init_sql.as_deref())?;
            }
            Cmd::Dashboard => {
                let query = build_dashboard_sql(&sql_files);
                let script = full_script(init_sql.as_deref(), &query);
                run_duckdb(&db_path, &script)?;
            }
            Cmd::Query { name } => {
                let Some(view_name) = resolve_query_view_name(&sql_files, &name) else {
                    bail!(
                        "unknown query '{name}'; run `cargo xtask investigate --input <glob> list`"
                    );
                };
                let query = format!("SELECT * FROM {view_name};\n");
                let script = full_script(init_sql.as_deref(), &query);
                run_duckdb(&db_path, &script)?;
            }
            Cmd::Exec { sql } => {
                let query = match sql {
                    Some(s) => s,
                    None => {
                        let mut buf = String::new();
                        std::io::stdin()
                            .read_to_string(&mut buf)
                            .context("failed to read SQL from stdin")?;
                        buf
                    }
                };
                let script = full_script(init_sql.as_deref(), &query);
                run_duckdb(&db_path, &script)?;
            }
            Cmd::Compare => {
                let query = build_compare_sql(&sql_files);
                let script = full_script(init_sql.as_deref(), &query);
                run_duckdb(&db_path, &script)?;
            }
            Cmd::List => unreachable!("handled above"),
        }

        Ok(())
    }

    fn cmd_list(&self, sql_files: &SqlFiles) -> Result<()> {
        println!("views:");
        for name in sql_files.view_names() {
            println!("  {name}");
        }
        println!("dashboard:");
        for name in sql_files.dashboard_names() {
            println!("  {name}");
        }
        println!("compare:");
        for name in sql_files.compare_names() {
            println!("  {name}");
        }
        Ok(())
    }
}

/// Concatenate optional init SQL with query SQL, prefixed with output-mode settings.
fn full_script(init_sql: Option<&str>, query_sql: &str) -> String {
    let mut s = String::new();
    s.push_str(".mode table\n");
    if let Some(init) = init_sql {
        s.push_str(init);
        s.push('\n');
    }
    s.push_str(query_sql);
    s
}

/// Walk up from the current directory to find the `dc/queries` folder.
fn find_queries_dir() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let candidate = dir.join("dc").join("queries");
        if candidate.is_dir() {
            return candidate;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    // Fallback: relative path from wherever the binary is invoked.
    PathBuf::from("dc/queries")
}
