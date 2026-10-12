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
    /// File name inside the context directory.
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
            let path = dir.join(file.name);
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
    }
}
