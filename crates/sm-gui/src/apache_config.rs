//! Read-only Apache config parsing and DocumentRoot → app mapping suggestions.
//!
//! The parser follows the subset of Apache configuration needed to discover
//! `DocumentRoot` values: `Define`, `${VAR}` expansion, `ServerRoot`, `Include`,
//! and `IncludeOptional` (including wildcard includes). It never executes Apache
//! or any other program, and traversal is bounded by depth/file limits plus a
//! visited set. Writes happen only in the explicit `apply_apache_mappings`
//! command after the user confirms the selected suggestions.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::Local;
use serde::Serialize;
use sm_config::ConfigPaths;
use sm_core::{ServiceConfig, ServiceType};
use sm_updater::registry::{load_config, save_config, AppConfig};
use sm_updater::settings::{effective_settings, settings_path_for};

use crate::updater::{load_views_at, UpdaterAppView};

const MAX_INCLUDE_DEPTH: usize = 12;
const MAX_CONFIG_FILES: usize = 200;
const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_VARIABLE_DEPTH: usize = 16;
const DEFAULT_APACHE_CONFIG: &str = "conf/httpd.conf";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApacheDocumentRoot {
    pub document_root: String,
    pub config_file: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedApacheConfig {
    pub document_roots: Vec<ApacheDocumentRoot>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApacheServiceOption {
    pub name: String,
    pub display_name: String,
    pub executable: String,
    pub root: String,
    pub config: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApacheMappingSuggestionView {
    pub app_id: String,
    pub app_name: String,
    pub local_path: String,
    pub document_root: String,
    pub config_file: String,
    /// `exact`, `descendant`, or `ancestor`.
    pub match_kind: String,
    /// `high` or `medium`.
    pub confidence: String,
    /// Existing `webServerService`, if the app is already mapped.
    pub existing_service: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheMappingScanView {
    /// Empty when no single service was selected/available.
    pub service_name: String,
    pub services: Vec<ApacheServiceOption>,
    pub suggestions: Vec<ApacheMappingSuggestionView>,
    pub already_mapped: Vec<ApacheMappingSuggestionView>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

struct ParseState {
    apache_root: PathBuf,
    server_root: Option<PathBuf>,
    variables: BTreeMap<String, String>,
    visited: HashSet<String>,
    files_read: usize,
    file_limit_warned: bool,
    warnings: Vec<String>,
    document_roots: Vec<ApacheDocumentRoot>,
}

impl ParseState {
    fn new(apache_root: PathBuf) -> Self {
        Self {
            apache_root,
            server_root: None,
            variables: BTreeMap::new(),
            visited: HashSet::new(),
            files_read: 0,
            file_limit_warned: false,
            warnings: Vec::new(),
            document_roots: Vec::new(),
        }
    }

    fn warn(&mut self, message: impl Into<String>) {
        let message = message.into();
        if !self.warnings.iter().any(|existing| existing == &message) {
            self.warnings.push(message);
        }
    }

    fn expand_variables(&mut self, value: &str, location: &str) -> Option<String> {
        match expand_with(&self.variables, value, 0) {
            Ok(expanded) => Some(expanded),
            Err(error) => {
                self.warn(format!("{location}: variable expansion failed: {error}"));
                None
            }
        }
    }

    fn include_bases(&self, current_file: &Path) -> Vec<PathBuf> {
        let mut bases = Vec::new();
        if let Some(root) = &self.server_root {
            bases.push(root.clone());
        }
        bases.push(self.apache_root.clone());
        if let Some(parent) = current_file.parent() {
            bases.push(parent.to_path_buf());
        }

        let mut seen = BTreeSet::new();
        bases.retain(|base| {
            let key = base
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase();
            !key.is_empty() && seen.insert(key)
        });
        bases
    }

    fn parse_file(&mut self, path: PathBuf, depth: usize, required: bool) {
        if depth > MAX_INCLUDE_DEPTH {
            self.warn(format!(
                "Apache include depth limit ({MAX_INCLUDE_DEPTH}) reached; skipped {}.",
                path.display()
            ));
            return;
        }
        if self.files_read >= MAX_CONFIG_FILES {
            if !self.file_limit_warned {
                self.file_limit_warned = true;
                self.warn(format!(
                    "Apache config file limit ({MAX_CONFIG_FILES}) reached; remaining includes were skipped."
                ));
            }
            return;
        }

        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                if required {
                    self.warn(format!(
                        "Apache config file not found or unreadable: {} ({error})",
                        path.display()
                    ));
                }
                return;
            }
        };
        if !metadata.is_file() {
            if required {
                self.warn(format!(
                    "Apache config path is not a file: {}",
                    path.display()
                ));
            }
            return;
        }
        if metadata.len() > MAX_CONFIG_BYTES {
            self.warn(format!(
                "Apache config file is larger than {} bytes; skipped {}.",
                MAX_CONFIG_BYTES,
                path.display()
            ));
            return;
        }

        let key = fs::canonicalize(&path)
            .unwrap_or_else(|_| path.clone())
            .to_string_lossy()
            .to_ascii_lowercase();
        if !self.visited.insert(key) {
            return;
        }

        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.warn(format!(
                    "Failed to read Apache config {}: {error}",
                    path.display()
                ));
                return;
            }
        };
        self.files_read += 1;

        let text = String::from_utf8_lossy(&bytes);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let display = path.to_string_lossy().to_string();

        for (index, raw_line) in text.lines().enumerate() {
            let line_number = index + 1;
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }
            let (tokens, unterminated) = tokenize(line);
            if unterminated {
                self.warn(format!(
                    "{display}:{line_number}: unterminated quote; directive was ignored or partially parsed."
                ));
            }
            let Some(directive) = tokens.first() else {
                continue;
            };
            let directive = directive.to_ascii_lowercase();
            let location = format!("{display}:{line_number}");

            match directive.as_str() {
                "define" => self.handle_define(&tokens, &location),
                "serverroot" => self.handle_server_root(&tokens, &location),
                "include" => self.handle_include(&tokens, &path, depth, true, &location),
                "includeoptional" => self.handle_include(&tokens, &path, depth, false, &location),
                "documentroot" => self.handle_document_root(&tokens, &path, &location),
                _ => {}
            }
        }
    }

    fn handle_define(&mut self, tokens: &[String], location: &str) {
        let Some(name) = tokens.get(1).map(|value| value.trim()) else {
            self.warn(format!("{location}: Define is missing a variable name."));
            return;
        };
        if name.is_empty() {
            self.warn(format!("{location}: Define is missing a variable name."));
            return;
        }
        let value = tokens.get(2..).unwrap_or_default().join(" ");
        self.variables
            .insert(name.to_ascii_uppercase(), value.trim().to_string());
    }

    fn handle_server_root(&mut self, tokens: &[String], location: &str) {
        let Some(raw) = tokens.get(1) else {
            self.warn(format!("{location}: ServerRoot is missing a path."));
            return;
        };
        let Some(expanded) = self.expand_variables(raw, location) else {
            return;
        };
        let base = self.apache_root.clone();
        self.server_root = Some(resolve_path_value(&expanded, &base));
    }

    fn handle_include(
        &mut self,
        tokens: &[String],
        current_file: &Path,
        depth: usize,
        required: bool,
        location: &str,
    ) {
        let Some(raw) = tokens.get(1) else {
            self.warn(format!("{location}: Include is missing a path."));
            return;
        };
        let Some(expanded) = self.expand_variables(raw, location) else {
            return;
        };
        let expanded = expanded.trim();
        if expanded.is_empty() {
            self.warn(format!("{location}: Include has an empty path."));
            return;
        }

        let wildcard = has_glob(expanded);
        let mut matches = Vec::new();
        for base in self.include_bases(current_file) {
            let candidate = if is_absolute_path_text(expanded) {
                PathBuf::from(expanded)
            } else {
                base.join(expanded)
            };
            let mut found = if wildcard {
                expand_wildcard(&candidate)
            } else if candidate.is_file() {
                vec![candidate]
            } else {
                Vec::new()
            };
            if !found.is_empty() {
                found.sort();
                matches = found;
                break;
            }
        }

        if matches.is_empty() {
            if required {
                self.warn(format!(
                    "{location}: Apache Include was not found: {expanded}"
                ));
            }
            return;
        }

        for file in matches {
            self.parse_file(file, depth + 1, required);
        }
    }

    fn handle_document_root(&mut self, tokens: &[String], current_file: &Path, location: &str) {
        if tokens.len() < 2 {
            self.warn(format!("{location}: DocumentRoot is missing a path."));
            return;
        }
        let raw = tokens[1..].join(" ");
        let Some(expanded) = self.expand_variables(&raw, location) else {
            return;
        };
        let expanded = expanded.trim().trim_matches('"').trim();
        if expanded.is_empty() {
            self.warn(format!("{location}: DocumentRoot has an empty path."));
            return;
        }

        let base = self
            .server_root
            .clone()
            .unwrap_or_else(|| self.apache_root.clone());
        let document_root = resolve_path_value(expanded, &base);
        let document_root = document_root.to_string_lossy().to_string();
        if document_root.trim().is_empty() {
            self.warn(format!(
                "{location}: DocumentRoot resolved to an empty path."
            ));
            return;
        }

        self.document_roots.push(ApacheDocumentRoot {
            document_root,
            config_file: current_file.to_string_lossy().to_string(),
        });
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (index, character) in line.char_indices() {
        match quote {
            Some(active) if character == active => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == '#' => return &line[..index],
            None => {}
        }
    }
    line
}

fn tokenize(line: &str) -> (Vec<String>, bool) {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;

    for character in line.chars() {
        match quote {
            Some(active) if character == active => {
                quote = None;
                started = true;
            }
            Some(_) => current.push(character),
            None if character == '"' || character == '\'' => {
                quote = Some(character);
                started = true;
            }
            None if character.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(character);
                started = true;
            }
        }
    }
    if started {
        tokens.push(current);
    }
    (tokens, quote.is_some())
}

