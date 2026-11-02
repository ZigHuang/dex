use super::editor_directory;
#[cfg(unix)]
use super::run_editor;
use dex_protocol::permissions::FileSystemAccessMode;
use dex_protocol::permissions::FileSystemSandboxEntry;
use dex_protocol::permissions::FileSystemSandboxPolicy;
use dex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

struct EditorPaths {
    _root: TempDir,
    dex_home: PathBuf,
    cwd: PathBuf,
}

impl EditorPaths {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("create editor test root");
        let canonical_root =
            dunce::canonicalize(root.path()).expect("canonicalize editor test root");
        let dex_home = canonical_root.join("dex-home");
        let cwd = canonical_root.join("workspace");
        fs::create_dir(&dex_home).expect("create Dex home");
        fs::create_dir(&cwd).expect("create workspace");

        Self {
            _root: root,
            dex_home,
            cwd,
        }
    }
}

fn workspace_write_policy(writable_roots: &[&Path]) -> FileSystemSandboxPolicy {
    let writable_roots = writable_roots
        .iter()
        .map(|root| AbsolutePathBuf::from_absolute_path(root).expect("absolute writable root"))
        .collect::<Vec<_>>();

    FileSystemSandboxPolicy::workspace_write(
        &writable_roots,
        /*exclude_tmpdir_env_var*/ true,
        /*exclude_slash_tmp*/ true,
    )
}

#[test]
fn editor_directory_is_inside_isolated_dex_home() {
    let paths = EditorPaths::new();
    let policy = workspace_write_policy(&[]);

    let directory = editor_directory(&[&paths.dex_home], &policy, &paths.cwd)
        .expect("create isolated editor directory");

    assert_eq!(directory, paths.dex_home.join("editor"));
    assert!(directory.is_dir());
}

#[test]
fn editor_directory_rejects_writable_home_editor_and_parent() {
    let paths = EditorPaths::new();
    let editor = paths.dex_home.join("editor");
    fs::create_dir(&editor).expect("create editor directory");
    let parent = paths.dex_home.parent().expect("Dex home parent");

    for writable_root in [paths.dex_home.as_path(), editor.as_path(), parent] {
        let policy = workspace_write_policy(&[writable_root]);

        assert!(
            editor_directory(&[&paths.dex_home], &policy, &paths.cwd).is_err(),
            "writable root {} must not expose editor buffers",
            writable_root.display()
        );
    }
}

#[test]
fn editor_directory_rejects_writable_descendant() {
    let paths = EditorPaths::new();
    let writable_descendant = paths.dex_home.join("editor").join("nested");
    fs::create_dir_all(&writable_descendant).expect("create writable editor descendant");
    let policy = workspace_write_policy(&[&writable_descendant]);

    assert!(editor_directory(&[&paths.dex_home], &policy, &paths.cwd).is_err());
}

