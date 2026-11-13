use std::path::{Path, PathBuf};

use sm_config::ProjectsFile;
use sm_core::ProjectOverride;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectInfo {
    pub name: String,
    pub path: PathBuf,
    pub config: ProjectOverride,
}

pub fn is_laravel_project(path: &Path) -> bool {
    path.is_dir()
        && path.join(".git").is_dir()
        && path.join("artisan").is_file()
        && path.join("composer.json").is_file()
}

pub fn discover_projects(settings: &ProjectsFile) -> Vec<ProjectInfo> {
    let mut projects: Vec<ProjectInfo> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    for root in &settings.scan_roots {
        let root = Path::new(root);
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !is_laravel_project(&path) {
                continue;
            }
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
            else {
                continue;
            };
            let key = name.to_ascii_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let config = settings.projects.get(&name).cloned().unwrap_or_default();
            projects.push(ProjectInfo { name, path, config });
        }
    }

    for (name, config) in &settings.projects {
        let key = name.to_ascii_lowercase();
        if seen.contains(&key) || config.path.trim().is_empty() {
            continue;
        }
        let path = PathBuf::from(config.path.trim());
        if !is_laravel_project(&path) {
            continue;
        }
        seen.push(key);
        projects.push(ProjectInfo {
            name: name.clone(),
            path,
            config: config.clone(),
        });
    }

    projects.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    projects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_project(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::create_dir_all(path.join(".git")).unwrap();
        std::fs::write(path.join("artisan"), b"#!/usr/bin/env php").unwrap();
        std::fs::write(path.join("composer.json"), b"{}").unwrap();
        path
    }

    fn settings(root: &Path) -> ProjectsFile {
        ProjectsFile {
            scan_roots: vec![root.to_string_lossy().to_string()],
            ..ProjectsFile::default()
        }
    }

    #[test]
    fn detects_laravel_projects() {
        let temp = tempfile::tempdir().unwrap();
        let project = make_project(temp.path(), "kasirin");
        assert!(is_laravel_project(&project));

        let not_laravel = temp.path().join("plain");
        std::fs::create_dir_all(&not_laravel).unwrap();
        assert!(!is_laravel_project(&not_laravel));
    }

    #[test]
    fn discovers_and_sorts_projects() {
        let temp = tempfile::tempdir().unwrap();
        make_project(temp.path(), "zeta");
        make_project(temp.path(), "alpha");
        std::fs::create_dir_all(temp.path().join("not-a-project")).unwrap();

        let projects = discover_projects(&settings(temp.path()));
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "alpha");
        assert_eq!(projects[1].name, "zeta");
    }

    #[test]
    fn applies_project_override() {
        let temp = tempfile::tempdir().unwrap();
        make_project(temp.path(), "kasirin");
        let mut projects = settings(temp.path());
        projects.projects.insert(
            "kasirin".into(),
            ProjectOverride {
                branch: "develop".into(),
                enabled: false,
                ..ProjectOverride::default()
            },
        );

        let discovered = discover_projects(&projects);
        assert_eq!(discovered[0].config.branch, "develop");
        assert!(!discovered[0].config.enabled);
    }

    #[test]
    fn adds_manual_project_outside_scan_roots() {
        let temp = tempfile::tempdir().unwrap();
        let other = temp.path().join("elsewhere");
        std::fs::create_dir_all(&other).unwrap();
        let project = make_project(&other, "manual");

        let mut projects = ProjectsFile {
            scan_roots: vec![temp.path().join("empty").to_string_lossy().to_string()],
            ..ProjectsFile::default()
        };
        projects.projects.insert(
            "manual".into(),
            ProjectOverride {
                path: project.to_string_lossy().to_string(),
                ..ProjectOverride::default()
            },
        );

        let discovered = discover_projects(&projects);
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].name, "manual");
    }

    #[test]
    fn missing_scan_root_is_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let projects = ProjectsFile {
            scan_roots: vec![temp.path().join("missing").to_string_lossy().to_string()],
            ..ProjectsFile::default()
        };
        assert!(discover_projects(&projects).is_empty());
    }
}
