use std::fs;
use std::path::{Path, PathBuf};

use super::*;

struct Fixture {
    _dir: tempfile::TempDir,
    base: PathBuf,
    config: ConfigDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = canonical(dir.path()).unwrap();
    let config = ConfigDir::new(base.join("home").join(".harness"));
    Fixture {
        _dir: dir,
        base,
        config,
    }
}

fn repo(base: &Path, relative: &str) -> PathBuf {
    let path = base.join(relative);
    fs::create_dir_all(path.join(".git")).unwrap();
    path
}

#[test]
fn init_writes_defaults_once() {
    let f = fixture();
    let written = f.config.init().unwrap();
    assert_eq!(written, [f.config.config_file()]);
    assert!(f.config.projects_dir().is_dir());
    // Built-in harnesses are not copied, so they never go stale on disk.
    assert!(!f.config.harnesses_dir().join("pi.toml").exists());
    assert_eq!(f.config.harness("pi").unwrap().llm, Some(LlmWiring::Pi));

    fs::write(f.config.config_file(), "machine = \"other\"\n").unwrap();
    assert!(f.config.init().unwrap().is_empty());
    assert_eq!(f.config.global().unwrap().machine, "other");
}

#[test]
fn global_defaults_and_errors() {
    let f = fixture();
    assert_eq!(f.config.global().unwrap(), GlobalConfig::default());

    fs::create_dir_all(f.config.root()).unwrap();
    fs::write(f.config.config_file(), "[defaults]\nnetwork = \"closed\"\n").unwrap();
    match f.config.global() {
        Err(Error::ConfigInvalid { path, message }) => {
            assert_eq!(path, f.config.config_file());
            assert!(message.contains("closed"), "{message}");
        }
        other => panic!("expected ConfigInvalid, got {other:?}"),
    }
}

#[test]
fn create_and_find_project() {
    let f = fixture();
    f.config.init().unwrap();
    let path = repo(&f.base, "repos/My App");

    assert_eq!(f.config.find_project(&path).unwrap(), None);
    let project = f.config.create_project(&path.join(".")).unwrap();
    assert_eq!(project.name.as_str(), "my-app");
    assert!(same_path(&project.path, &path));
    assert_eq!(project.harness, "pi");
    assert_eq!(project.network, NetworkPreset::Development);
    assert_eq!(project.resources.memory.as_deref(), Some("3g"));
    assert_eq!(project.resources.pids_limit, 1024);

    // The file is the commented template with the values filled in.
    let text = fs::read_to_string(&project.file).unwrap();
    assert!(text.contains("# Projekt „my-app“"), "{text}");
    assert!(text.contains("mode = \"isolated\""));

    assert_eq!(f.config.find_project(&path).unwrap().unwrap(), project);
    assert!(f.config.create_project(&path).is_err(), "created twice");

    // Same folder name elsewhere gets a suffix.
    let other = repo(&f.base, "elsewhere/My App");
    assert_eq!(
        f.config.create_project(&other).unwrap().name.as_str(),
        "my-app-2"
    );
    assert_eq!(f.config.project_names().unwrap().len(), 2);

    // A chosen name instead of the folder's, but never one that is taken.
    let named = repo(&f.base, "repos/third");
    let shop = ProjectName::new("shop").unwrap();
    assert_eq!(
        f.config
            .create_project_named(&named, Some(&shop))
            .unwrap()
            .name,
        shop
    );
    let fourth = repo(&f.base, "repos/fourth");
    assert!(f.config.create_project_named(&fourth, Some(&shop)).is_err());

    // Removing forgets the project, not the repository.
    f.config.remove_project(&shop).unwrap();
    assert_eq!(f.config.find_project(&named).unwrap(), None);
    assert!(named.join(".git").exists());
    assert!(f.config.remove_project(&shop).is_err());
}

#[cfg(windows)]
#[test]
fn find_project_ignores_case_on_windows() {
    let f = fixture();
    let path = repo(&f.base, "repos/app");
    f.config.create_project(&path).unwrap();
    let upper = PathBuf::from(path.to_string_lossy().to_uppercase());
    assert!(f.config.find_project(&upper).unwrap().is_some());
}

