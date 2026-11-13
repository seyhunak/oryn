//! Static repository analysis. Reads files, never executes them.

use once_cell::sync::Lazy;
use oryn_auth::{redact::redact, sha256_hex};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use utoipa::ToSchema;
use walkdir::WalkDir;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "out",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    "coverage",
    ".turbo",
    ".cache",
    ".idea",
    ".vscode",
];
const MAX_FILES: usize = 25_000;
const MAX_READ_BYTES: u64 = 1_000_000;
const MAX_HASH_BYTES: u64 = 8_000_000;
const CHUNK_CHARS: usize = 1_600;
const MAX_CHUNKS: usize = 2_000;

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct LanguageStat {
    pub files: u32,
    pub lines: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, ToSchema)]
pub struct Dependency {
    pub ecosystem: String,
    pub name: String,
    pub version: Option<String>,
    pub dev: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, ToSchema)]
pub struct Route {
    pub method: String,
    pub path: String,
    pub file: String,
    /// `page` (a browser route) or `api`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OpenApiDoc {
    pub file: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ModuleInfo {
    pub name: String,
    pub path_prefix: String,
    pub kind: String,
    pub file_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct ChangeSet {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub removed: Vec<String>,
}

impl ChangeSet {
    pub fn total(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len()
    }
    pub fn touched(&self) -> Vec<String> {
        self.added
            .iter()
            .chain(&self.modified)
            .chain(&self.removed)
            .cloned()
            .collect()
    }
}

/// The persisted analysis snapshot (stored as JSON in `application_contexts.analysis`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct Analysis {
    pub file_count: u32,
    pub total_bytes: u64,
    pub truncated: bool,
    pub languages: BTreeMap<String, LanguageStat>,
    pub frameworks: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub routes: Vec<Route>,
    pub openapi: Vec<OpenApiDoc>,
    pub test_files: Vec<String>,
    pub feature_files: Vec<String>,
    pub docs: Vec<String>,
    pub modules: Vec<ModuleInfo>,
    pub changes: ChangeSet,
}

/// Analysis plus the artefacts that are stored separately.
pub struct AnalysisOutput {
    pub analysis: Analysis,
    pub hashes: BTreeMap<String, String>,
    /// (source path, content) chunks for full-text retrieval; secrets redacted.
    pub chunks: Vec<(String, String)>,
    pub summary: String,
}

fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "rs" => "Rust",
        "ts" | "tsx" | "mts" | "cts" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "rb" => "Ruby",
        "php" => "PHP",
        "cs" => "C#",
        "swift" => "Swift",
        "scala" => "Scala",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" => "C++",
        "vue" => "Vue",
        "svelte" => "Svelte",
        "html" | "htm" => "HTML",
        "css" | "scss" | "sass" | "less" => "CSS",
        "sql" => "SQL",
        "feature" => "Gherkin",
        "sh" | "bash" | "zsh" => "Shell",
        _ => return None,
    })
}

fn is_text_ext(path: &str) -> bool {
    language_of(path).is_some()
        || [
            ".md",
            ".txt",
            ".json",
            ".yaml",
            ".yml",
            ".toml",
            ".xml",
            ".gradle",
            ".env.example",
            ".graphql",
            ".proto",
        ]
        .iter()
        .any(|e| path.ends_with(e))
}

fn is_test_file(path: &str) -> bool {
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(?i)(\.(test|spec)\.[a-z]+$|(^|/)test_[^/]+\.py$|_test\.(go|py|rs)$|(^|/)(tests?|__tests__|e2e|cypress|playwright)/)").unwrap()
    });
    RE.is_match(path) && language_of(path).is_some()
}

fn is_doc(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".txt") || lower.starts_with("docs/")
}

