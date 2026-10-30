//! Programs the user defines in the browser (app catalog, step 3). The
//! server keeps them per account as templates and hands them to every
//! device of the account (tunnel protocol 6); the device keeps a copy in
//! `server-apps.json` for terminal starts and turns each into an app recipe.
//!
//! For the package kinds the device writes the install script itself, so
//! the server only ever names packages, checked character by character
//! here. A script template's text is installed like any recipe: in the
//! installer container of a project, never on the host.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::model::{AppConfig, AppSource, DomainPattern, Validate};

/// Most packages a template names.
pub const MAX_PACKAGES: usize = 20;

/// Longest package name with its version.
pub const MAX_PACKAGE_LEN: usize = 100;

/// Longest install script of a script template.
pub const MAX_SCRIPT: usize = 20_000;

/// How a template installs its program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    /// Debian packages, unpacked into the app's folder.
    Apt,
    /// npm packages with their commands.
    Npm,
    /// Python programs, each in an environment of its own (like pipx).
    Pip,
    /// The user's own install script.
    Script,
}

impl AppKind {
    /// Hosts the installer needs for packages of this kind.
    fn hosts(self) -> &'static [&'static str] {
        match self {
            AppKind::Apt => &["deb.debian.org"],
            AppKind::Npm => &["registry.npmjs.org"],
            AppKind::Pip => &["pypi.org", "files.pythonhosted.org"],
            AppKind::Script => &[],
        }
    }

    /// The kind as the browser names it.
    fn label(self) -> &'static str {
        match self {
            AppKind::Apt => "apt",
            AppKind::Npm => "npm",
            AppKind::Pip => "pip",
            AppKind::Script => "Skript",
        }
    }
}

/// A program of the account, as the server sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppTemplate {
    /// App name, unique in the account.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// One line for listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How it installs.
    pub kind: AppKind,
    /// Packages for the package kinds, the first one names the version.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Install script of a script template.
    #[serde(default)]
    pub script: String,
    /// Hosts a script template's installer may reach.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Folders of a script template for `PATH`; `bin` if none.
    #[serde(default)]
    pub path: Vec<String>,
    /// Apps it requires.
    #[serde(default)]
    pub requires: Vec<String>,
}

impl AppTemplate {
    /// The template as an app recipe, checked like a recipe file.
    pub fn to_app(&self) -> Result<AppConfig, String> {
        let packages = self.checked_packages()?;
        let install = match self.kind {
            AppKind::Script => {
                if self.script.len() > MAX_SCRIPT {
                    return Err(format!("script: höchstens {MAX_SCRIPT} Zeichen"));
                }
                self.script.clone()
            }
            kind => package_script(kind, &packages),
        };
        let hosts = match self.kind {
            AppKind::Script => self
                .hosts
                .iter()
                .map(|host| host.parse::<DomainPattern>())
                .collect::<Result<_, _>>()
                .map_err(|e| format!("hosts: {e}"))?,
            kind => kind
                .hosts()
                .iter()
                .map(|host| host.parse().expect("hosts of a kind are valid"))
                .collect(),
        };
        let path = match self.kind {
            AppKind::Script if !self.path.is_empty() => self.path.clone(),
            _ => vec!["bin".to_owned()],
        };
        let description = self
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                (!packages.is_empty())
                    .then(|| format!("{}: {}", self.kind.label(), packages.join(", ")))
            });
        let app = AppConfig {
            name: self.name.clone(),
            title: self.title.trim().to_owned(),
            description,
            requires: self.requires.clone(),
            hosts,
            path,
            env: BTreeMap::new(),
            install,
            file: None,
            source: AppSource::Server,
        };
        app.validate()?;
        Ok(app)
    }

    /// The packages, each checked for its kind; none for a script.
    fn checked_packages(&self) -> Result<Vec<String>, String> {
        if self.kind == AppKind::Script {
            return Ok(Vec::new());
        }
        if self.packages.is_empty() || self.packages.len() > MAX_PACKAGES {
            return Err(format!("packages: 1 bis {MAX_PACKAGES} Pakete"));
        }
        for package in &self.packages {
            let valid = package.len() <= MAX_PACKAGE_LEN
                && match self.kind {
                    AppKind::Apt => valid_deb(package),
                    AppKind::Npm => npm_name(package).is_some(),
                    AppKind::Pip => pip_name(package).is_some(),
                    AppKind::Script => true,
                };
            if !valid {
                return Err(format!(
                    "packages: {package:?} ist kein gültiges {}-Paket",
                    self.kind.label()
                ));
            }
        }
        Ok(self.packages.clone())
    }
}

