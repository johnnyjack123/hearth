//! Program files from this computer for the app catalog (step 3b): a Linux
//! program, an AppImage, a Debian package or an archive with programs.
//!
//! Only the user decides on this computer which file goes into the VM: the
//! browser can ask, but `harness app approve` (or the tray icon) makes the
//! copy. Agents of every project that switches the app on can read the
//! file, so [`inspect`] already refuses whatever is not a program, such as
//! an SSH key or a document, before anyone is asked.

use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{AppConfig, ConfigDir, valid_file_name};
use crate::lang::tr;
use crate::{Error, Result, tr};

/// Largest file taken; an AppImage of a big program has a few hundred MB.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// What a program file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A Linux program (ELF) for the VM's architecture.
    Program,
    /// An AppImage; it is unpacked when installing, the VM has no FUSE.
    AppImage,
    /// A Debian package; its dependencies come from the archive.
    Deb,
    /// A tar archive, gzip or xz compressed.
    Tar,
    /// A zip archive.
    Zip,
}

impl FileKind {
    /// For people, e.g. in the question before approving; in the language
    /// of this computer, which the tray and the browser show as it is.
    pub fn label(self) -> String {
        let arch = vm_architecture();
        match self {
            FileKind::Program => tr!("Linux-Programm ({arch})", "Linux program ({arch})"),
            FileKind::AppImage => format!("AppImage ({arch})"),
            FileKind::Deb => tr("Debian-Paket", "Debian package").to_owned(),
            FileKind::Tar => tr("Archiv (tar)", "Archive (tar)").to_owned(),
            FileKind::Zip => tr("Archiv (zip)", "Archive (zip)").to_owned(),
        }
    }
}

/// A file that may become an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    /// What it is.
    pub kind: FileKind,
    /// Its size in bytes.
    pub size: u64,
}

/// The VM's architecture: Hyper-V runs it on the host's own.
fn vm_architecture() -> &'static str {
    std::env::consts::ARCH
}

/// ELF machine numbers of the architectures harness runs on.
fn elf_machine(machine: u16) -> Option<&'static str> {
    match machine {
        0x3e => Some("x86_64"),
        0xb7 => Some("aarch64"),
        _ => None,
    }
}

/// Reads what `path` is from its first bytes. Fails with a message for
/// people for anything that is not a Linux program file for the VM.
pub fn inspect(path: &Path) -> std::result::Result<FileInfo, String> {
    let metadata = fs::metadata(path).map_err(|e| unreadable(path, &e))?;
    if !metadata.is_file() {
        return Err(tr!(
            "{} ist keine Datei",
            "{} is not a file",
            path.display()
        ));
    }
    let size = metadata.len();
    if size > MAX_FILE_BYTES {
        return Err(tr!(
            "{} ist zu groß ({} MB, höchstens {} MB)",
            "{} is too large ({} MB, at most {} MB)",
            path.display(),
            size / 1024 / 1024,
            MAX_FILE_BYTES / 1024 / 1024
        ));
    }
    let mut head = [0u8; 64];
    let read = fs::File::open(path)
        .and_then(|mut file| file.read(&mut head))
        .map_err(|e| unreadable(path, &e))?;
    let kind = kind_of(&head[..read])?;
    Ok(FileInfo { kind, size })
}

fn unreadable(path: &Path, error: &std::io::Error) -> String {
    tr!(
        "{} nicht lesbar: {error}",
        "{} cannot be read: {error}",
        path.display()
    )
}