#[test]
fn project_uses_global_preset_and_own_overrides() {
    let f = fixture();
    fs::create_dir_all(f.config.root()).unwrap();
    fs::write(
        f.config.config_file(),
        "[defaults]\nharness = \"other\"\nnetwork = \"off\"\n[defaults.resources]\ncpus = 2\n",
    )
    .unwrap();
    let project = f.config.create_project(&repo(&f.base, "app")).unwrap();
    assert_eq!(project.network, NetworkPreset::Off);
    assert_eq!(project.harness, "other");
    assert_eq!(project.resources.cpus, Some(2.0));
    assert_eq!(project.resources.memory, None);

    let project = f
        .config
        .update_project(&project.name, |doc| {
            doc["default_harness"] = toml_edit::value("pi");
            doc["resources"]["memory"] = toml_edit::value("1g");
            Ok(())
        })
        .unwrap();
    assert_eq!(project.harness, "pi");
    assert_eq!(project.resources.memory.as_deref(), Some("1g"));
    assert_eq!(project.resources.cpus, Some(2.0));
}

#[test]
fn update_keeps_comments_and_rejects_invalid_edits() {
    let f = fixture();
    let project = f.config.create_project(&repo(&f.base, "app")).unwrap();
    let before = fs::read_to_string(&project.file).unwrap();

    let updated = f
        .config
        .update_project(&project.name, |doc| {
            doc["network"]["preset"] = toml_edit::value("custom");
            let mut allow = toml_edit::Array::new();
            allow.push("api.deepseek.com");
            doc["network"]["allow"] = toml_edit::value(allow);
            Ok(())
        })
        .unwrap();
    assert_eq!(updated.network, NetworkPreset::Custom);
    assert_eq!(updated.allow[0].to_string(), "api.deepseek.com");
    let after = fs::read_to_string(&project.file).unwrap();
    for line in before.lines().filter(|l| l.starts_with('#')) {
        assert!(after.contains(line), "comment lost: {line}");
    }

    let rejected = f.config.update_project(&project.name, |doc| {
        doc["network"]["preset"] = toml_edit::value("everything");
        Ok(())
    });
    assert!(matches!(rejected, Err(Error::ConfigInvalid { .. })));
    assert_eq!(fs::read_to_string(&project.file).unwrap(), after);
}

#[test]
fn network_edits_keep_the_file_intact() {
    let f = fixture();
    let project = f.config.create_project(&repo(&f.base, "app")).unwrap();
    assert!(project.ask);
    assert_eq!(project.llm_apis[0].to_string(), "api.deepseek.com");

    let open = f
        .config
        .set_network_preset(&project.name, NetworkPreset::Open)
        .unwrap();
    assert_eq!(open.network, NetworkPreset::Open);

    let docs: DomainPattern = "docs.rs".parse().unwrap();
    let (updated, changed) = f
        .config
        .add_network_domain(&project.name, DomainList::Allow, &docs)
        .unwrap();
    assert!(changed);
    assert_eq!(updated.allow, std::slice::from_ref(&docs));
    let (_, changed) = f
        .config
        .add_network_domain(&project.name, DomainList::Allow, &docs)
        .unwrap();
    assert!(!changed, "listed twice");

    // A missing list is created.
    let text = fs::read_to_string(&project.file)
        .unwrap()
        .replace("deny = []\n", "");
    fs::write(&project.file, text).unwrap();
    let evil: DomainPattern = "*.evil.example".parse().unwrap();
    let (updated, _) = f
        .config
        .add_network_domain(&project.name, DomainList::Deny, &evil)
        .unwrap();
    assert_eq!(updated.deny, [evil]);
    let text = fs::read_to_string(&project.file).unwrap();
    assert!(
        text.contains("# Zusätzlich erlaubte"),
        "comments kept: {text}"
    );
}

