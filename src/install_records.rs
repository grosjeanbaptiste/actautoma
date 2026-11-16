//! Keep cargo's own record of installed executables (`.crates.toml`, `.crates2.json`)
//! true after a binary was renamed, so `cargo uninstall NEW` keeps working.

use crate::forge::Error;
use crate::realign::{replace, Rename};
use std::fs;
use std::path::Path;

fn io(path: &Path, what: &str, e: &std::io::Error) -> Error {
    Error::Run(format!("cannot {what} {}: {e}", path.display()))
}

fn write_atomically(path: &Path, text: &str) -> Result<(), Error> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, text).map_err(|e| io(&tmp, "write", &e))?;
    fs::rename(&tmp, path).map_err(|e| io(path, "replace", &e))
}

/// One `.crates.toml` line: `"NAME VERSION (SOURCE)" = ["bin", ...]`.
fn rename_toml_line(line: &str, old: &str, new: &str, renames: &[Rename]) -> String {
    let Some(rest) = line.strip_prefix(&format!("\"{old} ")) else {
        return line.to_string();
    };
    let Some((key, bins)) = rest.split_once("\" = ") else {
        return line.to_string();
    };
    let (key, _) = replace(key, renames);
    let bins = bins.replace(&format!("\"{old}\""), &format!("\"{new}\""));
    format!("\"{new} {key}\" = {bins}")
}

fn rename_json(text: &str, old: &str, new: &str, renames: &[Rename]) -> Result<String, Error> {
    let bad = |e: serde_json::Error| Error::Run(format!("cannot parse .crates2.json: {e}"));
    let mut doc: serde_json::Value = serde_json::from_str(text).map_err(bad)?;
    if let Some(installs) = doc.get_mut("installs").and_then(|i| i.as_object_mut()) {
        for key in installs
            .keys()
            .filter(|k| k.starts_with(&format!("{old} ")))
            .cloned()
            .collect::<Vec<_>>()
        {
            let mut entry = installs.remove(&key).unwrap_or_default();
            if let Some(bins) = entry.get_mut("bins").and_then(|b| b.as_array_mut()) {
                for bin in bins.iter_mut().filter(|b| *b == old) {
                    *bin = new.into();
                }
            }
            let (rest, _) = replace(&key[old.len() + 1..], renames);
            installs.insert(format!("{new} {rest}"), entry);
        }
    }
    serde_json::to_string(&doc).map_err(bad)
}

/// Rewrites the records of `old` in `cargo_home` as `new`.
pub fn rename_install_records(
    cargo_home: &Path,
    old: &str,
    new: &str,
    renames: &[Rename],
) -> Result<(), Error> {
    let toml = cargo_home.join(".crates.toml");
    if let Ok(text) = fs::read_to_string(&toml) {
        let lines: Vec<String> = text
            .lines()
            .map(|l| rename_toml_line(l, old, new, renames))
            .collect();
        write_atomically(&toml, &(lines.join("\n") + "\n"))?;
    }
    let json = cargo_home.join(".crates2.json");
    if let Ok(text) = fs::read_to_string(&json) {
        write_atomically(&json, &rename_json(&text, old, new, renames)?)?;
    }
    Ok(())
}