fn kind_of(head: &[u8]) -> std::result::Result<FileKind, String> {
    if head.starts_with(b"\x7fELF") {
        // 64 bit, little endian: what both architectures use.
        if head.len() < 20 || head[4] != 2 || head[5] != 1 {
            return Err(tr(
                "ELF-Datei, aber kein 64-Bit-Programm",
                "ELF file, but not a 64-bit program",
            )
            .to_owned());
        }
        let machine = u16::from_le_bytes([head[18], head[19]]);
        let built_for =
            elf_machine(machine).unwrap_or(tr("eine andere Architektur", "another architecture"));
        if built_for != vm_architecture() {
            return Err(tr!(
                "Das Programm ist für {built_for} gebaut, die VM läuft auf {}",
                "The program is built for {built_for}, the VM runs on {}",
                vm_architecture()
            ));
        }
        // AppImages carry "AI" and their type in the ELF padding.
        return Ok(if &head[8..10] == b"AI" {
            FileKind::AppImage
        } else {
            FileKind::Program
        });
    }
    if head.starts_with(b"!<arch>\ndebian-binary") {
        return Ok(FileKind::Deb);
    }
    if head.starts_with(&[0x1f, 0x8b]) || head.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0]) {
        return Ok(FileKind::Tar);
    }
    if head.starts_with(b"PK\x03\x04") {
        return Ok(FileKind::Zip);
    }
    Err(tr(
        "Keine Linux-Programmdatei: erwartet wird ein Programm (ELF), eine AppImage, ein \
         .deb oder ein Archiv (.tar.gz, .tar.xz, .zip)",
        "Not a Linux program file: expected a program (ELF), an AppImage, a .deb or an \
         archive (.tar.gz, .tar.xz, .zip)",
    )
    .to_owned())
}

/// The file name the copy gets: the original's, with everything but plain
/// characters replaced. A Debian package keeps its `.deb`, which apt needs.
pub fn copy_name(path: &Path, kind: FileKind) -> String {
    let original = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut name: String = original
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    name = name
        .trim_start_matches(['.', '-'])
        .chars()
        .take(90)
        .collect();
    if name.is_empty() {
        name = "datei".to_owned();
    }
    if kind == FileKind::Deb && !name.ends_with(".deb") {
        name.push_str(".deb");
    }
    debug_assert!(valid_file_name(&name));
    name
}

/// The install script for a file of `kind`; programs and AppImages become
/// the command `command`.
fn install_script(kind: FileKind, command: &str, approved: u64, size: u64) -> String {
    // Changes with every approval, so a new file installs anew.
    let mut script = format!("# Freigegeben am Rechner: {approved}, {size} Bytes\n");
    script.push_str(&match kind {
        FileKind::Program => {
            format!("harness_program '{command}'\nharness_file_version \"$PREFIX/bin/{command}\"\n")
        }
        FileKind::AppImage => format!(
            "harness_appimage '{command}'\nharness_file_version \"$PREFIX/bin/{command}\"\n"
        ),
        FileKind::Deb => "harness_apt \"$HARNESS_FILE\"\n\
             harness_wrap usr/bin usr/sbin usr/games bin sbin\n\
             harness_version \"$(dpkg-deb --show --showformat='${Version}' \"$HARNESS_FILE\")\"\n"
            .to_owned(),
        FileKind::Tar => "harness_unpack tar\nharness_file_version\n".to_owned(),
        FileKind::Zip => "harness_unpack zip\nharness_file_version\n".to_owned(),
    });
    script
}

