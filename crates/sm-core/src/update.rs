use crate::model::{ProjectOverride, ToggleMode};

/// Rencana langkah lanjutan setelah `git merge --ff-only`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UpdatePlan {
    pub composer_install: bool,
    pub npm_ci: bool,
    pub npm_install: bool,
    pub npm_build: bool,
    pub optimize_clear: bool,
    pub queue_restart: bool,
}

fn normalized(path: &str) -> String {
    path.trim().replace('\\', "/")
}

pub fn is_migration_change(path: &str) -> bool {
    normalized(path).starts_with("database/migrations/")
}

pub fn is_code_change(path: &str) -> bool {
    let path = normalized(path);
    path.starts_with("app/")
        || path.starts_with("bootstrap/")
        || path.starts_with("config/")
        || path.starts_with("database/")
        || path.starts_with("lang/")
        || path.starts_with("resources/")
        || path.starts_with("routes/")
        || matches!(
            path.as_str(),
            "composer.json" | "composer.lock" | "package.json" | "package-lock.json"
        )
}

pub fn has_migration_changes(changed: &[String]) -> bool {
    changed.iter().any(|path| is_migration_change(path))
}

/// Hitung langkah yang perlu dijalankan berdasarkan file yang berubah.
pub fn plan_update(
    changed: &[String],
    project: &ProjectOverride,
    has_package_lock: bool,
    cache_files_exist: bool,
) -> UpdatePlan {
    let changed_files: Vec<String> = changed.iter().map(|path| normalized(path)).collect();
    let composer_changed = changed_files
        .iter()
        .any(|path| matches!(path.as_str(), "composer.json" | "composer.lock"));
    let package_lock_changed = changed_files
        .iter()
        .any(|path| path.as_str() == "package-lock.json");
    let package_changed = changed_files
        .iter()
        .any(|path| path.as_str() == "package.json");
    let frontend_changed = changed_files.iter().any(|path| {
        path.starts_with("resources/")
            || matches!(
                path.as_str(),
                "vite.config.ts"
                    | "vite.config.js"
                    | "tsconfig.json"
                    | "package.json"
                    | "package-lock.json"
            )
    });

    let composer_mode = project.composer.normalized();
    let build_mode = project.build.normalized();

    let composer_install = match composer_mode {
        ToggleMode::Always => true,
        ToggleMode::Auto => composer_changed,
        ToggleMode::Never | ToggleMode::Unknown => false,
    };

    let npm_build = match build_mode {
        ToggleMode::Always => true,
        ToggleMode::Auto => frontend_changed,
        ToggleMode::Never | ToggleMode::Unknown => false,
    };

    let npm_ci = npm_build
        && (package_lock_changed || (build_mode == ToggleMode::Always && has_package_lock));
    let npm_install = npm_build && !npm_ci && (package_changed || build_mode == ToggleMode::Always);
    let queue_restart =
        project.queue_restart && changed_files.iter().any(|path| is_code_change(path));

    UpdatePlan {
        composer_install,
        npm_ci,
        npm_install,
        npm_build,
        optimize_clear: cache_files_exist,
        queue_restart,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectOverride {
        ProjectOverride::default()
    }

    fn files(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn composer_runs_only_when_composer_changed_in_auto_mode() {
        let plan = plan_update(
            &files(&["app/Http/Controllers/Foo.php"]),
            &project(),
            false,
            false,
        );
        assert!(!plan.composer_install);
        assert!(!plan.npm_build);

        let plan = plan_update(
            &files(&["composer.lock", "app/Foo.php"]),
            &project(),
            false,
            false,
        );
        assert!(plan.composer_install);
    }

    #[test]
    fn composer_always_runs_even_without_changes() {
        let mut project = project();
        project.composer = ToggleMode::Always;
        let plan = plan_update(&files(&["README.md"]), &project, false, false);
        assert!(plan.composer_install);
    }

    #[test]
    fn package_lock_uses_npm_ci_and_build() {
        let plan = plan_update(&files(&["package-lock.json"]), &project(), true, false);
        assert!(plan.npm_ci);
        assert!(!plan.npm_install);
        assert!(plan.npm_build);
    }

    #[test]
    fn package_json_only_uses_npm_install() {
        let plan = plan_update(&files(&["package.json"]), &project(), true, false);
        assert!(!plan.npm_ci);
        assert!(plan.npm_install);
        assert!(plan.npm_build);
    }

    #[test]
    fn resources_change_triggers_build_without_install() {
        let plan = plan_update(&files(&["resources/js/app.ts"]), &project(), false, false);
        assert!(plan.npm_build);
        assert!(!plan.npm_ci);
        assert!(!plan.npm_install);
    }

    #[test]
    fn build_always_without_lock_uses_npm_install() {
        let mut project = project();
        project.build = ToggleMode::Always;
        let plan = plan_update(&files(&["README.md"]), &project, false, false);
        assert!(plan.npm_build);
        assert!(!plan.npm_ci);
        assert!(plan.npm_install);

        let plan = plan_update(&files(&["README.md"]), &project, true, false);
        assert!(plan.npm_ci);
        assert!(!plan.npm_install);
    }

    #[test]
    fn build_never_disables_frontend_steps() {
        let mut project = project();
        project.build = ToggleMode::Never;
        let plan = plan_update(&files(&["package-lock.json"]), &project, true, false);
        assert!(!plan.npm_build);
        assert!(!plan.npm_ci);
        assert!(!plan.npm_install);
    }

    #[test]
    fn queue_restart_only_for_code_changes_and_enabled() {
        let plan = plan_update(&files(&["routes/web.php"]), &project(), false, false);
        assert!(plan.queue_restart);

        let plan = plan_update(&files(&["README.md"]), &project(), false, false);
        assert!(!plan.queue_restart);

        let mut project = project();
        project.queue_restart = false;
        let plan = plan_update(&files(&["routes/web.php"]), &project, false, false);
        assert!(!plan.queue_restart);
    }

    #[test]
    fn optimize_clear_depends_on_cache_presence() {
        let plan = plan_update(&files(&[]), &project(), false, true);
        assert!(plan.optimize_clear);
    }

    #[test]
    fn migration_detection_is_path_aware() {
        assert!(is_migration_change(
            "database/migrations/2026_01_01_create_users.php"
        ));
        assert!(is_migration_change(
            "database\\migrations\\2026_01_01_create_users.php"
        ));
        assert!(!is_migration_change("database/seeders/DatabaseSeeder.php"));
        assert!(has_migration_changes(&files(&[
            "app/Foo.php",
            "database/migrations/2026_01_01_x.php"
        ])));
    }

    #[test]
    fn code_change_detection_matches_powershell_rules() {
        for path in [
            "app/Models/User.php",
            "bootstrap/app.php",
            "config/app.php",
            "database/seeders/DatabaseSeeder.php",
            "lang/en/messages.php",
            "resources/js/app.ts",
            "routes/web.php",
            "composer.json",
            "package.json",
        ] {
            assert!(is_code_change(path), "{path} should be a code change");
        }

        for path in ["README.md", "public/index.php", "storage/logs/laravel.log"] {
            assert!(!is_code_change(path), "{path} should not be a code change");
        }
    }
}
