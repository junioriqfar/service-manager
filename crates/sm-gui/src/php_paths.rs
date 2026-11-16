//! Detection and explicit replacement of hardcoded PHP paths in app commands.
//!
//! Saving Settings only *reports* hardcoded PHP paths. `projects.json` is
//! changed only when the user explicitly calls `replace_php_references`, and
//! the file is backed up first.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use chrono::Local;
use serde::Serialize;
use sm_config::ConfigPaths;
use sm_updater::registry::{load_config, save_config, AppConfig, CommandSpec, UpdaterConfig};

use crate::updater::{load_views_at, UpdaterAppView};

/// One command field that references the old PHP path or root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpReferenceView {
    pub app_id: String,
    pub app_name: String,
    pub field: String,
    pub value: String,
}

/// Preview returned after Save Settings when PHP path/root changed and
/// existing command strings still contain the old value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhpPathChangeView {
    pub old_path: String,
    pub old_root: String,
    pub new_path: String,
    pub new_root: String,
    pub references: Vec<PhpReferenceView>,
}

/// Normalize Windows paths for matching only. Matching is case-insensitive
/// and treats `/` and `\` as the same separator; the original string is kept
/// when applying replacements.
fn normalize_match(value: &str) -> String {
    value.replace('\\', "/").to_ascii_lowercase()
}

/// PHP root is the parent folder of the PHP executable (`PHP_HOME`).
pub(crate) fn php_home(path: &str) -> String {
    sm_updater::registry::tool_home(path)
}

pub(crate) fn php_path_changed(old_path: &str, new_path: &str) -> bool {
    normalize_match(old_path.trim()) != normalize_match(new_path.trim())
}

fn uses_php_variable(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    upper.contains("${PHP}") || upper.contains("${PHP_HOME}")
}

fn contains_old_php(value: &str, old_path: &str, old_root: &str) -> bool {
    if value.trim().is_empty() || uses_php_variable(value) {
        return false;
    }
    let normalized = normalize_match(value);
    [old_path, old_root]
        .iter()
        .map(|pattern| pattern.trim())
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| normalized.contains(&normalize_match(pattern)))
}

fn push_reference(
    references: &mut Vec<PhpReferenceView>,
    app: &AppConfig,
    field: impl Into<String>,
    value: &str,
    old_path: &str,
    old_root: &str,
) {
    if contains_old_php(value, old_path, old_root) {
        references.push(PhpReferenceView {
            app_id: app.id.clone(),
            app_name: if app.name.trim().is_empty() {
                app.id.clone()
            } else {
                app.name.clone()
            },
            field: field.into(),
            value: value.to_string(),
        });
    }
}

fn scan_spec(
    references: &mut Vec<PhpReferenceView>,
    app: &AppConfig,
    prefix: &str,
    spec: &CommandSpec,
    old_path: &str,
    old_root: &str,
) {
    push_reference(
        references,
        app,
        format!("{prefix}.program"),
        &spec.program,
        old_path,
        old_root,
    );
    for (index, arg) in spec.args.iter().enumerate() {
        push_reference(
            references,
            app,
            format!("{prefix}.args[{index}]"),
            arg,
            old_path,
            old_root,
        );
    }
    push_reference(
        references,
        app,
        format!("{prefix}.cwd"),
        &spec.cwd,
        old_path,
        old_root,
    );
    push_reference(
        references,
        app,
        format!("{prefix}.shell"),
        &spec.shell,
        old_path,
        old_root,
    );
    for (key, value) in &spec.env {
        push_reference(
            references,
            app,
            format!("{prefix}.env.{key}"),
            value,
            old_path,
            old_root,
        );
    }
}