fn expand_with(
    variables: &BTreeMap<String, String>,
    value: &str,
    depth: usize,
) -> Result<String, String> {
    if depth > MAX_VARIABLE_DEPTH {
        return Err(format!(
            "variable expansion depth limit ({MAX_VARIABLE_DEPTH}) reached"
        ));
    }

    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err("unterminated ${...} reference".to_string());
        };
        let name = after[..end].trim();
        if name.is_empty() {
            return Err("empty ${} variable name".to_string());
        }
        let key = name.to_ascii_uppercase();
        let Some(replacement) = variables.get(&key) else {
            return Err(format!("undefined variable '${{{name}}}'"));
        };
        output.push_str(&expand_with(variables, replacement, depth + 1)?);
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

pub(crate) fn is_absolute_path_text(value: &str) -> bool {
    let value = value.trim();
    Path::new(value).is_absolute()
        || value.starts_with('/')
        || value.starts_with("\\\\")
        || value.starts_with("//")
}

fn resolve_path_value(value: &str, base: &Path) -> PathBuf {
    let value = value.trim().trim_matches('"').trim();
    if is_absolute_path_text(value) {
        PathBuf::from(value)
    } else {
        base.join(value)
    }
}

fn has_glob(value: &str) -> bool {
    value.contains('*') || value.contains('?')
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let text: Vec<char> = text.to_ascii_lowercase().chars().collect();
    let (mut pattern_index, mut text_index) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_text = 0usize;

    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == text[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some(pattern_index);
            star_text = text_index;
            pattern_index += 1;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_text += 1;
            text_index = star_text;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn expand_wildcard(pattern: &Path) -> Vec<PathBuf> {
    let mut current: Vec<PathBuf> = Vec::new();
    for component in pattern.components() {
        match component {
            Component::Prefix(prefix) => {
                let text = prefix.as_os_str().to_string_lossy();
                current = vec![PathBuf::from(format!("{text}\\"))];
            }
            Component::RootDir => {
                if current.is_empty() {
                    current = vec![PathBuf::from("\\")];
                }
            }
            Component::CurDir => {}
            Component::ParentDir => {
                for base in &mut current {
                    base.push("..");
                }
            }
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                if has_glob(&name) {
                    let mut next = Vec::new();
                    for base in &current {
                        let Ok(entries) = fs::read_dir(base) else {
                            continue;
                        };
                        let mut matches: Vec<PathBuf> = entries
                            .flatten()
                            .map(|entry| entry.path())
                            .filter(|path| {
                                path.file_name()
                                    .map(|file_name| {
                                        glob_match(&name, &file_name.to_string_lossy())
                                    })
                                    .unwrap_or(false)
                            })
                            .collect();
                        matches.sort();
                        next.extend(matches.into_iter().take(MAX_CONFIG_FILES));
                    }
                    current = next;
                } else {
                    for base in &mut current {
                        base.push(name.as_ref());
                    }
                }
            }
        }
        if current.is_empty() {
            break;
        }
    }
    current
}

/// Resolve the configured config path: absolute, relative to the Apache root,
/// or the default `conf/httpd.conf`.
pub(crate) fn resolve_config_path(root: &Path, config: &str) -> PathBuf {
    let config = config.trim();
    if config.is_empty() {
        return root.join(DEFAULT_APACHE_CONFIG);
    }
    if is_absolute_path_text(config) {
        PathBuf::from(config)
    } else {
        root.join(config)
    }
}

/// Parse an Apache config file and its includes, read-only.
pub(crate) fn parse_apache_config(root: &Path, config: &str) -> ParsedApacheConfig {
    let config_path = resolve_config_path(root, config);
    let mut state = ParseState::new(root.to_path_buf());
    state.parse_file(config_path, 0, true);
    ParsedApacheConfig {
        document_roots: state.document_roots,
        warnings: state.warnings,
    }
}

/// Component-based, case-insensitive Windows path comparison. This avoids
/// treating `C:\app` and `C:\application` as related.
fn path_components(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .split('\\')
        .map(|component| component.trim().to_ascii_lowercase())
        .filter(|component| !component.is_empty())
        .collect()
}

pub(crate) fn paths_equal(left: &str, right: &str) -> bool {
    let left = path_components(left);
    let right = path_components(right);
    !left.is_empty() && left == right
}

pub(crate) fn path_is_descendant(path: &str, root: &str) -> bool {
    let path = path_components(path);
    let root = path_components(root);
    !path.is_empty()
        && !root.is_empty()
        && path.len() > root.len()
        && path[..root.len()] == root[..]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchKind {
    Exact,
    Descendant,
    Ancestor,
}

impl MatchKind {
    fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Descendant => "descendant",
            Self::Ancestor => "ancestor",
        }
    }

    fn confidence(self) -> &'static str {
        match self {
            Self::Exact | Self::Descendant => "high",
            Self::Ancestor => "medium",
        }
    }
}