/// The install script for packages of `kind` (helpers from `apps/lib.sh`).
/// Every package passed [`AppTemplate::checked_packages`], so none contains
/// a quote and single quotes keep them one word each.
fn package_script(kind: AppKind, packages: &[String]) -> String {
    let quoted: Vec<String> = packages
        .iter()
        .inspect(|package| debug_assert!(!package.contains('\'')))
        .map(|package| format!("'{package}'"))
        .collect();
    let list = quoted.join(" ");
    let first = &packages[0];
    match kind {
        AppKind::Apt => format!(
            "harness_apt {list}\n\
             harness_wrap usr/bin usr/sbin usr/games bin sbin\n\
             harness_version \"$(harness_deb_version '{first}')\"\n"
        ),
        AppKind::Npm => format!(
            "harness_npm {list}\n\
             harness_version \"$(harness_npm_version '{}')\"\n",
            npm_name(first).expect("checked")
        ),
        AppKind::Pip => format!(
            "harness_pip {list}\n\
             harness_version \"$(harness_pip_version '{}')\"\n",
            pip_name(first).expect("checked")
        ),
        AppKind::Script => unreachable!("scripts are the user's own"),
    }
}

/// A Debian package name: lowercase letters, digits and `+ - .`, starting
/// with a letter or digit, at least two characters (Debian policy 5.6.1).
fn valid_deb(name: &str) -> bool {
    name.len() >= 2
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
}

/// The package name of an npm spec such as `typescript`, `typescript@5` or
/// `@biomejs/biome@^1.9`; `None` if it is not one. Only registry names with
/// an optional version or range, no URLs, paths or git specs.
fn npm_name(spec: &str) -> Option<&str> {
    let (scope, rest) = match spec.strip_prefix('@') {
        Some(scoped) => {
            let (scope, rest) = scoped.split_once('/')?;
            (Some(scope), rest)
        }
        None => (None, spec),
    };
    let (name, version) = match rest.split_once('@') {
        Some((name, version)) => (name, Some(version)),
        None => (rest, None),
    };
    let word = |text: &str| {
        text.starts_with(|c: char| c.is_ascii_alphanumeric())
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '-'))
    };
    let range = |text: &str| {
        !text.is_empty()
            && text.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '^' | '~' | '*' | '+' | '-')
            })
    };
    let valid = scope.is_none_or(word) && word(name) && version.is_none_or(range);
    valid.then(|| &spec[..spec.len() - version.map_or(0, |v| v.len() + 1)])
}