/// Scan command-related app fields for `old_path` or `old_root`.
///
/// Fields that already use `${PHP}`/`${PHP_HOME}` are skipped. The result is
/// ordered by app and field, matching the structure of `projects.json`.
pub(crate) fn scan_php_references(
    config: &UpdaterConfig,
    old_path: &str,
    old_root: &str,
) -> Vec<PhpReferenceView> {
    let old_path = old_path.trim();
    let old_root = old_root.trim();
    let mut references = Vec::new();
    if old_path.is_empty() && old_root.is_empty() {
        return references;
    }

    for app in &config.apps {
        push_reference(
            &mut references,
            app,
            "installCommand",
            &app.install_command,
            old_path,
            old_root,
        );
        push_reference(
            &mut references,
            app,
            "buildCommand",
            &app.build_command,
            old_path,
            old_root,
        );
        push_reference(
            &mut references,
            app,
            "preUpdateCommand",
            &app.pre_update_command,
            old_path,
            old_root,
        );
        push_reference(
            &mut references,
            app,
            "postUpdateCommand",
            &app.post_update_command,
            old_path,
            old_root,
        );
        for (group, specs) in [
            ("commands.install", app.commands.install.as_slice()),
            ("commands.build", app.commands.build.as_slice()),
            ("commands.preUpdate", app.commands.pre_update.as_slice()),
            ("commands.postUpdate", app.commands.post_update.as_slice()),
        ] {
            for (index, spec) in specs.iter().enumerate() {
                scan_spec(
                    &mut references,
                    app,
                    &format!("{group}[{index}]"),
                    spec,
                    old_path,
                    old_root,
                );
            }
        }
    }
    references
}

/// Replace all case/slash-insensitive occurrences of the old path/root.
/// Matching uses normalized text, but the remainder of the original value is
/// preserved byte-for-byte.
fn replace_all(value: &str, patterns: &[(String, String)]) -> (String, bool) {
    let prepared: Vec<(String, &str)> = patterns
        .iter()
        .filter(|(old, _)| !old.trim().is_empty())
        .map(|(old, new)| (normalize_match(old.trim()), new.as_str()))
        .collect();
    if prepared.is_empty() || value.is_empty() {
        return (value.to_string(), false);
    }

    let haystack = normalize_match(value);
    let mut output = String::with_capacity(value.len());
    let mut last = 0;
    let mut search = 0;
    let mut changed = false;

    while search < haystack.len() {
        let mut best: Option<(usize, usize, &str)> = None;
        for (needle, replacement) in &prepared {
            if needle.is_empty() {
                continue;
            }
            let Some(relative) = haystack[search..].find(needle.as_str()) else {
                continue;
            };
            let start = search + relative;
            let end = start + needle.len();
            let replace = match best {
                None => true,
                Some((best_start, best_end, _)) => {
                    start < best_start || (start == best_start && end > best_end)
                }
            };
            if replace {
                best = Some((start, end, replacement));
            }
        }
        let Some((start, end, replacement)) = best else {
            break;
        };
        output.push_str(&value[last..start]);
        output.push_str(replacement);
        last = end;
        search = end;
        changed = true;
    }

    if !changed {
        return (value.to_string(), false);
    }
    output.push_str(&value[last..]);
    (output, true)
}

fn replace_field(value: &mut String, patterns: &[(String, String)]) -> bool {
    let (next, changed) = replace_all(value, patterns);
    if changed {
        *value = next;
    }
    changed
}

fn replace_spec(spec: &mut CommandSpec, patterns: &[(String, String)]) -> usize {
    let mut changed = 0;
    changed += usize::from(replace_field(&mut spec.program, patterns));
    for arg in &mut spec.args {
        changed += usize::from(replace_field(arg, patterns));
    }
    changed += usize::from(replace_field(&mut spec.cwd, patterns));
    changed += usize::from(replace_field(&mut spec.shell, patterns));
    for value in spec.env.values_mut() {
        changed += usize::from(replace_field(value, patterns));
    }
    changed
}

