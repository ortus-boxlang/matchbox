use matchbox_modules::discovery::{discover_module_installations, discover_modules_with_home};
use std::{fs, path::Path};
fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}
#[test]
fn configured_directories_share_cli_selection_precedence_settings_and_private_paths() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    for path in [
        "boxlang_modules/local",
        "home/modules/homecopy",
        "extra/local",
        "extra/homecopy",
        "extra/choice",
        "extra/disabled",
        "later/choice",
    ] {
        write(root, &format!("{path}/ModuleConfig.bx"), "class {}");
    }
    write(
        root,
        "boxlang.json",
        r#"{"modulesDirectory":["missing","extra","later","extra"],"modules":{"choice":{"settings":{"answer":42}},"disabled":{"enabled":false}}}"#,
    );
    let result = discover_module_installations(root, &[], Some(&root.join("home")), None).unwrap();
    let find = |name: &str| {
        result
            .modules
            .iter()
            .find(|module| module.name == name)
            .unwrap()
    };
    assert_eq!(
        find("local").path,
        root.join("boxlang_modules/local").canonicalize().unwrap()
    );
    assert_eq!(
        find("homecopy").path,
        root.join("home/modules/homecopy").canonicalize().unwrap()
    );
    assert_eq!(
        find("choice").path,
        root.join("extra/choice").canonicalize().unwrap()
    );
    assert_eq!(find("choice").settings["answer"], 42);
    assert!(
        !result
            .modules
            .iter()
            .any(|module| module.name == "disabled")
    );
    for path in ["extra", "extra/disabled", "later", "later/choice"] {
        assert!(
            result
                .private_paths
                .contains(&root.join(path).canonicalize().unwrap()),
            "{path}"
        );
    }
    let cli = discover_modules_with_home(root, &[], Some(&root.join("home"))).unwrap();
    assert_eq!(cli.len(), result.modules.len());
    write(root, "override/choice/ModuleConfig.bx", "class {}");
    let cli = discover_modules_with_home(
        root,
        &[root.join("override/choice")],
        Some(&root.join("home")),
    )
    .unwrap();
    assert_eq!(
        cli.iter()
            .find(|module| module.name == "choice")
            .unwrap()
            .path,
        root.join("override/choice").canonicalize().unwrap()
    );
}
#[test]
fn explicit_config_paths_resolve_directories_against_project_not_config_parent_and_validate_types()
{
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(root, "external/pkg/ModuleConfig.bx", "class {}");
    let config = root.join("config/runtime.json");
    write(
        root,
        "config/runtime.json",
        r#"{"modulesDirectory":["external"]}"#,
    );
    let found = discover_module_installations(root, &[], None, Some(&config)).unwrap();
    assert_eq!(found.modules[0].name, "pkg");
    for value in [r#""external""#, r#"[1]"#, r#"[null]"#, "{}"] {
        fs::write(&config, format!("{{\"modulesDirectory\":{value}}}")).unwrap();
        assert!(discover_module_installations(root, &[], None, Some(&config)).is_err());
    }
}
#[cfg(unix)]
#[test]
fn disabled_and_shadowed_symlink_packages_still_report_their_physical_paths_for_privacy() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    write(root, "boxlang_modules/pkg/ModuleConfig.bx", "class {}");
    write(root, "outside/pkg/ModuleConfig.bx", "class {}");
    write(root, "outside/disabled/ModuleConfig.bx", "class {}");
    fs::create_dir(root.join("external")).unwrap();
    for name in ["pkg", "disabled"] {
        std::os::unix::fs::symlink(
            root.join("outside").join(name),
            root.join("external").join(name),
        )
        .unwrap();
    }
    write(
        root,
        "boxlang.json",
        r#"{"modulesDirectory":["external"],"modules":{"disabled":{"enabled":false}}}"#,
    );
    let result = discover_module_installations(root, &[], None, None).unwrap();
    assert_eq!(result.modules.len(), 1);
    for name in ["pkg", "disabled"] {
        assert!(
            result
                .private_paths
                .contains(&root.join("outside").join(name).canonicalize().unwrap())
        );
    }
}