#[test]
fn server_url_is_written_and_removed() {
    let f = fixture();
    let global = f
        .config
        .set_server_url(Some("http://localhost:8080"))
        .unwrap();
    assert_eq!(global.server.url.as_deref(), Some("http://localhost:8080"));
    let text = fs::read_to_string(f.config.config_file()).unwrap();
    assert!(text.contains("# Podman-Machine"), "template kept: {text}");
    assert_eq!(
        f.config.global().unwrap().server.url.as_deref(),
        Some("http://localhost:8080")
    );

    assert_eq!(
        f.config.set_server_url(None).unwrap().server,
        ServerConfig::default()
    );
    assert_eq!(f.config.global().unwrap().server, ServerConfig::default());
    let text = fs::read_to_string(f.config.config_file()).unwrap();
    assert!(!text.lines().any(|l| l == "[server]"), "{text}");
    assert!(f.config.set_server_url(Some("localhost:8080")).is_err());
}

#[test]
fn server_ca_survives_logout() {
    let f = fixture();
    f.config.init().unwrap();
    let ca = f.base.join("root.crt");
    let mut text = fs::read_to_string(f.config.config_file()).unwrap();
    text.push_str(&format!(
        "\n[server]\n# Caddy tls internal\nca = '{}'\n",
        ca.display()
    ));
    fs::write(f.config.config_file(), text).unwrap();

    let global = f.config.set_server_url(Some("https://pi.lan")).unwrap();
    assert_eq!(global.server.url.as_deref(), Some("https://pi.lan"));
    assert_eq!(global.server.ca.as_deref(), Some(ca.as_path()));

    // Logging out forgets the server, not the hand-written CA.
    let global = f.config.set_server_url(None).unwrap();
    assert_eq!(global.server.url, None);
    assert_eq!(global.server.ca.as_deref(), Some(ca.as_path()));
    let text = fs::read_to_string(f.config.config_file()).unwrap();
    assert!(text.contains("# Caddy tls internal"), "{text}");

    // Relative paths would depend on the current directory.
    fs::write(f.config.config_file(), "[server]\nca = 'root.crt'\n").unwrap();
    match f.config.global() {
        Err(Error::ConfigInvalid { message, .. }) => {
            assert!(message.contains("server.ca"), "{message}");
        }
        other => panic!("expected ConfigInvalid, got {other:?}"),
    }
    fs::write(f.config.config_file(), "[server]\ncert = 'x'\n").unwrap();
    assert!(f.config.global().is_err(), "unknown key accepted");
}

#[test]
fn broken_files_do_not_hide_matches() {
    let f = fixture();
    let path = repo(&f.base, "good");
    f.config.create_project(&path).unwrap();
    fs::write(f.config.projects_dir().join("broken.toml"), "path = 42\n").unwrap();

    assert!(f.config.find_project(&path).unwrap().is_some());
    let unknown = repo(&f.base, "unknown");
    assert!(matches!(
        f.config.find_project(&unknown),
        Err(Error::ConfigInvalid { .. })
    ));
}

#[test]
fn two_files_for_one_repo_are_an_error() {
    let f = fixture();
    let path = repo(&f.base, "app");
    let project = f.config.create_project(&path).unwrap();
    fs::copy(&project.file, f.config.projects_dir().join("copy.toml")).unwrap();
    assert!(f.config.find_project(&path).is_err());
}

#[test]
fn config_and_projects_must_not_overlap() {
    let f = fixture();
    // A repository that contains the configuration folder (e.g. the profile).
    let home = repo(&f.base, "home");
    f.config.init().unwrap();
    assert!(f.config.create_project(&home).is_err());

    // A repository inside the configuration folder.
    let inside = repo(f.config.root(), "projects/sneaky");
    assert!(f.config.create_project(&inside).is_err());

    // A hand-edited file pointing at an overlapping path is refused on load.
    let ok = f.config.create_project(&repo(&f.base, "ok")).unwrap();
    let mut doc: DocumentMut = fs::read_to_string(&ok.file).unwrap().parse().unwrap();
    doc["path"] = toml_edit::value(home.to_string_lossy().into_owned());
    fs::write(&ok.file, doc.to_string()).unwrap();
    assert!(f.config.project(&ok.name).is_err());
    // update_project refuses to write such a path in the first place.
    let fresh = f.config.create_project(&repo(&f.base, "fresh")).unwrap();
    let home_text = home.to_string_lossy().into_owned();
    assert!(
        f.config
            .update_project(&fresh.name, |doc| {
                doc["path"] = toml_edit::value(home_text);
                Ok(())
            })
            .is_err()
    );
}