fn replace_app(app: &mut AppConfig, patterns: &[(String, String)]) -> usize {
    let mut changed = 0;
    changed += usize::from(replace_field(&mut app.install_command, patterns));
    changed += usize::from(replace_field(&mut app.build_command, patterns));
    changed += usize::from(replace_field(&mut app.pre_update_command, patterns));
    changed += usize::from(replace_field(&mut app.post_update_command, patterns));
    for group in [
        &mut app.commands.install,
        &mut app.commands.build,
        &mut app.commands.pre_update,
        &mut app.commands.post_update,
    ] {
        for spec in group {
            changed += replace_spec(spec, patterns);
        }
    }
    if changed > 0 {
        app.normalize();
    }
    changed
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

fn has_control_characters(value: &str) -> bool {
    value.chars().any(char::is_control)
}

/// Rewrite hardcoded PHP references in the selected apps.
///
/// `mode=variable` replaces the old PHP path with `${PHP}` and the old root
/// with `${PHP_HOME}`. `mode=new_path` uses the supplied new path/root.
pub(crate) fn replace_php_references_at(
    paths: &ConfigPaths,
    app_ids: &[String],
    old_path: &str,
    old_root: &str,
    new_path: &str,
    new_root: &str,
    mode: &str,
) -> Result<Vec<UpdaterAppView>, String> {
    let mode = mode.trim().to_ascii_lowercase();
    if mode != "variable" && mode != "new_path" {
        return Err(format!(
            "Unsupported PHP reference mode '{mode}'. Use 'variable' or 'new_path'."
        ));
    }
    if app_ids.is_empty() {
        return Err("Select at least one app with PHP references.".to_string());
    }

    let old_path = old_path.trim();
    let old_root = {
        let value = old_root.trim();
        if value.is_empty() {
            php_home(old_path)
        } else {
            value.to_string()
        }
    };
    if old_path.is_empty() && old_root.is_empty() {
        return Err("Old PHP path/root is empty; nothing to replace.".to_string());
    }

    let new_path = new_path.trim();
    let new_root = {
        let value = new_root.trim();
        if value.is_empty() {
            php_home(new_path)
        } else {
            value.to_string()
        }
    };
    if mode == "new_path" && new_path.is_empty() {
        return Err("New PHP path is required when replacing with the new path.".to_string());
    }
    if mode == "new_path" && !old_root.is_empty() && new_root.is_empty() {
        return Err(
            "New PHP root is required when replacing the old PHP root with the new path."
                .to_string(),
        );
    }
    for (field, value) in [
        ("oldPath", old_path),
        ("oldRoot", old_root.as_str()),
        ("newPath", new_path),
        ("newRoot", new_root.as_str()),
    ] {
        if has_control_characters(value) {
            return Err(format!(
                "PHP reference field '{field}' contains control characters."
            ));
        }
    }

    let patterns: Vec<(String, String)> = if mode == "variable" {
        vec![
            (old_path.to_string(), "${PHP}".to_string()),
            (old_root.clone(), "${PHP_HOME}".to_string()),
        ]
    } else {
        vec![
            (old_path.to_string(), new_path.to_string()),
            (old_root.clone(), new_root.clone()),
        ]
    };

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

    let mut changed = 0;
    for id in &requested {
        let app = config
            .find_mut(id)
            .expect("app existence was validated before writing");
        changed += replace_app(app, &patterns);
    }
    if changed == 0 {
        return Err("No hardcoded PHP references were found in the selected apps.".to_string());
    }

    backup_projects_file(paths)?;
    save_config(&paths.projects_path(), &config)?;
    load_views_at(&paths.projects_path())
}

#[tauri::command]
pub async fn replace_php_references(
    app_ids: Vec<String>,
    old_path: String,
    old_root: String,
    new_path: String,
    new_root: String,
    mode: String,
) -> Result<Vec<UpdaterAppView>, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        replace_php_references_at(
            &paths, &app_ids, &old_path, &old_root, &new_path, &new_root, &mode,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_updater::registry::{CustomCommands, Preset, UpdaterConfig};

    fn old_path() -> &'static str {
        r"C:\laragon\bin\php\php-8.3.16\php.exe"
    }

    fn old_root() -> &'static str {
        r"C:\laragon\bin\php\php-8.3.16"
    }

    fn command(program: &str) -> CommandSpec {
        CommandSpec {
            name: "custom".into(),
            program: program.into(),
            args: vec![format!(r"{}\artisan", old_root())],
            cwd: old_root().into(),
            env: [("PATH".to_string(), format!(r"{}\bin", old_root()))]
                .into_iter()
                .collect(),
            ..CommandSpec::default()
        }
    }

    fn config_with_apps() -> UpdaterConfig {
        let mut config = UpdaterConfig::default();
        let mut one = AppConfig {
            id: "one".into(),
            name: "One".into(),
            repo_url: "https://example.com/one.git".into(),
            local_path: r"C:\project\one".into(),
            preset: Preset::Custom,
            install_command: format!(r#""{}" artisan migrate"#, old_path()),
            build_command: "npm run build".into(),
            pre_update_command: format!(r#""{0}\php.exe" artisan down"#, old_root()),
            post_update_command: format!(r#""{0}\php.exe" artisan up"#, old_root()),
            ..AppConfig::default()
        };
        one.commands = CustomCommands {
            install: vec![command(old_path())],
            build: vec![CommandSpec {
                name: "shell".into(),
                shell: format!(r#""{}" artisan optimize"#, old_path()),
                ..CommandSpec::default()
            }],
            pre_update: vec![CommandSpec {
                name: "skip".into(),
                program: "${PHP}".into(),
                args: vec!["${PHP_HOME}\\artisan".into()],
                ..CommandSpec::default()
            }],
            post_update: vec![],
        };

        let mut two = AppConfig {
            id: "two".into(),
            name: "Two".into(),
            repo_url: "https://example.com/two.git".into(),
            local_path: r"C:\project\two".into(),
            preset: Preset::Static,
            install_command: format!(r#""{}" artisan migrate"#, old_path()),
            ..AppConfig::default()
        };
        two.commands.install = vec![command(old_path())];
        config.apps = vec![one, two];
        config
    }

    #[test]
    fn scans_top_level_and_custom_command_fields() {
        let config = config_with_apps();
        let references = scan_php_references(&config, old_path(), old_root());

        let fields: Vec<&str> = references
            .iter()
            .filter(|reference| reference.app_id == "one")
            .map(|reference| reference.field.as_str())
            .collect();
        for expected in [
            "installCommand",
            "preUpdateCommand",
            "postUpdateCommand",
            "commands.install[0].program",
            "commands.install[0].args[0]",
            "commands.install[0].cwd",
            "commands.install[0].env.PATH",
            "commands.build[0].shell",
        ] {
            assert!(
                fields.contains(&expected),
                "missing field {expected}: {fields:?}"
            );
        }
        assert!(
            !fields
                .iter()
                .any(|field| field.starts_with("commands.preUpdate")),
            "fields using ${{PHP}} must be skipped"
        );
        assert!(
            !fields.contains(&"buildCommand"),
            "buildCommand without the old path must not be reported"
        );
        assert_eq!(
            references
                .iter()
                .filter(|reference| reference.app_id == "two")
                .count(),
            5,
            "second app has installCommand plus program/args/cwd/env"
        );
    }

    #[test]
    fn scan_is_case_and_slash_insensitive() {
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            install_command: r#""c:/LARAGON\bin\PHP\php-8.3.16\PHP.EXE" artisan migrate"#.into(),
            ..AppConfig::default()
        });

        let references = scan_php_references(&config, old_path(), old_root());
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].field, "installCommand");
    }

    #[test]
    fn replaces_with_variables_preserving_rest_of_value() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        save_config(&paths.projects_path(), &config_with_apps()).unwrap();

        let views = replace_php_references_at(
            &paths,
            &["one".to_string()],
            old_path(),
            old_root(),
            r"C:\php-new\php.exe",
            r"C:\php-new",
            "variable",
        )
        .unwrap();

        let loaded = load_config(&paths.projects_path()).unwrap();
        let app = loaded.find("one").unwrap();
        assert!(app.install_command.contains("${PHP}"));
        assert_eq!(app.pre_update_command, r#""${PHP}" artisan down"#);
        assert_eq!(app.post_update_command, r#""${PHP}" artisan up"#);
        assert_eq!(app.commands.install[0].program, "${PHP}");
        assert_eq!(app.commands.install[0].args[0], r"${PHP_HOME}\artisan");
        assert_eq!(app.commands.install[0].cwd, "${PHP_HOME}");
        assert_eq!(app.commands.install[0].env["PATH"], r"${PHP_HOME}\bin");
        assert!(app.commands.build[0].shell.contains("${PHP}"));
        assert_eq!(
            app.commands.pre_update[0].program, "${PHP}",
            "already-variable fields stay unchanged"
        );

        let other = loaded.find("two").unwrap();
        assert!(
            other.install_command.contains(old_path()),
            "unselected apps must not change"
        );
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].id, "one");
    }

    #[test]
    fn replaces_with_new_path_and_creates_backup() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        save_config(&paths.projects_path(), &config_with_apps()).unwrap();
        let before = std::fs::read_to_string(paths.projects_path()).unwrap();

        replace_php_references_at(
            &paths,
            &["one".to_string()],
            old_path(),
            old_root(),
            r"D:\php\php.exe",
            r"D:\php",
            "new_path",
        )
        .unwrap();

        let loaded = load_config(&paths.projects_path()).unwrap();
        let app = loaded.find("one").unwrap();
        assert_eq!(app.commands.install[0].program, r"D:\php\php.exe");
        assert_eq!(app.commands.install[0].args[0], r"D:\php\artisan");
        assert_eq!(app.commands.install[0].env["PATH"], r"D:\php\bin");

        let backups: Vec<_> = std::fs::read_dir(paths.backup_dir())
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(backups[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("projects-"));
        assert_eq!(std::fs::read_to_string(&backups[0]).unwrap(), before);
    }

    #[test]
    fn validates_mode_ids_paths_and_missing_apps() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        save_config(&paths.projects_path(), &config_with_apps()).unwrap();

        assert!(replace_php_references_at(
            &paths,
            &["one".to_string()],
            old_path(),
            old_root(),
            r"C:\php\php.exe",
            r"C:\php",
            "other",
        )
        .unwrap_err()
        .contains("Unsupported"));

        assert!(replace_php_references_at(
            &paths,
            &[],
            old_path(),
            old_root(),
            r"C:\php\php.exe",
            r"C:\php",
            "variable",
        )
        .unwrap_err()
        .contains("at least one"));

        assert!(replace_php_references_at(
            &paths,
            &["missing".to_string()],
            old_path(),
            old_root(),
            r"C:\php\php.exe",
            r"C:\php",
            "variable",
        )
        .unwrap_err()
        .contains("not found"));

        assert!(replace_php_references_at(
            &paths,
            &["one".to_string()],
            old_path(),
            old_root(),
            "",
            "",
            "new_path",
        )
        .unwrap_err()
        .contains("New PHP path"));
    }

    #[test]
    fn reports_when_nothing_matches() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: r"C:\demo".into(),
            ..AppConfig::default()
        });
        save_config(&paths.projects_path(), &config).unwrap();

        let error = replace_php_references_at(
            &paths,
            &["demo".to_string()],
            old_path(),
            old_root(),
            r"C:\php\php.exe",
            r"C:\php",
            "variable",
        )
        .unwrap_err();
        assert!(error.contains("No hardcoded PHP references"));
    }
}