/// Walk `root` and analyse it. `previous` holds the hashes from the last run, for the change set.
pub fn analyze_dir(
    root: &Path,
    previous: &BTreeMap<String, String>,
) -> std::io::Result<AnalysisOutput> {
    let mut a = Analysis::default();
    let mut hashes = BTreeMap::new();
    let mut texts: Vec<(String, String)> = Vec::new();
    let mut frameworks = BTreeSet::new();
    let mut deps = BTreeSet::new();
    let mut routes = BTreeSet::new();
    let mut module_counts: BTreeMap<String, u32> = BTreeMap::new();

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && e.depth() > 0
                && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        });
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue; // symlinks and specials are never followed
        }
        if a.file_count as usize >= MAX_FILES {
            a.truncated = true;
            break;
        }
        let rel = rel_path(root, entry.path());
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        a.file_count += 1;
        a.total_bytes += size;
        *module_counts.entry(module_prefix(&rel)).or_default() += 1;

        if size <= MAX_HASH_BYTES {
            if let Ok(bytes) = std::fs::read(entry.path()) {
                hashes.insert(rel.clone(), sha256_hex(&bytes));
                if size <= MAX_READ_BYTES && is_text_ext(&rel) {
                    if let Ok(text) = String::from_utf8(bytes) {
                        texts.push((rel.clone(), text));
                    }
                }
            }
        } else {
            hashes.insert(rel.clone(), format!("size:{size}"));
        }

        if let Some(lang) = language_of(&rel) {
            let stat = a.languages.entry(lang.to_string()).or_default();
            stat.files += 1;
        }
        if rel.ends_with(".feature") {
            a.feature_files.push(rel.clone());
        } else if is_test_file(&rel) {
            a.test_files.push(rel.clone());
        }
        if is_doc(&rel) && !rel.contains("CHANGELOG") {
            a.docs.push(rel.clone());
        }
    }

    for (path, text) in &texts {
        if let Some(lang) = language_of(path) {
            if let Some(stat) = a.languages.get_mut(lang) {
                stat.lines += text.lines().count() as u64;
            }
        }
        manifest(path, text, &mut deps, &mut frameworks);
        if language_of(path).is_some() && !is_test_file(path) {
            extract_routes(path, text, &mut routes);
        }
        if let Some(doc) = openapi_doc(path, text) {
            for ep in &doc.endpoints {
                if let Some((m, p)) = ep.split_once(' ') {
                    routes.insert(Route {
                        method: m.into(),
                        path: p.into(),
                        file: path.clone(),
                        kind: "api".into(),
                    });
                }
            }
            a.openapi.push(doc);
        }
    }
    next_routes(&hashes, &mut routes);

    a.frameworks = frameworks.into_iter().collect();
    a.dependencies = deps.into_iter().collect();
    a.routes = routes.into_iter().collect();
    a.modules = module_counts
        .into_iter()
        .filter(|(p, n)| *n >= 2 && !p.is_empty())
        .map(|(p, n)| ModuleInfo {
            name: p.rsplit('/').next().unwrap_or(&p).to_string(),
            kind: module_kind(&p).into(),
            path_prefix: p,
            file_count: n,
        })
        .collect();

    for (path, h) in &hashes {
        match previous.get(path) {
            None if !previous.is_empty() => a.changes.added.push(path.clone()),
            Some(old) if old != h => a.changes.modified.push(path.clone()),
            _ => {}
        }
    }
    a.changes.removed = previous
        .keys()
        .filter(|p| !hashes.contains_key(*p))
        .cloned()
        .collect();

    let chunks = chunk_texts(&texts);
    let summary = summarize(&a);
    Ok(AnalysisOutput {
        analysis: a,
        hashes,
        chunks,
        summary,
    })
}