/// Writes the app `name` for the program file at `path`: a copy into
/// `app-files/<name>/` and the recipe `apps/<name>.toml`. An earlier file
/// app of that name is replaced; a recipe without `[file]` is not, nor a
/// catalog app. Returns the new recipe.
pub fn add(config: &ConfigDir, name: &str, title: &str, path: &Path) -> Result<AppConfig> {
    let invalid = |message: String| Error::ConfigInvalid {
        path: path.to_owned(),
        message,
    };
    let info = inspect(path).map_err(Error::Unsupported)?;
    match config.app(name) {
        Ok(existing) if existing.file.is_none() => {
            return Err(Error::Unsupported(tr!(
                "{name} gibt es schon ({}); bitte einen anderen Namen wählen",
                "{name} exists already ({}); please choose another name",
                existing.title
            )));
        }
        Ok(_) | Err(Error::AppUnknown(_)) => {}
        Err(error) => return Err(error),
    }
    let title = title.trim();
    let file_name = copy_name(path, info.kind);
    let approved = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let hosts = if info.kind == FileKind::Deb {
        "[\"deb.debian.org\"]"
    } else {
        "[]"
    };
    let source = path.display().to_string();
    let recipe = format!(
        "# Programm aus einer Datei dieses Rechners, freigegeben mit harness app approve/add.\n\
         # Original: {source}\n\
         # Installiert wird die Kopie in app-files/{plain}/; ändern heißt neu freigeben.\n\
         name = {name}\n\
         title = {title}\n\
         description = {description}\n\
         hosts = {hosts}\n\
         path = [\"bin\"]\n\
         install = '''\n{install}'''\n\
         \n\
         [file]\n\
         name = {file}\n",
        source = source.replace(['\n', '\r'], " "),
        plain = name,
        name = quoted(name),
        title = quoted(title),
        description = quoted(&tr!(
            "Datei vom Rechner: {}",
            "File from this computer: {}",
            info.kind.label()
        )),
        install = install_script(info.kind, name, approved, info.size),
        file = quoted(&file_name),
    );
    // Checked before anything is written: an invalid title or name never
    // leaves half an app behind.
    let mut app: AppConfig = toml::from_str(&recipe).map_err(|e| invalid(e.to_string()))?;
    crate::config::check_app(&app).map_err(invalid)?;

    let dir = config.app_files_dir().join(name);
    let partial = config.app_files_dir().join(format!("{name}.partial"));
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial)
        .map_err(|e| Error::io(tr!("{} anlegen", "Creating {}", partial.display()), e))?;
    fs::copy(path, partial.join(&file_name))
        .map_err(|e| Error::io(tr!("{} kopieren", "Copying {}", path.display()), e))?;
    // The copy must still be what was checked; whatever wrote the original
    // in between does not get through.
    if inspect(&partial.join(&file_name)).map(|copied| copied.kind) != Ok(info.kind) {
        let _ = fs::remove_dir_all(&partial);
        return Err(invalid(
            tr(
                "Die Datei hat sich beim Kopieren geändert",
                "The file changed while it was being copied",
            )
            .to_owned(),
        ));
    }
    let _ = fs::remove_dir_all(&dir);
    fs::rename(&partial, &dir)
        .map_err(|e| Error::io(tr!("{} anlegen", "Creating {}", dir.display()), e))?;
    let apps = config.apps_dir();
    fs::create_dir_all(&apps)
        .map_err(|e| Error::io(tr!("{} anlegen", "Creating {}", apps.display()), e))?;
    let file = apps.join(format!("{name}.toml"));
    fs::write(&file, &recipe)
        .map_err(|e| Error::io(tr!("{} schreiben", "Writing {}", file.display()), e))?;
    if let Some(program) = &mut app.file {
        program.host = dir.join(&program.name);
    }
    app.source = crate::config::AppSource::Local;
    Ok(app)
}

/// Deletes a file app: its recipe and its copy. Other recipes stay.
pub fn forget(config: &ConfigDir, name: &str) -> Result<()> {
    let app = config.app(name)?;
    if app.file.is_none() {
        return Err(Error::Unsupported(tr!(
            "{name} ist kein Programm aus einer Datei; Katalog-Apps schaltet man aus, \
             eigene Rezepte löscht man selbst in {}",
            "{name} is not a program from a file; switch catalog apps off instead, and \
             delete your own recipes yourself in {}",
            config.apps_dir().display()
        )));
    }
    let file = config.apps_dir().join(format!("{name}.toml"));
    fs::remove_file(&file)
        .map_err(|e| Error::io(tr!("{} löschen", "Deleting {}", file.display()), e))?;
    let _ = fs::remove_dir_all(config.app_files_dir().join(name));
    Ok(())
}