#[test]
fn editor_directory_rejects_read_only_carveout_with_writable_parent() {
    let paths = EditorPaths::new();
    let editor = paths.dex_home.join("editor");
    fs::create_dir(&editor).expect("create editor directory");
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::from_absolute_path(&paths.dex_home)
                .expect("absolute Dex home")
                .into(),
            FileSystemAccessMode::Write,
        ),
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::from_absolute_path(&editor)
                .expect("absolute editor directory")
                .into(),
            FileSystemAccessMode::Read,
        ),
    ]);

    assert!(editor_directory(&[&paths.dex_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_preexisting_symlink() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let outside = paths.cwd.join("outside");
    fs::create_dir(&outside).expect("create editor symlink target");
    symlink(&outside, paths.dex_home.join("editor")).expect("create editor directory symlink");
    let policy = FileSystemSandboxPolicy::read_only();

    assert!(editor_directory(&[&paths.dex_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_writable_dex_home_alias() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let aliased_home = paths.cwd.join("dex-home-link");
    symlink(&paths.dex_home, &aliased_home).expect("create Dex home symlink");
    let policy = workspace_write_policy(&[]);

    assert!(policy.can_write_local_path_with_cwd(&aliased_home.join("editor"), &paths.cwd));
    assert!(!policy.can_write_local_path_with_cwd(&paths.dex_home.join("editor"), &paths.cwd));
    assert!(editor_directory(&[&aliased_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_writable_dex_home_alias_target() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let alias_parent = paths
        .dex_home
        .parent()
        .expect("Dex home parent")
        .join("aliases");
    fs::create_dir(&alias_parent).expect("create protected alias parent");
    let aliased_home = alias_parent.join("dex-home-link");
    symlink(&paths.dex_home, &aliased_home).expect("create Dex home symlink");
    let policy = workspace_write_policy(&[&paths.dex_home]);

    assert!(!policy.can_write_local_path_with_cwd(&aliased_home.join("editor"), &paths.cwd));
    assert!(policy.can_write_local_path_with_cwd(&paths.dex_home.join("editor"), &paths.cwd));
    assert!(editor_directory(&[&aliased_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_uses_protected_workspace_fallback_with_default_temporary_grants() {
    let root = tempfile::tempdir().expect("create editor test root");
    let dex_home = root.path().join("dex-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&dex_home).expect("create Dex home");
    fs::create_dir(&cwd).expect("create workspace");
    let workspace_dex_home = cwd.join(".dex");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );

    assert!(!workspace_dex_home.exists());
    assert!(policy.can_write_local_path_with_cwd(&dex_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_dex_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_dex_home.join("editor"), &cwd));

    let directory = editor_directory(&[&dex_home, &workspace_dex_home], &policy, &cwd)
        .expect("use protected workspace metadata directory");

    assert_eq!(
        directory,
        dunce::canonicalize(&workspace_dex_home)
            .expect("canonicalize workspace metadata directory")
            .join("editor")
    );
    assert!(directory.is_dir());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_explicitly_writable_workspace_fallback() {
    let root = tempfile::tempdir().expect("create editor test root");
    let dex_home = root.path().join("dex-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&dex_home).expect("create Dex home");
    fs::create_dir(&cwd).expect("create workspace");
    let workspace_dex_home = cwd.join(".dex");
    let writable_workspace_dex_home = AbsolutePathBuf::from_absolute_path(&workspace_dex_home)
        .expect("absolute workspace metadata directory");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[writable_workspace_dex_home],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );

    assert!(policy.can_write_local_path_with_cwd(&dex_home, &cwd));
    assert!(policy.can_write_local_path_with_cwd(&workspace_dex_home, &cwd));
    assert!(
        editor_directory(&[&dex_home, &workspace_dex_home], &policy, &cwd).is_err(),
        "explicitly writable metadata must not be used for editor buffers"
    );
    assert!(!workspace_dex_home.exists());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_workspace_fallback_symlink_to_writable_target() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let workspace_dex_home = paths.cwd.join(".dex");
    symlink(&paths.dex_home, &workspace_dex_home).expect("create workspace metadata symlink");
    let policy = workspace_write_policy(&[&paths.dex_home]);

    assert!(!policy.can_write_local_path_with_cwd(&workspace_dex_home, &paths.cwd));
    assert!(policy.can_write_local_path_with_cwd(&paths.dex_home, &paths.cwd));
    assert!(
        editor_directory(&[&paths.dex_home, &workspace_dex_home], &policy, &paths.cwd,).is_err()
    );
}

#[test]
fn editor_directory_uses_next_protected_candidate_after_creation_error() {
    let paths = EditorPaths::new();
    let unavailable_home = paths
        .dex_home
        .parent()
        .expect("Dex home parent")
        .join("unavailable-home");
    fs::write(&unavailable_home, "not a directory").expect("create unavailable Dex home");
    let policy = workspace_write_policy(&[]);

    let directory = editor_directory(&[&unavailable_home, &paths.dex_home], &policy, &paths.cwd)
        .expect("use next protected candidate after directory creation fails");

    assert_eq!(directory, paths.dex_home.join("editor"));
}

#[test]
#[cfg(windows)]
fn editor_directory_rejects_windows_temporary_directory_outside_tmpdir_policy_root() {
    let paths = EditorPaths::new();
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ true,
    );

    assert!(
        editor_directory(&[&paths.dex_home], &policy, &paths.cwd).is_err(),
        "effective Windows temporary directories must not contain editor buffers"
    );
    assert!(!paths.dex_home.join("editor").exists());
}

#[test]
fn editor_directory_allows_full_disk_write_policies() {
    let paths = EditorPaths::new();

    for policy in [
        FileSystemSandboxPolicy::unrestricted(),
        FileSystemSandboxPolicy::external_sandbox(),
    ] {
        let directory = editor_directory(&[&paths.dex_home], &policy, &paths.cwd)
            .expect("full-disk-write policies should preserve external editor support");

        assert_eq!(directory, paths.dex_home.join("editor"));
    }
}

#[tokio::test]
#[cfg(unix)]
async fn editor_process_receives_buffer_in_isolated_dex_home() {
    let paths = EditorPaths::new();
    let policy = workspace_write_policy(&[]);
    let editor_directory = paths.dex_home.join("editor");
    let editor_command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "case \"$2\" in \"$1\"/*) printf edited > \"$2\" ;; *) exit 88 ;; esac".to_string(),
        "editor".to_string(),
        editor_directory.to_string_lossy().into_owned(),
    ];

    let content = run_editor(
        "seed",
        &editor_command,
        &paths.dex_home,
        &policy,
        &paths.cwd,
    )
    .await
    .expect("run editor with isolated buffer");

    assert_eq!(content, "edited");
}

#[tokio::test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn editor_process_uses_protected_workspace_fallback_with_default_temporary_grants() {
    let root = tempfile::tempdir().expect("create editor test root");
    let dex_home = root.path().join("dex-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&dex_home).expect("create Dex home");
    fs::create_dir(&cwd).expect("create workspace");
    let default_dex_home = dirs::home_dir().expect("home directory").join(".dex");
    let writable_default_dex_home =
        AbsolutePathBuf::from_absolute_path(&default_dex_home).expect("absolute default Dex home");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[writable_default_dex_home],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );
    let workspace_dex_home = cwd.join(".dex");
    let editor_directory = dunce::canonicalize(&cwd)
        .expect("canonicalize workspace")
        .join(".dex")
        .join("editor");
    let editor_command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "case \"$2\" in \"$1\"/*) printf edited > \"$2\" ;; *) exit 88 ;; esac".to_string(),
        "editor".to_string(),
        editor_directory.to_string_lossy().into_owned(),
    ];

    assert!(!workspace_dex_home.exists());
    assert!(policy.can_write_local_path_with_cwd(&dex_home, &cwd));
    assert!(policy.can_write_local_path_with_cwd(&default_dex_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_dex_home, &cwd));

    let content = run_editor("seed", &editor_command, &dex_home, &policy, &cwd)
        .await
        .expect("run editor with protected workspace fallback");

    assert_eq!(content, "edited");
    assert!(editor_directory.is_dir());
}