fn rel_path(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Group files into modules: `src/cart/x.ts` → `src/cart`, `crates/api/src/..` → `crates/api`.
fn module_prefix(rel: &str) -> String {
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() < 2 {
        return String::new();
    }
    let containers = [
        "src",
        "app",
        "lib",
        "pages",
        "packages",
        "crates",
        "apps",
        "services",
        "modules",
        "internal",
        "pkg",
        "components",
        "features",
    ];
    if containers.contains(&parts[0]) && parts.len() >= 3 {
        format!("{}/{}", parts[0], parts[1])
    } else {
        parts[0].to_string()
    }
}

fn module_kind(prefix: &str) -> &'static str {
    let p = prefix.to_ascii_lowercase();
    if p.contains("test") || p.contains("spec") || p.contains("e2e") || p.contains("feature") {
        "tests"
    } else if p.starts_with("docs") {
        "docs"
    } else if p.contains("api") || p.contains("server") || p.contains("routes") {
        "api"
    } else if p.contains("component")
        || p.contains("pages")
        || p.contains("app")
        || p.contains("public")
        || p.contains("views")
    {
        "ui"
    } else {
        "module"
    }
}

fn manifest(path: &str, text: &str, deps: &mut BTreeSet<Dependency>, fw: &mut BTreeSet<String>) {
    let file = path.rsplit('/').next().unwrap_or(path);
    let known: &[(&str, &str)] = &[
        ("next", "Next.js"),
        ("react", "React"),
        ("vue", "Vue"),
        ("@angular/core", "Angular"),
        ("svelte", "Svelte"),
        ("express", "Express"),
        ("fastify", "Fastify"),
        ("@nestjs/core", "NestJS"),
        ("koa", "Koa"),
        ("@playwright/test", "Playwright"),
        ("playwright", "Playwright"),
        ("cypress", "Cypress"),
        ("jest", "Jest"),
        ("vitest", "Vitest"),
        ("mocha", "Mocha"),
        ("@cucumber/cucumber", "Cucumber"),
        ("axum", "Axum"),
        ("actix-web", "Actix Web"),
        ("rocket", "Rocket"),
        ("django", "Django"),
        ("flask", "Flask"),
        ("fastapi", "FastAPI"),
        ("pytest", "pytest"),
        ("github.com/gin-gonic/gin", "Gin"),
        ("github.com/labstack/echo", "Echo"),
        ("spring-boot", "Spring Boot"),
        ("rails", "Rails"),
        ("laravel/framework", "Laravel"),
    ];
    let mut add = |eco: &str, name: &str, ver: Option<String>, dev: bool| {
        for (k, label) in known {
            if name == *k
                || name.starts_with(&format!("{k}/"))
                || (eco == "maven" && name.contains(k))
            {
                fw.insert((*label).to_string());
            }
        }
        deps.insert(Dependency {
            ecosystem: eco.into(),
            name: name.into(),
            version: ver,
            dev,
        });
    };
    match file {
        "package.json" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                for (section, dev) in [("dependencies", false), ("devDependencies", true)] {
                    if let Some(o) = v[section].as_object() {
                        for (k, ver) in o {
                            add("npm", k, ver.as_str().map(String::from), dev);
                        }
                    }
                }
                if v["dependencies"]
                    .as_object()
                    .map(|o| o.is_empty())
                    .unwrap_or(true)
                    && v["scripts"]["start"]
                        .as_str()
                        .map(|s| s.contains("node"))
                        .unwrap_or(false)
                {
                    fw.insert("Node.js".into());
                }
            }
        }
        "Cargo.toml" => {
            let mut section = String::new();
            for line in text.lines() {
                let l = line.trim();
                if l.starts_with('[') {
                    section = l.trim_matches(|c| c == '[' || c == ']').to_string();
                    continue;
                }
                if section.ends_with("dependencies") {
                    if let Some((name, rest)) = l.split_once('=') {
                        let ver = rest.split('"').nth(1).map(str::to_string);
                        add("cargo", name.trim(), ver, section.contains("dev"));
                    }
                }
            }
        }
        "requirements.txt" => {
            for l in text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('-'))
            {
                let (name, ver) = l
                    .split_once("==")
                    .map(|(n, v)| (n, Some(v.to_string())))
                    .unwrap_or((l, None));
                add("pypi", &name.trim().to_ascii_lowercase(), ver, false);
            }
        }
        "go.mod" => {
            for l in text.lines().map(str::trim) {
                let l = l.trim_start_matches("require ").trim();
                if let Some((name, ver)) = l.split_once(' ') {
                    if name.contains('.') && ver.starts_with('v') {
                        add("go", name, Some(ver.to_string()), false);
                    }
                }
            }
        }
        "pom.xml" | "build.gradle" | "build.gradle.kts" => {
            if text.contains("spring-boot") {
                add("maven", "spring-boot", None, false);
            }
        }
        "Gemfile" => {
            static GEM: Lazy<Regex> =
                Lazy::new(|| Regex::new(r#"(?m)^\s*gem\s+['"]([^'"]+)['"]"#).unwrap());
            for c in GEM.captures_iter(text) {
                add("rubygems", &c[1], None, false);
            }
        }
        "composer.json" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
                if let Some(o) = v["require"].as_object() {
                    for (k, ver) in o {
                        add("composer", k, ver.as_str().map(String::from), false);
                    }
                }
            }
        }
        _ => {}
    }
}