/// The name a file suggests for its app, without extension and version:
/// `rg`, `tool` for `tool-1.2.tar.gz`, `fd` for `fd_10.2.0_arm64.deb`.
pub fn suggested_name(path: &Path) -> String {
    let file = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut stem = file.as_str();
    for suffix in [
        ".tar.gz",
        ".tgz",
        ".tar.xz",
        ".txz",
        ".zip",
        ".deb",
        ".appimage",
    ] {
        if let Some(rest) = stem.strip_suffix(suffix) {
            stem = rest;
            break;
        }
    }
    // Debian packages: name_version_arch; others: name-1.2.3 or name-v1.2.
    stem = stem.split('_').next().unwrap_or(stem);
    let parts: Vec<&str> = stem.split(['-', '.']).collect();
    let keep = parts
        .iter()
        .position(|part| {
            part.starts_with(|c: char| c.is_ascii_digit())
                || (part.starts_with('v') && part[1..].starts_with(|c: char| c.is_ascii_digit()))
        })
        .filter(|&at| at > 0)
        .unwrap_or(parts.len());
    let mut name = String::new();
    for c in parts[..keep].join("-").chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c);
        } else if !name.is_empty() && !name.ends_with('-') {
            name.push('-');
        }
    }
    name.truncate(30);
    let name = name.trim_matches('-').to_owned();
    if name.is_empty() {
        "programm".to_owned()
    } else {
        name
    }
}

