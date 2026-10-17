//! Images the harness builds itself. The build context is compiled into the
//! binary, so the CLI works without a checkout of this repository.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use crate::paths::local_data_dir;
use crate::provider::{ImageBuild, Labels, Provider};
use crate::{Error, Result};

/// Image label holding [`ImageDefinition::context_hash`], used to detect an
/// image built from an older Containerfile.
pub const CONTEXT_HASH_LABEL: &str = "io.llm-harness.context-hash";

/// A file of an image build context.
#[derive(Debug, Clone, Copy)]
pub struct ContextFile {
    /// Path inside the context directory, `/`-separated.
    pub name: &'static str,
    /// File contents.
    pub contents: &'static str,
}

/// An image with an embedded build context.
#[derive(Debug, Clone, Copy)]
pub struct ImageDefinition {
    /// Short name, also the build directory name.
    pub name: &'static str,
    /// Tag the image is built as.
    pub tag: &'static str,
    /// Build context; must include a `Containerfile`.
    pub files: &'static [ContextFile],
}

/// The base image all instances run on (`images/base`).
pub const BASE_IMAGE: ImageDefinition = ImageDefinition {
    name: "base",
    tag: "localhost/harness/base:latest",
    files: &[
        ContextFile {
            name: "Containerfile",
            contents: include_str!("../../../images/base/Containerfile"),
        },
        ContextFile {
            name: "gitconfig",
            contents: include_str!("../../../images/base/gitconfig"),
        },
        ContextFile {
            name: "profile.sh",
            contents: include_str!("../../../images/base/profile.sh"),
        },
    ],
};

/// The egress gateway (`images/gateway`), compiled from the
/// `harness-gateway` crate inside the VM. Every file in that crate's `src/`
/// must be listed here (checked by a test).
///
/// The lock file is the gateway's own, pruned from the workspace lock
/// (`scripts/gateway-lock.sh`, checked by a test): with the whole workspace
/// lock, every unrelated dependency update would mark the image outdated.
pub const GATEWAY_IMAGE: ImageDefinition = ImageDefinition {
    name: "gateway",
    tag: "localhost/harness/gateway:latest",
    files: &[
        ContextFile {
            name: "Containerfile",
            contents: include_str!("../../../images/gateway/Containerfile"),
        },
        ContextFile {
            name: "Cargo.lock",
            contents: include_str!("../../../images/gateway/Cargo.lock"),
        },
        ContextFile {
            name: "Cargo.toml",
            contents: include_str!("../../harness-gateway/Cargo.toml"),
        },
        ContextFile {
            name: "src/address.rs",
            contents: include_str!("../../harness-gateway/src/address.rs"),
        },
        ContextFile {
            name: "src/control.rs",
            contents: include_str!("../../harness-gateway/src/control.rs"),
        },
        ContextFile {
            name: "src/gateway.rs",
            contents: include_str!("../../harness-gateway/src/gateway.rs"),
        },
        ContextFile {
            name: "src/gateway/tests.rs",
            contents: include_str!("../../harness-gateway/src/gateway/tests.rs"),
        },
        ContextFile {
            name: "src/http.rs",
            contents: include_str!("../../harness-gateway/src/http.rs"),
        },
        ContextFile {
            name: "src/lib.rs",
            contents: include_str!("../../harness-gateway/src/lib.rs"),
        },
        ContextFile {
            name: "src/llm.rs",
            contents: include_str!("../../harness-gateway/src/llm.rs"),
        },
        ContextFile {
            name: "src/main.rs",
            contents: include_str!("../../harness-gateway/src/main.rs"),
        },
        ContextFile {
            name: "src/pattern.rs",
            contents: include_str!("../../harness-gateway/src/pattern.rs"),
        },
        ContextFile {
            name: "src/pem.rs",
            contents: include_str!("../../harness-gateway/src/pem.rs"),
        },
        ContextFile {
            name: "src/policy.rs",
            contents: include_str!("../../harness-gateway/src/policy.rs"),
        },
        ContextFile {
            name: "src/proxy.rs",
            contents: include_str!("../../harness-gateway/src/proxy.rs"),
        },
        ContextFile {
            name: "src/upstream.rs",
            contents: include_str!("../../harness-gateway/src/upstream.rs"),
        },
        ContextFile {
            name: "src/upstream/tls.rs",
            contents: include_str!("../../harness-gateway/src/upstream/tls.rs"),
        },
        ContextFile {
            name: "src/upstream/tls/tests.rs",
            contents: include_str!("../../harness-gateway/src/upstream/tls/tests.rs"),
        },
    ],
};

/// All images, in build order.
pub const IMAGES: &[ImageDefinition] = &[BASE_IMAGE, GATEWAY_IMAGE];