fn kind_for(path: &str) -> &'static str {
    if path.starts_with("/api") || path.contains("/api/") {
        "api"
    } else {
        "page"
    }
}

fn extract_routes(file: &str, text: &str, out: &mut BTreeSet<Route>) {
    static PATTERNS: Lazy<Vec<(Regex, bool)>> = Lazy::new(|| {
        vec![
            // Express / Koa / Fastify / Hono: app.get('/x', …)
            (Regex::new(r#"\b(?:app|router|server|api|fastify)\.(get|post|put|patch|delete)\(\s*['"`](/[^'"`]*)['"`]"#).unwrap(), true),
            // FastAPI / Flask: @app.get("/x") / @router.post("/x")
            (Regex::new(r#"@(?:app|router|bp|blueprint)\.(get|post|put|patch|delete|route)\(\s*['"](/[^'"]*)['"]"#).unwrap(), true),
            // Spring: @GetMapping("/x")
            (Regex::new(r#"@(Get|Post|Put|Patch|Delete|Request)Mapping\(\s*(?:value\s*=\s*|path\s*=\s*)?"(/[^"]*)""#).unwrap(), true),
            // Plain Node: pathname === '/x' && method === 'GET'
            (Regex::new(r#"pathname\s*===\s*['"](/[^'"]*)['"]\s*&&\s*method\s*===\s*['"]([A-Z]+)['"]"#).unwrap(), false),
            // Plain Node: pathname === '/x' (method resolved below)
            (Regex::new(r#"if\s*\(\s*pathname\s*===\s*['"](/[^'"]*)['"]\s*\)"#).unwrap(), false),
        ]
    });
    static AXUM: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"\.route\(\s*"(/[^"]*)"\s*,\s*([a-z_]+)"#).unwrap());
    for (i, (re, method_first)) in PATTERNS.iter().enumerate() {
        for c in re.captures_iter(text) {
            let (method, path) = match (i, method_first) {
                (3, _) => (c[2].to_string(), c[1].to_string()),
                (4, _) => ("GET".to_string(), c[1].to_string()),
                (_, true) => (c[1].to_ascii_uppercase(), c[2].to_string()),
                _ => continue,
            };
            let method = match method.as_str() {
                "ROUTE" | "REQUEST" => "GET".to_string(),
                m => m.to_string(),
            };
            if path.starts_with("/api/__") {
                continue; // test hooks are not product routes
            }
            out.insert(Route {
                kind: kind_for(&path).into(),
                method,
                path,
                file: file.into(),
            });
        }
    }
    for c in AXUM.captures_iter(text) {
        out.insert(Route {
            method: c[2].to_ascii_uppercase(),
            path: c[1].to_string(),
            file: file.into(),
            kind: kind_for(&c[1]).into(),
        });
    }
}

/// Next.js app-router (`app/x/page.tsx`) and pages-router routes from file paths.
fn next_routes(hashes: &BTreeMap<String, String>, out: &mut BTreeSet<Route>) {
    for p in hashes.keys() {
        let (segments, is_page) = if let Some(rest) = p.split("app/").nth(1).filter(|_| {
            p.contains("app/")
                && p.rsplit('/')
                    .next()
                    .map(|f| f.starts_with("page."))
                    .unwrap_or(false)
        }) {
            (
                rest.rsplit_once('/')
                    .map(|(d, _)| d.to_string())
                    .unwrap_or_default(),
                true,
            )
        } else if let Some(rest) = p
            .strip_prefix("pages/")
            .or_else(|| p.split("/pages/").nth(1))
        {
            let stem = rest.rsplit_once('.').map(|(s, _)| s).unwrap_or(rest);
            if stem.starts_with('_')
                || !p.ends_with("x") && !p.ends_with(".js") && !p.ends_with(".ts")
            {
                continue;
            }
            (
                stem.trim_end_matches("index")
                    .trim_end_matches('/')
                    .to_string(),
                true,
            )
        } else {
            continue;
        };
        if !is_page {
            continue;
        }
        let route: Vec<&str> = segments
            .split('/')
            .filter(|s| !s.is_empty() && !(s.starts_with('(') && s.ends_with(')')))
            .collect();
        let path = format!("/{}", route.join("/"));
        out.insert(Route {
            method: "GET".into(),
            kind: kind_for(&path).into(),
            path,
            file: p.clone(),
        });
    }
}

fn openapi_doc(path: &str, text: &str) -> Option<OpenApiDoc> {
    let lower = path.to_ascii_lowercase();
    let v: serde_json::Value = if lower.ends_with(".json") {
        if !text.contains("\"openapi\"") && !text.contains("\"swagger\"") {
            return None;
        }
        serde_json::from_str(text).ok()?
    } else if lower.ends_with(".yaml") || lower.ends_with(".yml") {
        if !text.contains("openapi:") && !text.contains("swagger:") {
            return None;
        }
        serde_yaml::from_str(text).ok()?
    } else {
        return None;
    };
    if v.get("openapi").is_none() && v.get("swagger").is_none() {
        return None;
    }
    let mut endpoints = vec![];
    if let Some(paths) = v["paths"].as_object() {
        for (p, ops) in paths {
            if let Some(ops) = ops.as_object() {
                for m in ops
                    .keys()
                    .filter(|m| ["get", "post", "put", "patch", "delete"].contains(&m.as_str()))
                {
                    endpoints.push(format!("{} {}", m.to_ascii_uppercase(), p));
                }
            }
        }
    }
    Some(OpenApiDoc {
        file: path.into(),
        title: v["info"]["title"].as_str().map(String::from),
        version: v["info"]["version"].as_str().map(String::from),
        endpoints,
    })
}

fn chunk_texts(texts: &[(String, String)]) -> Vec<(String, String)> {
    // Docs and route-bearing sources first: they matter most for planning.
    let mut ordered: Vec<&(String, String)> = texts.iter().collect();
    ordered.sort_by_key(|(p, _)| {
        (
            if is_doc(p) {
                0
            } else if p.ends_with(".feature") {
                1
            } else {
                2
            },
            p.len(),
        )
    });
    let mut out = vec![];
    for (path, text) in ordered {
        if path.ends_with("lock.json") || path.ends_with(".lock") || path.ends_with(".min.js") {
            continue;
        }
        let mut buf = String::new();
        for line in text.lines() {
            if buf.len() + line.len() > CHUNK_CHARS && !buf.is_empty() {
                out.push((path.clone(), redact(&buf)));
                buf.clear();
            }
            buf.push_str(&line.chars().take(CHUNK_CHARS).collect::<String>());
            buf.push('\n');
        }
        if !buf.trim().is_empty() {
            out.push((path.clone(), redact(&buf)));
        }
        if out.len() >= MAX_CHUNKS {
            out.truncate(MAX_CHUNKS);
            break;
        }
    }
    out
}

pub fn summarize(a: &Analysis) -> String {
    let mut langs: Vec<(&String, &LanguageStat)> = a.languages.iter().collect();
    langs.sort_by_key(|x| std::cmp::Reverse(x.1.lines));
    let langs: Vec<String> = langs
        .iter()
        .take(4)
        .map(|(l, s)| format!("{l} ({} files)", s.files))
        .collect();
    let pages = a.routes.iter().filter(|r| r.kind == "page").count();
    let apis = a.routes.iter().filter(|r| r.kind == "api").count();
    format!(
        "{} files in {}. Frameworks: {}. {} page routes and {} API endpoints detected; {} OpenAPI document(s); {} existing test files and {} feature files.",
        a.file_count,
        if langs.is_empty() { "no recognised languages".to_string() } else { langs.join(", ") },
        if a.frameworks.is_empty() { "none detected".to_string() } else { a.frameworks.join(", ") },
        pages,
        apis,
        a.openapi.len(),
        a.test_files.len(),
        a.feature_files.len()
    )
}

/// Map a list of changed paths to the analysis modules they belong to.
pub fn modules_for(paths: &[String], modules: &[ModuleInfo]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for p in paths {
        if let Some(m) = modules
            .iter()
            .filter(|m| p.starts_with(&format!("{}/", m.path_prefix)) || p == &m.path_prefix)
            .max_by_key(|m| m.path_prefix.len())
        {
            out.insert(m.path_prefix.clone());
        }
    }
    out.into_iter().collect()
}

pub fn workspace_path(base: &Path, repo_id: &str) -> PathBuf {
    base.join(repo_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyses_demo_like_tree() {
        let dir = tempfile::tempdir().unwrap();
        let w = |p: &str, c: &str| {
            let f = dir.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        };
        w(
            "package.json",
            r#"{"name":"x","scripts":{"start":"node src/server.js"},"devDependencies":{"vitest":"1.0.0"}}"#,
        );
        w("src/server.js", "if (pathname === '/api/products' && method === 'GET') {}\nif (pathname === '/login') {}\napp.post('/api/cart', h)\nif (pathname === '/api/__reset' && method === 'POST') {}");
        w(
            "src/pricing.js",
            "export const x = 1;\nconst key = 'sk-proj-abcdefghijklmnopqrstuvwx';",
        );
        w("test/pricing.test.js", "test('x', () => {})");
        w("features/a.feature", "Feature: A");
        w(
            "openapi.json",
            r#"{"openapi":"3.0.0","info":{"title":"Demo","version":"1"},"paths":{"/api/health":{"get":{}}}}"#,
        );
        w("node_modules/x/index.js", "ignored");
        w("app/settings/page.tsx", "export default function P(){}");

        let out = analyze_dir(dir.path(), &BTreeMap::new()).unwrap();
        let a = &out.analysis;
        assert!(a.frameworks.contains(&"Vitest".to_string()));
        let paths: Vec<_> = a
            .routes
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect();
        for want in [
            "GET /api/products",
            "GET /login",
            "POST /api/cart",
            "GET /api/health",
            "GET /settings",
        ] {
            assert!(
                paths.contains(&want.to_string()),
                "missing {want} in {paths:?}"
            );
        }
        assert!(!paths.iter().any(|p| p.contains("__reset")));
        assert_eq!(a.test_files, vec!["test/pricing.test.js".to_string()]);
        assert_eq!(a.feature_files.len(), 1);
        assert!(!out.hashes.keys().any(|k| k.contains("node_modules")));
        assert!(
            out.chunks
                .iter()
                .all(|(_, c)| !c.contains("sk-proj-abcdef")),
            "chunks are redacted"
        );

        // Incremental: modify one file.
        std::fs::write(dir.path().join("src/pricing.js"), "changed").unwrap();
        let again = analyze_dir(dir.path(), &out.hashes).unwrap();
        assert_eq!(
            again.analysis.changes.modified,
            vec!["src/pricing.js".to_string()]
        );
        assert!(again.analysis.changes.added.is_empty());
        assert_eq!(
            modules_for(&again.analysis.changes.modified, &again.analysis.modules),
            vec!["src".to_string()]
        );
    }
}