/// A TOML basic string.
fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn elf(machine: u16, appimage: bool) -> Vec<u8> {
        let mut head = vec![0u8; 64];
        head[..4].copy_from_slice(b"\x7fELF");
        head[4] = 2;
        head[5] = 1;
        if appimage {
            head[8..11].copy_from_slice(b"AI\x02");
        }
        head[18..20].copy_from_slice(&machine.to_le_bytes());
        head
    }

    fn own_machine() -> u16 {
        if vm_architecture() == "aarch64" {
            0xb7
        } else {
            0x3e
        }
    }

    fn other_machine() -> u16 {
        if vm_architecture() == "aarch64" {
            0x3e
        } else {
            0xb7
        }
    }

    #[test]
    fn only_program_files_pass() {
        assert_eq!(kind_of(&elf(own_machine(), false)), Ok(FileKind::Program));
        assert_eq!(kind_of(&elf(own_machine(), true)), Ok(FileKind::AppImage));
        let foreign = kind_of(&elf(other_machine(), false)).unwrap_err();
        assert!(foreign.contains("gebaut"), "{foreign}");
        let mut thirty_two = elf(own_machine(), false);
        thirty_two[4] = 1;
        assert!(kind_of(&thirty_two).is_err());
        assert_eq!(kind_of(b"!<arch>\ndebian-binary   "), Ok(FileKind::Deb));
        assert_eq!(kind_of(&[0x1f, 0x8b, 8, 0]), Ok(FileKind::Tar));
        assert_eq!(kind_of(b"\xfd7zXZ\x00rest"), Ok(FileKind::Tar));
        assert_eq!(kind_of(b"PK\x03\x04rest"), Ok(FileKind::Zip));
        // Keys, documents, scripts, Windows programs: never.
        for refused in [
            &b"-----BEGIN OPENSSH PRIVATE KEY-----"[..],
            b"ssh-ed25519 AAAA",
            b"#!/bin/sh\necho",
            b"MZ\x90\x00",
            b"%PDF-1.7",
            b"!<arch>\nother",
            b"",
        ] {
            assert!(kind_of(refused).is_err(), "{refused:?}");
        }
    }

    #[test]
    fn names_come_from_the_file() {
        for (file, name) in [
            ("C:/x/rg", "rg"),
            ("C:/x/tool-1.2.tar.gz", "tool"),
            ("C:/x/fd_10.2.0_arm64.deb", "fd"),
            ("C:/x/harness-greet_1.2.3_all.deb", "harness-greet"),
            ("C:/x/Obsidian-1.6.7-arm64.AppImage", "obsidian"),
            ("C:/x/my tool v2.zip", "my-tool-v2"),
            ("C:/x/node-v22.1.0-linux-arm64.tar.xz", "node"),
            ("C:/x/1.2.zip", "1-2"),
            ("C:/x/äöü", "programm"),
        ] {
            assert_eq!(suggested_name(Path::new(file)), name, "{file}");
        }
    }

    #[test]
    fn copies_get_plain_names() {
        assert_eq!(copy_name(Path::new("C:/x/rg"), FileKind::Program), "rg");
        assert_eq!(
            copy_name(Path::new("C:/x/Mein Tool (1).AppImage"), FileKind::AppImage),
            "Mein_Tool__1_.AppImage"
        );
        assert_eq!(
            copy_name(Path::new("C:/x/.hidden"), FileKind::Program),
            "hidden"
        );
        assert_eq!(copy_name(Path::new("C:/x/pkg"), FileKind::Deb), "pkg.deb");
        assert_eq!(copy_name(Path::new("C:/x/äöü"), FileKind::Zip), "___");
    }

    #[test]
    fn approved_files_become_apps_with_their_own_copy() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().join("home"));
        let original = temp.path().join("my tool");
        fs::write(&original, elf(own_machine(), false)).unwrap();

        let app = add(&config, "mytool", "Mein \"Tool\"", &original).unwrap();
        assert_eq!(app.title, "Mein \"Tool\"");
        assert!(
            app.install.contains("harness_program 'mytool'"),
            "{}",
            app.install
        );
        assert!(app.hosts.is_empty());
        let copy = config.app_files_dir().join("mytool").join("my_tool");
        assert_eq!(app.file.as_ref().unwrap().host, copy);
        assert!(copy.exists());
        // Read back like any recipe, as the computer's own.
        let loaded = config.app("mytool").unwrap();
        assert_eq!(loaded.file.unwrap().host, copy);
        assert_eq!(loaded.source, crate::config::AppSource::Local);

        // Changing the original changes nothing until approved again.
        fs::write(&original, b"#!/bin/sh\nevil").unwrap();
        assert_eq!(fs::read(&copy).unwrap(), elf(own_machine(), false));
        assert!(add(&config, "mytool", "Mein Tool", &original).is_err());
        assert!(copy.exists());

        // A new approval installs anew: the script changes with it.
        fs::write(&original, elf(own_machine(), false)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let again = add(&config, "mytool", "Mein Tool", &original).unwrap();
        assert_ne!(again.install, app.install);

        // Names of the catalog stay theirs; forgetting removes copy and recipe.
        assert!(add(&config, "rust", "Rust", &original).is_err());
        assert!(forget(&config, "rust").is_err());
        forget(&config, "mytool").unwrap();
        assert!(matches!(config.app("mytool"), Err(Error::AppUnknown(_))));
        assert!(!config.app_files_dir().join("mytool").exists());
    }

    #[test]
    fn packages_and_archives_install_with_their_helpers() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().join("home"));
        let deb = temp.path().join("tool_1.0_arm64");
        fs::write(&deb, b"!<arch>\ndebian-binary   ").unwrap();
        let app = add(&config, "tool", "Tool", &deb).unwrap();
        assert_eq!(app.file.as_ref().unwrap().name, "tool_1.0_arm64.deb");
        assert!(app.install.contains("harness_apt \"$HARNESS_FILE\""));
        assert_eq!(app.hosts[0].to_string(), "deb.debian.org");

        let zip = temp.path().join("tools.zip");
        fs::write(&zip, b"PK\x03\x04rest").unwrap();
        let app = add(&config, "tools", "Tools", &zip).unwrap();
        assert!(app.install.contains("harness_unpack zip"));
        assert_eq!(app.path, ["bin"]);
    }
}