/// Whether an image exists and matches the embedded build context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStatus {
    /// Not built yet.
    Missing,
    /// Built from a different build context (or by hand).
    Outdated,
    /// Built from exactly the embedded build context.
    Current,
}

/// Options for [`ImageDefinition::build`].
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    /// Pull newer versions of the `FROM` images.
    pub pull: bool,
    /// Ignore the layer cache.
    pub no_cache: bool,
}

impl ImageDefinition {
    /// Stable fingerprint of the build context (FNV-1a, 64 bit). Not a
    /// security measure, only change detection.
    pub fn context_hash(&self) -> String {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        for file in self.files {
            // NUL separators keep ("ab","c") and ("a","bc") apart.
            for byte in file
                .name
                .bytes()
                .chain([0])
                .chain(file.contents.bytes())
                .chain([0])
            {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(PRIME);
            }
        }
        let mut hex = String::with_capacity(16);
        let _ = write!(hex, "{hash:016x}");
        hex
    }

    /// Writes the build context into `dir`, replacing whatever was there.
    pub fn write_context(&self, dir: &Path) -> Result<()> {
        match fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(format!("{} leeren", dir.display()), e)),
        }
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("{} anlegen", dir.display()), e))?;
        for file in self.files {
            let path = file
                .name
                .split('/')
                .fold(dir.to_path_buf(), |p, part| p.join(part));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| Error::io(format!("{} anlegen", parent.display()), e))?;
            }
            fs::write(&path, file.contents)
                .map_err(|e| Error::io(format!("{} schreiben", path.display()), e))?;
        }
        Ok(())
    }

    /// Compares the image in the VM with the embedded build context.
    pub fn status(&self, provider: &dyn Provider) -> Result<ImageStatus> {
        Ok(match provider.image_labels(self.tag)? {
            None => ImageStatus::Missing,
            Some(labels) if labels.get(CONTEXT_HASH_LABEL) == Some(&self.context_hash()) => {
                ImageStatus::Current
            }
            Some(_) => ImageStatus::Outdated,
        })
    }

    /// Builds the image in the VM. The context is written to
    /// `%LOCALAPPDATA%\llm-harness\build\<name>` and uploaded by podman.
    pub fn build(&self, provider: &dyn Provider, options: BuildOptions) -> Result<()> {
        let context = local_data_dir()?.join("build").join(self.name);
        self.write_context(&context)?;
        let labels: Labels = [(CONTEXT_HASH_LABEL.to_owned(), self.context_hash())].into();
        provider.build_image(&ImageBuild {
            tag: self.tag.to_owned(),
            context,
            labels,
            pull: options.pull,
            no_cache: options.no_cache,
            ..ImageBuild::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_context_is_complete() {
        let names: Vec<_> = BASE_IMAGE.files.iter().map(|f| f.name).collect();
        assert!(names.contains(&"Containerfile"));
        // Every file the Containerfile copies must be embedded.
        let containerfile = BASE_IMAGE
            .files
            .iter()
            .find(|f| f.name == "Containerfile")
            .unwrap()
            .contents;
        for line in containerfile.lines().filter(|l| l.starts_with("COPY ")) {
            if line.contains("--from=") {
                continue;
            }
            let source = line
                .split_whitespace()
                .skip(1)
                .find(|word| !word.starts_with("--"))
                .unwrap();
            assert!(names.contains(&source), "{source} is not embedded");
        }
    }

    #[test]
    fn gateway_context_is_complete() {
        let embedded: Vec<&str> = GATEWAY_IMAGE.files.iter().map(|f| f.name).collect();
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../harness-gateway");
        let mut on_disk = vec![
            "Containerfile".to_owned(),
            "Cargo.lock".to_owned(),
            "Cargo.toml".to_owned(),
        ];
        let mut dirs = vec![crate_dir.join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else {
                    let relative = path.strip_prefix(&crate_dir).unwrap();
                    let parts: Vec<_> = relative
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect();
                    on_disk.push(parts.join("/"));
                }
            }
        }
        for file in &on_disk {
            assert!(embedded.contains(&file.as_str()), "{file} is not embedded");
        }
        assert_eq!(
            embedded.len(),
            on_disk.len(),
            "stale entries in GATEWAY_IMAGE"
        );
        // The embedded copy is what the build uses, not a stale snapshot.
        for file in GATEWAY_IMAGE
            .files
            .iter()
            .filter(|f| f.name.starts_with("src/"))
        {
            let disk = fs::read_to_string(crate_dir.join(file.name)).unwrap();
            assert_eq!(disk, file.contents, "{}", file.name);
        }
    }

    /// `[[package]]` of a `Cargo.lock`.
    #[derive(serde::Deserialize)]
    struct LockedPackage {
        name: String,
        version: String,
        source: Option<String>,
        checksum: Option<String>,
        #[serde(default)]
        dependencies: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct LockFile {
        package: Vec<LockedPackage>,
    }

    impl LockFile {
        fn parse(text: &str) -> LockFile {
            toml::from_str(text).unwrap()
        }

        fn get(&self, name: &str, version: &str) -> Option<&LockedPackage> {
            self.package
                .iter()
                .find(|p| p.name == name && p.version == version)
        }

        /// `name`, `name version` or `name version (source)` as listed in
        /// `dependencies`; the bare name only appears when one version is
        /// locked, so the reference is unambiguous within its own file.
        fn resolve(&self, reference: &str) -> (String, String) {
            let mut words = reference.split(' ');
            let name = words.next().unwrap();
            let version = match words.next() {
                Some(version) => version.to_owned(),
                None => {
                    let matches: Vec<_> = self.package.iter().filter(|p| p.name == name).collect();
                    assert_eq!(matches.len(), 1, "{reference} is ambiguous");
                    matches[0].version.clone()
                }
            };
            (name.to_owned(), version)
        }

        fn dependencies(&self, package: &LockedPackage) -> Vec<(String, String)> {
            let mut list: Vec<_> = package
                .dependencies
                .iter()
                .map(|d| self.resolve(d))
                .collect();
            list.sort();
            list
        }
    }

    /// The image is built with `--locked` from images/gateway/Cargo.lock.
    /// It must be exactly the workspace lock pruned to the gateway: same
    /// versions and checksums as what the workspace builds and tests. Not
    /// covered here: different features of the same versions (edges the
    /// workspace adds through other crates make that ambiguous); the
    /// workflow `gateway.yml` builds the image context with `--locked`.
    #[test]
    fn gateway_lock_matches_the_workspace() {
        let hint = "images/gateway/Cargo.lock passt nicht zu Cargo.lock; neu erzeugen mit \
                    `bash scripts/gateway-lock.sh`";
        let lock = GATEWAY_IMAGE
            .files
            .iter()
            .find(|f| f.name == "Cargo.lock")
            .unwrap()
            .contents;
        let gateway = LockFile::parse(lock);
        let workspace_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let workspace = LockFile::parse(&fs::read_to_string(workspace_path).unwrap());

        for package in &gateway.package {
            let label = format!("{} {}", package.name, package.version);
            let theirs = workspace
                .get(&package.name, &package.version)
                .unwrap_or_else(|| panic!("{label} fehlt im Workspace. {hint}"));
            assert_eq!(package.source, theirs.source, "{label}: {hint}");
            assert_eq!(package.checksum, theirs.checksum, "{label}: {hint}");
            let ours = gateway.dependencies(package);
            let all = workspace.dependencies(theirs);
            if package.name == "harness-gateway" {
                // The crate's own dependencies come from its manifest alone.
                assert_eq!(ours, all, "{label}: {hint}");
            } else {
                for dependency in &ours {
                    assert!(all.contains(dependency), "{label} → {dependency:?}: {hint}");
                }
            }
        }

        // Nothing beyond what the gateway reaches; cargo would want to drop it.
        let mut reached = vec![gateway.resolve("harness-gateway")];
        let mut index = 0;
        while let Some((name, version)) = reached.get(index).cloned() {
            let package = gateway
                .get(&name, &version)
                .unwrap_or_else(|| panic!("{name} {version} fehlt. {hint}"));
            for dependency in gateway.dependencies(package) {
                if !reached.contains(&dependency) {
                    reached.push(dependency);
                }
            }
            index += 1;
        }
        assert_eq!(
            reached.len(),
            gateway.package.len(),
            "überzählige Pakete. {hint}"
        );
    }

    #[test]
    fn hash_is_stable_and_sensitive() {
        let a = ImageDefinition {
            name: "t",
            tag: "localhost/t",
            files: &[ContextFile {
                name: "Containerfile",
                contents: "FROM x",
            }],
        };
        let b = ImageDefinition {
            files: &[ContextFile {
                name: "Containerfile",
                contents: "FROM y",
            }],
            ..a
        };
        assert_eq!(a.context_hash(), a.context_hash());
        assert_eq!(a.context_hash().len(), 16);
        assert_ne!(a.context_hash(), b.context_hash());
    }

    #[test]
    fn writes_context() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ctx");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("stale"), "x").unwrap();
        BASE_IMAGE.write_context(&target).unwrap();
        assert!(target.join("Containerfile").is_file());
        assert!(target.join("gitconfig").is_file());
        assert!(!target.join("stale").exists());

        GATEWAY_IMAGE.write_context(&target).unwrap();
        assert!(
            target
                .join("src")
                .join("gateway")
                .join("tests.rs")
                .is_file()
        );
        assert!(!target.join("gitconfig").exists());
    }
}