#[test]
fn rejects_non_repositories() {
    let f = fixture();
    let plain = f.base.join("plain");
    fs::create_dir_all(&plain).unwrap();
    assert!(f.config.create_project(&plain).is_err());
}

#[test]
fn harness_definitions() {
    let f = fixture();
    // Built-in without any file.
    let pi = f.config.harness("pi").unwrap();
    assert_eq!(pi.command, ["pi"]);
    assert!(matches!(
        f.config.harness("nope"),
        Err(Error::HarnessUnknown(_))
    ));
    assert!(f.config.harness("../x").is_err());

    // A file overrides the built-in one.
    fs::create_dir_all(f.config.harnesses_dir()).unwrap();
    fs::write(
        f.config.harnesses_dir().join("pi.toml"),
        "name = \"pi\"\ninstall = \"npm i -g pi@1\"\ncommand = [\"pi\", \"--x\"]\n",
    )
    .unwrap();
    assert_eq!(f.config.harness("pi").unwrap().command, ["pi", "--x"]);

    fs::write(
        f.config.harnesses_dir().join("mine.toml"),
        "name = \"other\"\ninstall = \"true\"\ncommand = [\"x\"]\n",
    )
    .unwrap();
    assert!(matches!(
        f.config.harness("mine"),
        Err(Error::ConfigInvalid { .. })
    ));
}

#[test]
fn unknown_project() {
    let f = fixture();
    let name = ProjectName::new("missing").unwrap();
    assert!(matches!(
        f.config.project(&name),
        Err(Error::ProjectUnknown(_))
    ));
}

#[test]
fn remote_access_is_switched_and_chats_remembered() {
    let f = fixture();
    f.config.init().unwrap();
    let project = f.config.create_project(&repo(&f.base, "app")).unwrap();
    let name = project.name;
    assert_eq!(
        f.config.project_config(&name).unwrap().remote,
        RemoteConfig::default()
    );

    let remote = f.config.set_remote(&name, Some(true), Some(true)).unwrap();
    assert!(remote.enabled && remote.confirm);
    f.config.remember_remote_chat(&name, "0199-abc").unwrap();
    f.config.remember_remote_chat(&name, "0199-abc").unwrap();
    assert!(f.config.remember_remote_chat(&name, "../x").is_err());
    let remote = f.config.set_remote(&name, None, Some(false)).unwrap();
    assert_eq!(remote.chats, ["0199-abc"]);
    assert!(!remote.confirm);

    // A project file from before `[remote]` existed gets the table on demand
    // and is offered like a new one.
    let file = f.config.project_file(&name);
    let text = fs::read_to_string(&file).unwrap();
    let start = text.find("[remote]").unwrap();
    let end = text.find("[resources]").unwrap();
    fs::write(&file, format!("{}{}", &text[..start], &text[end..])).unwrap();
    assert!(f.config.project_config(&name).unwrap().remote.enabled);
    f.config.remember_remote_chat(&name, "c2").unwrap();
    assert_eq!(f.config.project_config(&name).unwrap().remote.chats, ["c2"]);
    let text = fs::read_to_string(&file).unwrap();
    assert!(text.contains("# Git-Repository auf dem Host"), "{text}");
}

#[test]
fn app_catalog_is_valid() {
    let f = fixture();
    let apps = f.config.apps().unwrap();
    let names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
    assert_eq!(names, ["c", "rust", "go", "java", "deno", "bun", "sqlite"]);
    for app in &apps {
        // Every requirement resolves, and every recipe reports its version.
        f.config.resolve_apps(&app.requires).unwrap();
        assert!(app.install.contains("harness_version"), "{}", app.name);
        assert!(!app.hosts.is_empty(), "{} gets no network", app.name);
    }
    let rust = f.config.app("rust").unwrap();
    assert_eq!(
        rust.path_dirs(),
        ["/opt/harness-apps/rust/cargo/bin", "/home/agent/.cargo/bin"]
    );
    assert!(matches!(f.config.app("cobol"), Err(Error::AppUnknown(_))));
    assert!(f.config.app("../c").is_err());
}