fn classify_match(local_path: &str, document_root: &str) -> Option<MatchKind> {
    if paths_equal(local_path, document_root) {
        Some(MatchKind::Exact)
    } else if path_is_descendant(document_root, local_path) {
        Some(MatchKind::Descendant)
    } else if path_is_descendant(local_path, document_root) {
        Some(MatchKind::Ancestor)
    } else {
        None
    }
}

fn best_document_root<'a>(
    local_path: &str,
    document_roots: &'a [ApacheDocumentRoot],
) -> Option<(&'a ApacheDocumentRoot, MatchKind)> {
    let mut best: Option<(&ApacheDocumentRoot, MatchKind)> = None;
    for document_root in document_roots {
        let Some(kind) = classify_match(local_path, &document_root.document_root) else {
            continue;
        };
        if best
            .as_ref()
            .map(|(_, current)| kind < *current)
            .unwrap_or(true)
        {
            best = Some((document_root, kind));
        }
    }
    best
}

fn suggestion_view(
    app: &AppConfig,
    matched: Option<(&ApacheDocumentRoot, MatchKind)>,
    existing_service: &str,
) -> ApacheMappingSuggestionView {
    let (document_root, config_file, match_kind, confidence) = match matched {
        Some((document_root, kind)) => (
            document_root.document_root.clone(),
            document_root.config_file.clone(),
            kind.label().to_string(),
            kind.confidence().to_string(),
        ),
        None => (String::new(), String::new(), String::new(), String::new()),
    };
    ApacheMappingSuggestionView {
        app_id: app.id.clone(),
        app_name: app.name.clone(),
        local_path: app.local_path.clone(),
        document_root,
        config_file,
        match_kind,
        confidence,
        existing_service: existing_service.to_string(),
    }
}

