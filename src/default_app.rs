//! Registers rawv with Windows as a program for its file types, for the current user only.
//!
//! Registration makes rawv appear in "Open with" and under Settings > Apps > Default apps.
//! Windows does not let a program make itself the default: the user picks rawv there.

use std::io;
use std::path::Path;

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS, RegType};
use winreg::types::ToRegValue;

use crate::media::formats;

/// Puts every key below this one instead of the user's real settings, for tests.
const ROOT_VARIABLE: &str = "RAWV_REGISTRY_ROOT";

pub const PROG_ID: &str = "rawv.image";
pub const APP_NAME: &str = "rawv";
const EXE_NAME: &str = "rawv.exe";
const CAPABILITIES: &str = r"Software\rawv\Capabilities";
const REGISTERED_APPLICATIONS: &str = r"Software\RegisteredApplications";
const CLASSES: &str = r"Software\Classes";
/// The Settings page that lists the file types a registered program can open.
const SETTINGS_PAGE: &str = "ms-settings:defaultapps?registeredAppUser=rawv";

/// Where the keys go: the user's own settings, or below the test root.
struct Hive {
    user: RegKey,
    prefix: String,
}

impl Hive {
    fn open() -> Hive {
        let prefix = std::env::var(ROOT_VARIABLE)
            .ok()
            .filter(|root| !root.is_empty())
            .map(|root| format!(r"{}\", root.trim_end_matches('\\')))
            .unwrap_or_default();
        Hive {
            user: RegKey::predef(HKEY_CURRENT_USER),
            prefix,
        }
    }

    fn is_real(&self) -> bool {
        self.prefix.is_empty()
    }

    fn path(&self, path: &str) -> String {
        format!("{}{path}", self.prefix)
    }

    fn set<T: ToRegValue>(&self, key: &str, name: &str, value: &T) -> io::Result<()> {
        let (key, _) = self.user.create_subkey(self.path(key))?;
        key.set_value(name, value)
    }

    /// Creates `key` with an empty value `name` of type REG_NONE, the form Explorer expects
    /// in `OpenWithProgids`.
    fn set_marker(&self, key: &str, name: &str) -> io::Result<()> {
        let (key, _) = self.user.create_subkey(self.path(key))?;
        key.set_raw_value(
            name,
            &winreg::RegValue {
                bytes: std::borrow::Cow::Borrowed(&[]),
                vtype: RegType::REG_NONE,
            },
        )
    }

    fn delete_tree(&self, key: &str) -> io::Result<()> {
        ignore_missing(self.user.delete_subkey_all(self.path(key)))
    }

    fn delete_value(&self, key: &str, name: &str) -> io::Result<()> {
        match self
            .user
            .open_subkey_with_flags(self.path(key), KEY_ALL_ACCESS)
        {
            Ok(key) => ignore_missing(key.delete_value(name)),
            Err(error) => ignore_missing(Err(error)),
        }
    }

    /// Deletes `key` if nothing is left in it.
    fn delete_if_empty(&self, key: &str) -> io::Result<()> {
        let Ok(opened) = self.user.open_subkey(self.path(key)) else {
            return Ok(());
        };
        let info = opened.query_info()?;
        if info.sub_keys == 0 && info.values == 0 {
            ignore_missing(self.user.delete_subkey(self.path(key)))
        } else {
            Ok(())
        }
    }
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// What `register` did.
#[derive(Debug)]
pub struct Registration {
    pub extensions: usize,
    /// Whether the keys went into the user's real settings rather than below a test root.
    pub real: bool,
}

/// Registers `exe` as the program that opens rawv's file types.
pub fn register(exe: &Path) -> io::Result<Registration> {
    if !exe.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no viewer at {}", exe.display()),
        ));
    }
    let exe = exe.display();
    let command = format!("\"{exe}\" \"%1\"");
    let hive = Hive::open();

    let prog_id = format!(r"{CLASSES}\{PROG_ID}");
    hive.set(&prog_id, "", &"rawv image")?;
    hive.set(
        &format!(r"{prog_id}\DefaultIcon"),
        "",
        &format!("\"{exe}\",0"),
    )?;
    hive.set(&format!(r"{prog_id}\shell\open\command"), "", &command)?;

    let application = format!(r"{CLASSES}\Applications\{EXE_NAME}");
    hive.set(&application, "FriendlyAppName", &APP_NAME)?;
    hive.set(&format!(r"{application}\shell\open\command"), "", &command)?;

    hive.set(CAPABILITIES, "ApplicationName", &APP_NAME)?;
    hive.set(
        CAPABILITIES,
        "ApplicationDescription",
        &"Fast photo and RAW viewer",
    )?;
    hive.set(REGISTERED_APPLICATIONS, APP_NAME, &CAPABILITIES)?;

    let mut extensions = 0;
    for extension in formats::extensions() {
        let dotted = format!(".{extension}");
        hive.set(
            &format!(r"{CAPABILITIES}\FileAssociations"),
            &dotted,
            &PROG_ID,
        )?;
        hive.set(&format!(r"{application}\SupportedTypes"), &dotted, &"")?;
        hive.set_marker(&format!(r"{CLASSES}\{dotted}\OpenWithProgids"), PROG_ID)?;
        extensions += 1;
    }
    Ok(Registration {
        extensions,
        real: hive.is_real(),
    })
}

/// Removes everything `register` wrote. A file type the user had set to rawv falls back to
/// Windows asking which program to use.
pub fn unregister() -> io::Result<()> {
    let hive = Hive::open();
    hive.delete_value(REGISTERED_APPLICATIONS, APP_NAME)?;
    hive.delete_tree(CAPABILITIES)?;
    hive.delete_if_empty(r"Software\rawv")?;
    hive.delete_tree(&format!(r"{CLASSES}\Applications\{EXE_NAME}"))?;
    hive.delete_tree(&format!(r"{CLASSES}\{PROG_ID}"))?;
    for extension in formats::extensions() {
        let open_with = format!(r"{CLASSES}\.{extension}\OpenWithProgids");
        hive.delete_value(&open_with, PROG_ID)?;
        hive.delete_if_empty(&open_with)?;
        hive.delete_if_empty(&format!(r"{CLASSES}\.{extension}"))?;
    }
    Ok(())
}

/// Opens the Settings page where the user picks rawv for each file type.
pub fn open_settings() -> io::Result<()> {
    std::process::Command::new("explorer.exe")
        .arg(SETTINGS_PAGE)
        .spawn()
        .map(|_| ())
}