/// The normalized name (PEP 503) of a pip requirement such as `ruff`,
/// `yt-dlp[default]` or `black==24.8.0`; `None` if it is not one. Only
/// names with optional extras and one version clause, no URLs or markers.
/// uv names a tool's environment after it.
fn pip_name(spec: &str) -> Option<String> {
    let end = spec
        .find(['[', '=', '<', '>', '~', '!'])
        .unwrap_or(spec.len());
    let (name, rest) = spec.split_at(end);
    let valid_name = name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.ends_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let rest = match rest.strip_prefix('[') {
        Some(extras) => {
            let (extras, after) = extras.split_once(']')?;
            let valid = !extras.is_empty()
                && extras
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ','));
            if !valid {
                return None;
            }
            after
        }
        None => rest,
    };
    let version_ok = rest.is_empty()
        || ["==", ">=", "<=", "~=", "!=", "<", ">"]
            .iter()
            .find_map(|op| rest.strip_prefix(op))
            .is_some_and(|version| {
                !version.is_empty()
                    && version.chars().all(|c| {
                        c.is_ascii_alphanumeric() || matches!(c, '.' | '*' | '+' | '!' | '_' | '-')
                    })
            });
    if !(valid_name && version_ok) {
        return None;
    }
    let mut normalized = String::new();
    for c in name.chars() {
        if matches!(c, '.' | '_' | '-') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.push(c.to_ascii_lowercase());
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(kind: AppKind, packages: &[&str]) -> AppTemplate {
        AppTemplate {
            name: "tool".into(),
            title: "Tool".into(),
            description: None,
            kind,
            packages: packages.iter().map(|p| (*p).to_owned()).collect(),
            script: String::new(),
            hosts: Vec::new(),
            path: Vec::new(),
            requires: Vec::new(),
        }
    }

    #[test]
    fn package_kinds_write_their_own_script() {
        let apt = template(AppKind::Apt, &["ffmpeg", "libimage-exiftool-perl"])
            .to_app()
            .unwrap();
        assert_eq!(apt.source, AppSource::Server);
        assert_eq!(apt.path, ["bin"]);
        assert_eq!(apt.hosts[0].to_string(), "deb.debian.org");
        assert!(
            apt.install
                .starts_with("harness_apt 'ffmpeg' 'libimage-exiftool-perl'\n"),
            "{}",
            apt.install
        );
        assert!(apt.install.contains("harness_deb_version 'ffmpeg'"));
        assert_eq!(
            apt.description.as_deref(),
            Some("apt: ffmpeg, libimage-exiftool-perl")
        );

        let npm = template(AppKind::Npm, &["@biomejs/biome@^1.9", "typescript"])
            .to_app()
            .unwrap();
        assert!(
            npm.install
                .contains("harness_npm '@biomejs/biome@^1.9' 'typescript'")
        );
        assert!(npm.install.contains("harness_npm_version '@biomejs/biome'"));
        assert_eq!(npm.hosts[0].to_string(), "registry.npmjs.org");

        let pip = template(AppKind::Pip, &["Yt_DLP[default]>=2024.1", "ruff"])
            .to_app()
            .unwrap();
        assert!(pip.install.contains("harness_pip_version 'yt-dlp'"));
        assert_eq!(pip.hosts.len(), 2);
    }

    #[test]
    fn packages_are_plain_names() {
        for (kind, bad) in [
            (AppKind::Apt, "ffmpeg; rm -rf /"),
            (AppKind::Apt, "FFmpeg"),
            (AppKind::Apt, "x"),
            (AppKind::Npm, "git+https://example.com/x.git"),
            (AppKind::Npm, "x@1 2"),
            (AppKind::Npm, "x'"),
            (AppKind::Npm, "@scope"),
            (AppKind::Pip, "https://example.com/x.whl"),
            (AppKind::Pip, "x; python_version<'3'"),
            (AppKind::Pip, "x @ file:///etc/passwd"),
            (AppKind::Pip, "x[]"),
        ] {
            let error = template(kind, &[bad]).to_app().unwrap_err();
            assert!(error.contains("packages"), "{bad}: {error}");
        }
        assert!(template(AppKind::Apt, &[]).to_app().is_err());
        assert_eq!(npm_name("left-pad"), Some("left-pad"));
        assert_eq!(npm_name("@a/b"), Some("@a/b"));
        assert_eq!(pip_name("black==24.8.0").as_deref(), Some("black"));
        assert_eq!(pip_name("Foo.Bar__baz").as_deref(), Some("foo-bar-baz"));
    }

    #[test]
    fn scripts_bring_their_own_hosts_and_path() {
        let mut script = template(AppKind::Script, &["ignored"]);
        script.script = "mkdir -p \"$PREFIX/tool/bin\"".into();
        script.hosts = vec!["github.com".into(), "*.githubusercontent.com".into()];
        script.path = vec!["tool/bin".into()];
        let app = script.to_app().unwrap();
        assert_eq!(app.install, script.script);
        assert_eq!(app.path, ["tool/bin"]);
        assert_eq!(app.hosts.len(), 2);
        assert_eq!(app.description, None);

        script.hosts = vec!["127.0.0.1".into()];
        assert!(script.to_app().unwrap_err().contains("hosts"));
        script.hosts.clear();
        script.path = vec!["../etc".into()];
        assert!(script.to_app().is_err());
        script.path.clear();
        script.script = " ".into();
        assert!(script.to_app().is_err());
        script.script = "x".repeat(MAX_SCRIPT + 1);
        assert!(script.to_app().is_err());
    }

    #[test]
    fn names_and_titles_follow_the_recipe_rules() {
        let mut bad = template(AppKind::Apt, &["tree"]);
        bad.name = "Tree!".into();
        assert!(bad.to_app().is_err());
        let mut untitled = template(AppKind::Apt, &["tree"]);
        untitled.title = "  ".into();
        assert!(untitled.to_app().is_err());
    }

    #[test]
    fn templates_read_from_the_server() {
        let json = serde_json::json!({
            "name": "tree", "title": "tree", "kind": "apt", "packages": ["tree"],
            "script": "", "hosts": [], "path": [], "requires": ["c"], "description": null
        });
        let template: AppTemplate = serde_json::from_value(json).unwrap();
        assert_eq!(template.kind, AppKind::Apt);
        assert_eq!(template.requires, ["c"]);
        assert!(
            serde_json::from_value::<AppTemplate>(serde_json::json!({
                "name": "x", "title": "x", "kind": "cargo"
            }))
            .is_err()
        );
    }
}