/// Build suggestions. Apps already linked to any `webServerService` are
/// returned separately and never proposed for overwrite.
pub(crate) fn build_mapping_suggestions(
    document_roots: &[ApacheDocumentRoot],
    apps: &[AppConfig],
) -> (
    Vec<ApacheMappingSuggestionView>,
    Vec<ApacheMappingSuggestionView>,
) {
    // Keep first-seen app order while merging duplicate app ids. Exact and
    // descendant matches beat ancestor matches; an existing mapping always
    // wins over a suggestion so detection never proposes an overwrite.
    let mut candidates: Vec<(ApacheMappingSuggestionView, bool, Option<MatchKind>)> = Vec::new();
    let mut indexes: BTreeMap<String, usize> = BTreeMap::new();

    for app in apps {
        let app_key = app.id.trim().to_ascii_lowercase();
        if app_key.is_empty() {
            continue;
        }
        let matched = best_document_root(&app.local_path, document_roots);
        let existing = app.web_server_service.trim();
        let is_mapped = !existing.is_empty();
        if !is_mapped && matched.is_none() {
            continue;
        }
        let view = suggestion_view(app, matched, existing);
        let rank = matched.map(|(_, kind)| kind);

        if let Some(index) = indexes.get(&app_key).copied() {
            let (current_view, current_mapped, current_rank) = &mut candidates[index];
            if is_mapped && !*current_mapped {
                *current_view = view;
                *current_mapped = true;
                *current_rank = rank;
                continue;
            }
            if is_mapped == *current_mapped {
                let better = match (rank, *current_rank) {
                    (Some(next), Some(current)) => next < current,
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if better {
                    *current_view = view;
                    *current_rank = rank;
                }
            }
            continue;
        }

        indexes.insert(app_key, candidates.len());
        candidates.push((view, is_mapped, rank));
    }

    let mut suggestions = Vec::new();
    let mut already_mapped = Vec::new();
    for (view, mapped, _) in candidates {
        if mapped {
            already_mapped.push(view);
        } else {
            suggestions.push(view);
        }
    }
    (suggestions, already_mapped)
}

fn apache_service_options(services: &[ServiceConfig]) -> Vec<ApacheServiceOption> {
    let mut options: Vec<ApacheServiceOption> = services
        .iter()
        .filter(|service| service.service_type.normalized() == ServiceType::Apache)
        .map(|service| ApacheServiceOption {
            name: service.name.clone(),
            display_name: if service.display_name.trim().is_empty() {
                service.name.clone()
            } else {
                service.display_name.clone()
            },
            executable: service.executable.clone(),
            root: service.apache_root.clone(),
            config: service.apache_config.clone(),
        })
        .collect();
    options.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    options
}

fn error_view(
    services: Vec<ApacheServiceOption>,
    service_name: String,
    error: impl Into<String>,
) -> ApacheMappingScanView {
    ApacheMappingScanView {
        service_name,
        services,
        suggestions: Vec::new(),
        already_mapped: Vec::new(),
        warnings: Vec::new(),
        error: Some(error.into()),
    }
}

/// Scan configured Apache services/config and match document roots to apps.
pub(crate) fn detect_apache_mappings_at(
    paths: &ConfigPaths,
    service_name: Option<&str>,
) -> ApacheMappingScanView {
    let loaded = sm_config::load_services_lenient(&paths.services_path(), &paths.backup_dir());
    let services = apache_service_options(&loaded.services);
    let requested = service_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let Some(warning) = loaded.warning {
        return error_view(
            services,
            requested.unwrap_or_default(),
            format!("Could not read services.json: {warning}"),
        );
    }

    let target = match requested.as_deref() {
        Some(name) => match loaded
            .services
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(name))
        {
            Some(service) => {
                if service.service_type.normalized() != ServiceType::Apache {
                    return error_view(
                        services,
                        name.to_string(),
                        format!(
                            "Service '{name}' is not an Apache service (serviceType: {}).",
                            service.service_type.normalized().label()
                        ),
                    );
                }
                service
            }
            None => {
                return error_view(
                    services,
                    name.to_string(),
                    format!("Service '{name}' was not found in data/services.json."),
                )
            }
        },
        None if services.len() == 1 => loaded
            .services
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(&services[0].name))
            .expect("Apache option came from the loaded services"),
        None if services.is_empty() => {
            return error_view(
                services,
                String::new(),
                "No Apache service was found in data/services.json.",
            )
        }
        None => {
            let names = services
                .iter()
                .map(|service| service.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return error_view(
                services,
                String::new(),
                format!(
                    "Multiple Apache services found. Select one before detecting mappings: {names}"
                ),
            );
        }
    };

    let config = match load_config(&paths.projects_path()) {
        Ok(config) => config,
        Err(error) => {
            return error_view(
                services,
                target.name.clone(),
                format!("Could not read projects.json: {error}"),
            )
        }
    };
    let settings = match effective_settings(
        &settings_path_for(&paths.projects_path()),
        &config.settings,
        &config.trigger,
    ) {
        Ok(settings) => settings,
        Err(error) => {
            return error_view(
                services,
                target.name.clone(),
                format!("Could not load effective settings: {error}"),
            )
        }
    };

    let root = if target.apache_root.trim().is_empty() {
        settings.tools.apache.root.trim().to_string()
    } else {
        target.apache_root.trim().to_string()
    };
    let apache_config = if target.apache_config.trim().is_empty() {
        settings.tools.apache.config.trim().to_string()
    } else {
        target.apache_config.trim().to_string()
    };
    if root.is_empty() {
        return error_view(
            services,
            target.name.clone(),
            format!(
                "Apache service '{}' has no apacheRoot. Set it in Settings or the service config first.",
                target.name
            ),
        );
    }

    let parsed = parse_apache_config(Path::new(&root), &apache_config);
    let (suggestions, already_mapped) =
        build_mapping_suggestions(&parsed.document_roots, &config.apps);
    let mut warnings = parsed.warnings;
    if parsed.document_roots.is_empty() && warnings.is_empty() {
        warnings.push(format!(
            "No DocumentRoot directives were found in Apache config under {root}."
        ));
    } else if suggestions.is_empty() && !parsed.document_roots.is_empty() {
        warnings.push(
            "DocumentRoot entries were found, but no app localPath matched them.".to_string(),
        );
    }

    ApacheMappingScanView {
        service_name: target.name.clone(),
        services,
        suggestions,
        already_mapped,
        warnings,
        error: None,
    }
}

