//! Public runtime assets shared by interactive onboarding and offline repair.
//! No configuration, credentials, databases, provider calls or activation changes.
use anyhow::Context as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

struct Asset {
    path: &'static str,
    bytes: &'static [u8],
    executable: bool,
}

macro_rules! asset {
    ($path:literal) => {
        Asset {
            path: $path,
            bytes: include_bytes!(concat!("../", $path)),
            executable: false,
        }
    };
    ($path:literal, executable) => {
        Asset {
            path: $path,
            bytes: include_bytes!(concat!("../", $path)),
            executable: true,
        }
    };
}

// Deliberate allow-list: never recursively bundle arbitrary user files/secrets.
// A coverage regression guards all shipped POML/JSON/SM/skill dependencies.
const BUNDLED_ASSETS: &[Asset] = &[
    asset!("contexts/standard.sm"),
    asset!("contexts/default.sm"),
    asset!("contexts/chat.sm"),
    asset!("contexts/coding.sm"),
    asset!("contexts/self_learning.sm"),
    asset!("templates/standard.poml"),
    asset!("templates/language_instructor.poml"),
    asset!("templates/code_assistant.poml"),
    asset!("templates/researcher.poml"),
    asset!("templates/system.poml"),
    asset!("templates/language_learning.poml"),
    asset!("templates/user.poml"),
    asset!("templates/compaction.poml"),
    asset!("templates/blueprints/standard.json"),
    asset!("templates/blueprints/language_instructor.json"),
    asset!("templates/blueprints/code_assistant.json"),
    asset!("templates/blueprints/researcher.json"),
    asset!("templates/shared/blueprint.poml"),
    asset!("templates/shared/runtime.poml"),
    asset!("templates/shared/task_inputs.poml"),
    asset!("templates/roles/senior_dev.poml"),
    asset!("templates/roles/technical_writer.poml"),
    asset!("templates/roles/researcher.poml"),
    asset!("templates/tasks/plan.poml"),
    asset!("templates/tasks/code.poml"),
    asset!("templates/tasks/code_review.poml"),
    asset!("templates/tasks/test.poml"),
    asset!("templates/tasks/review.poml"),
    asset!("templates/tasks/feedback.poml"),
    asset!("templates/tasks/done.poml"),
    asset!("skills/code_review/skill.json"),
    asset!("skills/code_review/skill.poml"),
    asset!("skills/debug/skill.json"),
    asset!("skills/debug/skill.poml"),
    asset!("skills/tmux/skill.json"),
    asset!("skills/tmux/skill.poml"),
    asset!("skills/tmux/scripts/create_session.sh", executable),
    asset!("skills/tmux/scripts/kill_session.sh", executable),
    asset!("skills/tmux/scripts/list_sessions.sh", executable),
    asset!("skills/poml_templates/skill.json"),
    asset!("skills/poml_templates/skill.poml"),
    asset!("skills/poml_templates/reference.md"),
    asset!("skills/poml_templates/examples/defaults.json"),
    asset!("skills/poml_templates/examples/footer.poml"),
    asset!("skills/poml_templates/examples/starter.poml"),
    asset!("skills/poml_templates/scripts/validate.py", executable),
    asset!("static/index.html"),
    asset!("static/style.css"),
    asset!("static/app.js"),
    asset!("static/logo.svg"),
    asset!("static/logo.png"),
    asset!("static/favicon.ico"),
    asset!("static/favicon.png"),
    asset!("static/apple-touch-icon.png"),
    asset!("examples/poml-test-context.json"),
    asset!("docs/SKILLS.md"),
    asset!("docs/POML_WORKFLOWS.md"),
    asset!("docs/CONTEXT_VARIABLES.md"),
];

#[derive(Debug, Default)]
pub struct InstallReport {
    pub created: Vec<&'static str>,
    pub preserved: Vec<&'static str>,
    pub updated: Vec<&'static str>,
    pub backup_dir: Option<PathBuf>,
}

/// Fill missing bundled assets only. Existing prompt/SM/skill/configuration files
/// are never replaced. `update_dashboard` explicitly allows replacing DIFFERENT
/// bundled static files, with a recoverable backup of every previous file.
/// Each file is atomic; this is not a cross-file transaction.
pub fn install(directory: &Path, update_dashboard: bool) -> anyhow::Result<InstallReport> {
    fs::create_dir_all(directory)
        .with_context(|| format!("Cannot create asset directory {}", directory.display()))?;
    let root = directory.canonicalize()?;
    // Preflight every destination before writing files. Do not follow asset
    // symlinks, including dangling links, into unrelated directories.
    for asset in BUNDLED_ASSETS {
        check_destination(&root, Path::new(asset.path))?;
    }
    let mut report = InstallReport::default();
    for asset in BUNDLED_ASSETS {
        let destination = root.join(asset.path);
        let existing = match fs::symlink_metadata(&destination) {
            Ok(meta) => {
                anyhow::ensure!(
                    meta.is_file() && !meta.file_type().is_symlink(),
                    "Asset destination is not a regular file: {}",
                    destination.display()
                );
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        let update = existing && update_dashboard && asset.path.starts_with("static/");
        if existing && !update {
            report.preserved.push(asset.path);
            continue;
        }
        if update {
            let previous = fs::read(&destination)?;
            if previous.as_slice() == asset.bytes {
                report.preserved.push(asset.path);
                continue;
            }
            if report.backup_dir.is_none() {
                let relative =
                    PathBuf::from(".praxis-asset-backups").join(uuid::Uuid::new_v4().to_string());
                check_destination(&root, &relative.join("placeholder"))?;
                fs::create_dir_all(root.join(&relative))?;
                report.backup_dir = Some(root.join(relative));
            }
            let backup = report
                .backup_dir
                .as_ref()
                .expect("backup directory initialized")
                .join(asset.path);
            fs::create_dir_all(backup.parent().context("Asset backup has no parent")?)?;
            write_atomic(&backup, &previous, asset.executable, false)?;
        }
        check_destination(&root, Path::new(asset.path))?;
        fs::create_dir_all(destination.parent().context("Asset has no parent")?)?;
        write_atomic(&destination, asset.bytes, asset.executable, update)
            .with_context(|| format!("Cannot install {}", destination.display()))?;
        if update {
            report.updated.push(asset.path);
        } else {
            report.created.push(asset.path);
        }
    }
    Ok(report)
}

fn check_destination(root: &Path, relative: &Path) -> anyhow::Result<()> {
    let mut path = root.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Invalid bundled asset path"
        );
        path.push(component.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(meta) => {
                anyhow::ensure!(
                    !meta.file_type().is_symlink(),
                    "Refusing symlink asset destination: {}",
                    path.display()
                );
                let leaf = index + 1 == components.len();
                anyhow::ensure!(
                    if leaf { meta.is_file() } else { meta.is_dir() },
                    "Invalid asset destination: {}",
                    path.display()
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8], executable: bool, replace: bool) -> anyhow::Result<()> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(path.parent().context("Asset has no parent")?)?;
    temporary.write_all(bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(if executable {
                0o755
            } else {
                0o644
            }))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    temporary.as_file().sync_all()?;
    if replace {
        temporary.persist(path)?;
    } else {
        temporary.persist_noclobber(path)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "assets_tests.rs"]
mod tests;