#[test]
fn own_apps_add_to_and_replace_the_catalog() {
    let f = fixture();
    fs::create_dir_all(f.config.apps_dir()).unwrap();
    let own = "name = 'zz'\ntitle = 'Eigenes'\nrequires = ['go']\npath = ['bin']\n\
               install = 'harness_version 1'\n";
    fs::write(f.config.apps_dir().join("zz.toml"), own).unwrap();
    let go = "name = 'go'\ntitle = 'Mein Go'\ninstall = 'harness_version 2'\n";
    fs::write(f.config.apps_dir().join("go.toml"), go).unwrap();
    let apps = f.config.apps().unwrap();
    assert_eq!(apps.last().unwrap().name, "zz");
    assert_eq!(f.config.app("go").unwrap().title, "Mein Go");

    // Required apps first, each once.
    let resolved: Vec<String> = f
        .config
        .resolve_apps(&["zz".into(), "rust".into(), "go".into()])
        .unwrap()
        .into_iter()
        .map(|app| app.name)
        .collect();
    assert_eq!(resolved, ["go", "zz", "c", "rust"]);

    let wrong_name = "name = 'other'\ntitle = 'X'\ninstall = 'true'\n";
    fs::write(f.config.apps_dir().join("yy.toml"), wrong_name).unwrap();
    assert!(f.config.app("yy").is_err());
    fs::remove_file(f.config.apps_dir().join("yy.toml")).unwrap();

    let cycle = "name = 'go'\ntitle = 'Go'\nrequires = ['zz']\ninstall = 'true'\n";
    fs::write(f.config.apps_dir().join("go.toml"), cycle).unwrap();
    let error = f
        .config
        .resolve_apps(&["zz".into()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz → go → zz"), "{error}");
}

#[test]
fn templates_of_the_account_come_after_the_computers_apps() {
    let f = fixture();
    assert!(f.config.templates().is_empty());
    let template = |name: &str, title: &str| AppTemplate {
        name: name.into(),
        title: title.into(),
        description: None,
        kind: AppKind::Apt,
        packages: vec!["tree".into()],
        script: String::new(),
        hosts: Vec::new(),
        path: Vec::new(),
        requires: vec!["c".into()],
    };
    f.config
        .save_templates(&[template("tree", "Baum"), template("go", "Fremdes Go")])
        .unwrap();
    assert_eq!(f.config.templates().len(), 2);
    let tree = f.config.app("tree").unwrap();
    assert_eq!(
        (tree.title.as_str(), tree.source),
        ("Baum", AppSource::Server)
    );
    // A template never replaces the catalog or a file of the computer.
    let go = f.config.app("go").unwrap();
    assert_eq!((go.title.as_str(), go.source), ("Go", AppSource::Builtin));
    fs::create_dir_all(f.config.apps_dir()).unwrap();
    let own = "name = 'tree'\ntitle = 'Mein Baum'\ninstall = 'true'\n";
    fs::write(f.config.apps_dir().join("tree.toml"), own).unwrap();
    let tree = f.config.app("tree").unwrap();
    assert_eq!(
        (tree.title.as_str(), tree.source),
        ("Mein Baum", AppSource::Local)
    );
    let names = f.config.app_names().unwrap();
    assert_eq!(names.iter().filter(|name| *name == "tree").count(), 1);
    fs::remove_file(f.config.apps_dir().join("tree.toml")).unwrap();
    assert_eq!(names.last().unwrap(), "tree");
    let resolved: Vec<String> = f
        .config
        .resolve_apps(&["tree".into()])
        .unwrap()
        .into_iter()
        .map(|app| app.name)
        .collect();
    assert_eq!(resolved, ["c", "tree"]);

    // A broken copy or a template this harness cannot read stops nothing.
    fs::write(f.config.templates_file(), "{nope").unwrap();
    assert!(f.config.templates().is_empty());
    assert!(matches!(f.config.app("tree"), Err(Error::AppUnknown(_))));
    let newer = r#"[{"name":"x","title":"X","kind":"cargo"},
        {"name":"tree","title":"Baum","kind":"apt","packages":["tree"]}]"#;
    fs::write(f.config.templates_file(), newer).unwrap();
    assert_eq!(f.config.templates().len(), 1);
    // One the server should never have sent fails only itself.
    let bad = r#"[{"name":"tree","title":"Baum","kind":"apt","packages":["a b"]}]"#;
    fs::write(f.config.templates_file(), bad).unwrap();
    let error = f.config.app("tree").unwrap_err().to_string();
    assert!(error.contains("Vorlage tree"), "{error}");
    assert!(f.config.app("go").is_ok());
}

#[test]
fn app_recipes_are_validated() {
    let valid = "name = 'x'\ntitle = 'X'\ninstall = 'true'\n";
    let with = |extra: &str| -> Result<AppConfig> {
        let text = format!("{valid}{extra}");
        parse(Path::new("x.toml"), &text)
    };
    with("").unwrap();
    with("path = ['bin', 'sub/bin', '~/.x/bin']\n").unwrap();
    for bad in [
        "path = ['/usr/bin']\n",
        "path = ['../bin']\n",
        "path = ['a:b']\n",
        "path = ['~other/bin']\n",
        "path = ['']\n",
        "requires = ['x']\n",
        "requires = ['Nope']\n",
        "[env]\nPATH = '/x'\n",
        "[env]\nHOME = '/x'\n",
        "[env]\n'A B' = 'x'\n",
        "hosts = ['127.0.0.1']\n",
        "unknown = 1\n",
    ] {
        assert!(with(bad).is_err(), "{bad}");
    }
    let untitled = "name = 'x'\ntitle = ''\ninstall = 'true'\n";
    assert!(parse::<AppConfig>(Path::new("x.toml"), untitled).is_err());
}

#[test]
fn apps_are_switched_per_project() {
    let f = fixture();
    let repo = repo(&f.base, "work/demo");
    let project = f.config.create_project(&repo).unwrap();
    // C/C++ used to be part of the base image; new projects start with it.
    assert_eq!(project.apps, ["c"]);
    let text = fs::read_to_string(&project.file).unwrap();
    assert!(
        text.contains("[apps]") && text.contains(r#"enabled = ["c"]"#),
        "{text}"
    );

    let project = f
        .config
        .set_apps(&project.name, &["rust".into(), "go".into()], &[])
        .unwrap();
    assert_eq!(project.apps, ["c", "rust", "go"]);
    let names: Vec<String> = f
        .config
        .project_apps(&project)
        .unwrap()
        .into_iter()
        .map(|app| app.name)
        .collect();
    assert_eq!(names, ["c", "rust", "go"]);
    // Twice on stays once; unknown apps cannot be switched on.
    f.config
        .set_apps(&project.name, &["go".into()], &[])
        .unwrap();
    assert!(
        f.config
            .set_apps(&project.name, &["cobol".into()], &[])
            .is_err()
    );
    let project = f
        .config
        .set_apps(&project.name, &[], &["rust".into()])
        .unwrap();
    assert_eq!(project.apps, ["c", "go"]);
    // Comments of the template survive.
    let text = fs::read_to_string(&project.file).unwrap();
    assert!(text.contains("# Programme aus dem App-Katalog"), "{text}");

    // A project file from before [apps] keeps C/C++ and gets the table on
    // first use. Literal TOML strings ('…') keep the backslashes of Windows
    // paths as they are.
    fs::write(&project.file, format!("path = '{}'\n", repo.display())).unwrap();
    assert_eq!(f.config.project(&project.name).unwrap().apps, ["c"]);
    let project = f
        .config
        .set_apps(&project.name, &["sqlite".into()], &[])
        .unwrap();
    assert_eq!(project.apps, ["c", "sqlite"]);
    // Switching off an app whose recipe is gone works.
    fs::write(
        &project.file,
        format!("path = '{}'\n[apps]\nenabled = ['gone']\n", repo.display()),
    )
    .unwrap();
    let project = f
        .config
        .set_apps(&project.name, &[], &["gone".into()])
        .unwrap();
    assert!(project.apps.is_empty());
}