/// Backup `projects.json` to `backup/projects-<timestamp>.json`.
fn backup_projects_file(paths: &ConfigPaths) -> Result<Option<PathBuf>, String> {
    let projects = paths.projects_path();
    if !projects.is_file() {
        return Ok(None);
    }
    let backup_dir = paths.backup_dir();
    fs::create_dir_all(&backup_dir).map_err(|error| {
        format!(
            "Failed to create backup folder {}: {error}",
            backup_dir.display()
        )
    })?;

    let stamp = Local::now().format("%Y%m%d-%H%M%S-%3f").to_string();
    let mut destination = backup_dir.join(format!("projects-{stamp}.json"));
    let mut counter = 1;
    while destination.exists() {
        destination = backup_dir.join(format!("projects-{stamp}-{counter}.json"));
        counter += 1;
    }
    fs::copy(&projects, &destination).map_err(|error| {
        format!(
            "Failed to back up projects.json to {}: {error}",
            destination.display()
        )
    })?;
    Ok(Some(destination))
}

/// Apply explicit mapping selections. Only the selected app ids are changed;
/// `projects.json` is backed up before writing.
pub(crate) fn apply_apache_mappings_at(
    paths: &ConfigPaths,
    app_ids: &[String],
    service_name: &str,
) -> Result<Vec<UpdaterAppView>, String> {
    let service_name = service_name.trim();
    if service_name.is_empty() {
        return Err("Select an Apache service before applying mappings.".to_string());
    }
    if app_ids.is_empty() {
        return Err("Select at least one app to map.".to_string());
    }

    let loaded = sm_config::load_services_lenient(&paths.services_path(), &paths.backup_dir());
    if let Some(warning) = loaded.warning {
        return Err(format!("Could not read services.json: {warning}"));
    }
    let Some(service) = loaded
        .services
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(service_name))
    else {
        return Err(format!(
            "Apache service '{service_name}' was not found in data/services.json."
        ));
    };
    if service.service_type.normalized() != ServiceType::Apache {
        return Err(format!(
            "Service '{}' is not an Apache service (serviceType: {}).",
            service.name,
            service.service_type.normalized().label()
        ));
    }

    let mut config = load_config(&paths.projects_path())?;
    let mut requested = Vec::new();
    let mut seen = BTreeSet::new();
    for id in app_ids {
        let id = id.trim();
        if id.is_empty() {
            return Err("App id list contains an empty id.".to_string());
        }
        if seen.insert(id.to_ascii_lowercase()) {
            requested.push(id.to_string());
        }
    }
    for id in &requested {
        if config.find(id).is_none() {
            return Err(format!("App '{id}' was not found in projects.json."));
        }
    }

    backup_projects_file(paths)?;

    for id in &requested {
        let app = config
            .find_mut(id)
            .expect("app existence was validated before writing");
        app.web_server_service = service.name.clone();
        app.normalize();
    }
    save_config(&paths.projects_path(), &config)?;
    load_views_at(&paths.projects_path())
}

#[tauri::command]
pub async fn detect_apache_mappings(service_name: Option<String>) -> ApacheMappingScanView {
    let paths = ConfigPaths::detect();
    match tauri::async_runtime::spawn_blocking(move || {
        detect_apache_mappings_at(&paths, service_name.as_deref())
    })
    .await
    {
        Ok(view) => view,
        Err(error) => error_view(
            Vec::new(),
            String::new(),
            format!("Apache mapping detection failed: {error}"),
        ),
    }
}

#[tauri::command]
pub async fn apply_apache_mappings(
    app_ids: Vec<String>,
    service_name: String,
) -> Result<Vec<UpdaterAppView>, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        apply_apache_mappings_at(&paths, &app_ids, &service_name)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_updater::registry::UpdaterConfig;

    fn write_file(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn app(id: &str, local_path: &str) -> AppConfig {
        AppConfig {
            id: id.into(),
            name: id.into(),
            repo_url: "https://example.test/app.git".into(),
            local_path: local_path.into(),
            ..AppConfig::default()
        }
    }

    fn apache_service(name: &str, root: &Path) -> ServiceConfig {
        let mut service = ServiceConfig {
            name: name.into(),
            display_name: name.into(),
            service_type: ServiceType::Apache,
            executable: root.join(r"bin\httpd.exe").to_string_lossy().to_string(),
            apache_root: root.to_string_lossy().to_string(),
            apache_config: "conf/httpd.conf".into(),
            ..ServiceConfig::default()
        };
        service.normalize();
        service
    }

    #[test]
    fn parses_define_and_variable_document_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        let projects = temp.path().join("projects");
        let config = format!(
            "Define PROJECTS \"{}\"\nDefine SITE kasirin\nDocumentRoot \"${{PROJECTS}}/${{SITE}}/public\"\n",
            projects.to_string_lossy()
        );
        write_file(&root.join("conf/httpd.conf"), &config);

        let parsed = parse_apache_config(&root, "conf/httpd.conf");
        assert_eq!(parsed.document_roots.len(), 1, "{:?}", parsed.warnings);
        let expected = projects.join("kasirin").join("public");
        assert!(
            paths_equal(
                &parsed.document_roots[0].document_root,
                &expected.to_string_lossy()
            ),
            "{} != {}",
            parsed.document_roots[0].document_root,
            expected.display()
        );
        assert!(parsed.document_roots[0].config_file.ends_with("httpd.conf"));
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn parses_include_optional_wildcard_and_nested_include() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        let site_a = temp.path().join("site-a");
        let site_b = temp.path().join("site-b");
        write_file(
            &root.join("conf/httpd.conf"),
            &format!(
                "ServerRoot \"{}\"\nIncludeOptional \"conf/sites-enabled/*.conf\"\n",
                root.to_string_lossy()
            ),
        );
        write_file(
            &root.join("conf/sites-enabled/a.conf"),
            &format!(
                "Include \"../extra/nested.conf\"\nDocumentRoot \"{}\"\n",
                site_a.to_string_lossy()
            ),
        );
        write_file(
            &root.join("conf/sites-enabled/b.conf"),
            &format!("DocumentRoot \"{}\"\n", site_b.to_string_lossy()),
        );
        write_file(
            &root.join("conf/extra/nested.conf"),
            &format!(
                "DocumentRoot \"{}\"\n",
                site_a.join("public").to_string_lossy()
            ),
        );

        let parsed = parse_apache_config(&root, "");
        let roots: Vec<&str> = parsed
            .document_roots
            .iter()
            .map(|root| root.document_root.as_str())
            .collect();
        assert_eq!(roots.len(), 3, "{:?} {roots:?}", parsed.warnings);
        assert!(roots
            .iter()
            .any(|value| paths_equal(value, &site_a.to_string_lossy())));
        assert!(roots
            .iter()
            .any(|value| paths_equal(value, &site_b.to_string_lossy())));
        assert!(roots
            .iter()
            .any(|value| paths_equal(value, &site_a.join("public").to_string_lossy())));
        assert!(parsed
            .document_roots
            .iter()
            .any(|root| root.config_file.ends_with("nested.conf")));
    }

    #[test]
    fn resolves_relative_document_root_from_server_root_and_current_file_includes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        let site = root.join("htdocs/site");
        write_file(
            &root.join("conf/httpd.conf"),
            &format!(
                "ServerRoot \"{}\"\nDocumentRoot \"htdocs/site\"\nInclude \"extra/site.conf\"\n",
                root.to_string_lossy()
            ),
        );
        write_file(
            &root.join("conf/extra/site.conf"),
            &format!("DocumentRoot \"{}\"\n", site.to_string_lossy()),
        );

        let parsed = parse_apache_config(&root, "conf/httpd.conf");
        assert_eq!(parsed.document_roots.len(), 2, "{:?}", parsed.warnings);
        assert!(paths_equal(
            &parsed.document_roots[0].document_root,
            &site.to_string_lossy()
        ));
        assert!(parsed.document_roots[1].config_file.ends_with("site.conf"));
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn missing_required_include_warns_but_optional_does_not() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        write_file(
            &root.join("conf/httpd.conf"),
            "Include \"conf/missing.conf\"\nIncludeOptional \"conf/optional-*.conf\"\n",
        );

        let parsed = parse_apache_config(&root, "conf/httpd.conf");
        assert!(parsed.document_roots.is_empty());
        assert_eq!(parsed.warnings.len(), 1, "{:?}", parsed.warnings);
        assert!(parsed.warnings[0].contains("Include was not found"));
    }

    #[test]
    fn bounded_variable_expansion_reports_cycles() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        write_file(
            &root.join("conf/httpd.conf"),
            "Define A \"${B}\"\nDefine B \"${A}\"\nDocumentRoot \"${A}\"\n",
        );

        let parsed = parse_apache_config(&root, "conf/httpd.conf");
        assert!(parsed.document_roots.is_empty());
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.contains("depth limit")));
    }

    #[test]
    fn matches_exact_descendant_ancestor_and_prefers_exact() {
        let apps = vec![
            app("exact", r"d:/projects/EXACT/"),
            app("descendant", r"D:\Projects\descendant"),
            app("ancestor", r"D:\Other\ancestor\app"),
            app("none", r"E:\Nope"),
        ];
        let roots = [
            ApacheDocumentRoot {
                document_root: r"D:\Projects".into(),
                config_file: "parent.conf".into(),
            },
            ApacheDocumentRoot {
                document_root: r"D:\Projects\exact".into(),
                config_file: "exact.conf".into(),
            },
            ApacheDocumentRoot {
                document_root: r"D:\Other\ancestor".into(),
                config_file: "ancestor.conf".into(),
            },
            ApacheDocumentRoot {
                document_root: r"D:\Projects\descendant\public".into(),
                config_file: "descendant.conf".into(),
            },
        ];

        let (suggestions, already) = build_mapping_suggestions(&roots, &apps);
        assert!(already.is_empty());
        assert_eq!(suggestions.len(), 3);

        let exact = suggestions.iter().find(|s| s.app_id == "exact").unwrap();
        assert_eq!(exact.match_kind, "exact");
        assert_eq!(exact.confidence, "high");
        assert_eq!(exact.config_file, "exact.conf");

        let descendant = suggestions
            .iter()
            .find(|s| s.app_id == "descendant")
            .unwrap();
        assert_eq!(descendant.match_kind, "descendant");
        assert_eq!(descendant.confidence, "high");
        assert!(descendant.document_root.ends_with("public"));

        let ancestor = suggestions.iter().find(|s| s.app_id == "ancestor").unwrap();
        assert_eq!(ancestor.match_kind, "ancestor");
        assert_eq!(ancestor.confidence, "medium");
    }

    #[test]
    fn already_mapped_apps_are_separate_and_deduplicated_by_app_id() {
        let roots = vec![ApacheDocumentRoot {
            document_root: r"D:\Projects\mapped\public".into(),
            config_file: "vhost.conf".into(),
        }];
        let mut mapped = app("mapped", r"D:\Projects\mapped");
        mapped.web_server_service = "Apache2.4".into();
        let apps = vec![
            mapped.clone(),
            app("mapped", r"D:\Projects\mapped"),
            app("unmapped", r"D:\Projects\mapped"),
        ];

        let (suggestions, already) = build_mapping_suggestions(&roots, &apps);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].app_id, "unmapped");
        assert!(suggestions[0].existing_service.is_empty());

        assert_eq!(already.len(), 1);
        assert_eq!(already[0].app_id, "mapped");
        assert_eq!(already[0].existing_service, "Apache2.4");
        assert_eq!(already[0].match_kind, "descendant");
    }

    #[test]
    fn duplicate_app_ids_prefer_exact_or_descendant_over_ancestor() {
        let roots = vec![ApacheDocumentRoot {
            document_root: r"D:\Projects\dup".into(),
            config_file: "vhost.conf".into(),
        }];
        let apps = vec![
            app("dup", r"D:\Projects\dup\app"),
            app("dup", r"D:\Projects\dup"),
        ];

        let (suggestions, already) = build_mapping_suggestions(&roots, &apps);
        assert!(already.is_empty());
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].app_id, "dup");
        assert_eq!(suggestions[0].match_kind, "exact");
        assert_eq!(suggestions[0].local_path, r"D:\Projects\dup");
    }

    #[test]
    fn detect_matches_configured_document_root_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        let root = temp.path().join("apache");
        let site = temp.path().join("projects/kasirin");
        write_file(
            &root.join("conf/httpd.conf"),
            &format!("DocumentRoot \"{}/public\"\n", site.to_string_lossy()),
        );
        sm_config::save_services(
            &paths.services_path(),
            &paths.backup_dir(),
            &[apache_service("Apache2.4", &root)],
        )
        .unwrap();
        let config = UpdaterConfig {
            apps: vec![app("kasirin", &site.to_string_lossy())],
            ..UpdaterConfig::default()
        };
        save_config(&paths.projects_path(), &config).unwrap();

        let view = detect_apache_mappings_at(&paths, None);
        assert!(view.error.is_none(), "{:?}", view.error);
        assert_eq!(view.service_name, "Apache2.4");
        assert_eq!(view.suggestions.len(), 1, "{:?}", view.warnings);
        let suggestion = &view.suggestions[0];
        assert_eq!(suggestion.app_id, "kasirin");
        assert_eq!(suggestion.match_kind, "descendant");
        assert_eq!(suggestion.confidence, "high");
        assert!(paths_equal(
            &suggestion.document_root,
            &site.join("public").to_string_lossy()
        ));
        assert!(suggestion.config_file.ends_with("httpd.conf"));
    }

    #[test]
    fn detect_requires_service_selection_when_multiple_apache_services_exist() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        let services = vec![
            apache_service("ApacheA", &temp.path().join("a")),
            apache_service("ApacheB", &temp.path().join("b")),
        ];
        sm_config::save_services(&paths.services_path(), &paths.backup_dir(), &services).unwrap();
        save_config(&paths.projects_path(), &UpdaterConfig::default()).unwrap();

        let view = detect_apache_mappings_at(&paths, None);
        assert!(view.service_name.is_empty());
        assert_eq!(view.services.len(), 2);
        assert!(view
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Multiple Apache services"));
        assert!(view.suggestions.is_empty());
    }

    #[test]
    fn detect_rejects_missing_and_non_apache_service() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        let mut api = ServiceConfig {
            name: "api".into(),
            executable: r"C:\api\api.exe".into(),
            ..ServiceConfig::default()
        };
        api.normalize();
        sm_config::save_services(&paths.services_path(), &paths.backup_dir(), &[api]).unwrap();
        save_config(&paths.projects_path(), &UpdaterConfig::default()).unwrap();

        let view = detect_apache_mappings_at(&paths, Some("missing"));
        assert!(view.error.unwrap_or_default().contains("not found"));

        let view = detect_apache_mappings_at(&paths, Some("api"));
        assert!(view
            .error
            .unwrap_or_default()
            .contains("not an Apache service"));
    }

    fn setup_apply_fixture() -> (tempfile::TempDir, ConfigPaths) {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        let services = vec![apache_service("Apache2.4", &temp.path().join("apache"))];
        sm_config::save_services(&paths.services_path(), &paths.backup_dir(), &services).unwrap();

        let config = UpdaterConfig {
            apps: vec![
                app("one", r"D:\Projects\one"),
                app("two", r"D:\Projects\two"),
            ],
            ..UpdaterConfig::default()
        };
        save_config(&paths.projects_path(), &config).unwrap();
        (temp, paths)
    }

    #[test]
    fn apply_updates_only_selected_apps_and_creates_backup() {
        let (_temp, paths) = setup_apply_fixture();
        let before = fs::read_to_string(paths.projects_path()).unwrap();

        let views = apply_apache_mappings_at(&paths, &["one".to_string()], "apache2.4").unwrap();
        let one = views.iter().find(|view| view.id == "one").unwrap();
        let two = views.iter().find(|view| view.id == "two").unwrap();
        assert_eq!(one.web_server_service, "Apache2.4");
        assert!(two.web_server_service.is_empty());

        let config = load_config(&paths.projects_path()).unwrap();
        assert_eq!(config.find("one").unwrap().web_server_service, "Apache2.4");
        assert_eq!(config.find("two").unwrap().web_server_service, "");

        let backups: Vec<_> = fs::read_dir(paths.backup_dir())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("projects-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(backups[0].path()).unwrap(),
            before,
            "backup must contain the pre-apply projects.json"
        );
    }

    #[test]
    fn apply_rejects_missing_service_wrong_service_and_unknown_app() {
        let (_temp, paths) = setup_apply_fixture();

        let error = apply_apache_mappings_at(&paths, &["one".to_string()], "missing").unwrap_err();
        assert!(error.contains("not found"));

        let mut api = ServiceConfig {
            name: "api".into(),
            executable: r"C:\api\api.exe".into(),
            ..ServiceConfig::default()
        };
        api.normalize();
        let mut services =
            sm_config::load_services_strict(&paths.services_path(), &paths.backup_dir()).unwrap();
        services.push(api);
        sm_config::save_services(&paths.services_path(), &paths.backup_dir(), &services).unwrap();

        let error = apply_apache_mappings_at(&paths, &["one".to_string()], "api").unwrap_err();
        assert!(error.contains("not an Apache service"));

        let error =
            apply_apache_mappings_at(&paths, &["unknown".to_string()], "Apache2.4").unwrap_err();
        assert!(error.contains("was not found in projects.json"));

        assert!(fs::read_dir(paths.backup_dir())
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().starts_with("projects-")));
    }
}
