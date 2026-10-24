//! Finding the source map of a generated file on disk.
//!
//! rustc reports a workspace member's files relative to the workspace
//! root and every other file by absolute path, so a relative name is
//! resolved against the directories above the package's `Cargo.toml`
//! (`manifest_path` of the cargo message), nearest first.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use outou_sourcemap::SourceMapJson;

use super::translate::{Resolved, Resolver, SourceFile, Unit};

const GENERATED_DIR: &str = ".generated";

/// Reads `<generated>.rs.map.json` next to each generated file, and the
/// files it names, caching the result per generated file.
#[derive(Debug, Default)]
pub struct DiskResolver {
    cache: HashMap<PathBuf, Resolved>,
}

impl DiskResolver {
    /// An empty resolver.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Resolver for DiskResolver {
    fn resolve(&mut self, file_name: &str, manifest_path: &Path) -> Resolved {
        let Some(path) = locate_file(file_name, manifest_path) else {
            // A relative name nothing contains: still a generated file if
            // its path says so (a deleted file), otherwise not ours.
            return if is_generated(&lexically_normalized(Path::new(file_name))) {
                Resolved::GeneratedWithoutMap
            } else {
                Resolved::NotGenerated
            };
        };
        if !is_generated(&path) {
            return Resolved::NotGenerated;
        }
        self.cache
            .entry(path.clone())
            .or_insert_with(|| load_unit(&path))
            .clone()
    }
}

/// The absolute, `..`-free path of `file_name`.
fn locate_file(file_name: &str, manifest_path: &Path) -> Option<PathBuf> {
    let name = Path::new(file_name);
    if name.is_absolute() {
        return Some(lexically_normalized(name));
    }
    manifest_path
        .parent()?
        .ancestors()
        .map(|dir| lexically_normalized(&dir.join(name)))
        .find(|candidate| candidate.exists())
}

fn is_generated(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == GENERATED_DIR)
}

/// Resolves `.` and `..` without touching the file system, so that
/// `src/.generated/../state.rs` becomes `src/state.rs` even though the
/// plain module lives outside `.generated`.
fn lexically_normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn load_unit(generated: &Path) -> Resolved {
    match read_unit(generated) {
        Some(unit) => Resolved::Unit(Rc::new(unit)),
        None => Resolved::GeneratedWithoutMap,
    }
}

fn read_unit(generated: &Path) -> Option<Unit> {
    let mut map_name = generated.as_os_str().to_owned();
    map_name.push(".map.json");
    let json: SourceMapJson =
        serde_json::from_str(&std::fs::read_to_string(PathBuf::from(map_name)).ok()?).ok()?;
    let generated_text = std::fs::read_to_string(generated).ok()?;

    // `generated` inside the map is crate-relative, so the crate
    // directory is whatever precedes it.
    let crate_dir = generated.ancestors().find(|dir| {
        dir.join(&json.generated)
            .components()
            .eq(generated.components())
    })?;
    let mut sources = Vec::with_capacity(json.sources.len());
    for relative in &json.sources {
        let path = crate_dir.join(relative);
        sources.push(SourceFile {
            display: display_path(&path),
            text: std::fs::read_to_string(&path).ok()?,
        });
    }
    let texts: Vec<&str> = sources.iter().map(|source| source.text.as_str()).collect();
    let source_map = json.into_source_map(&generated_text, &texts).ok()?;
    Some(Unit {
        id: generated.display().to_string(),
        generated_text,
        source_map,
        sources,
    })
}

/// A path for output: relative to the current directory when inside it
/// (what a terminal can click), absolute otherwise.
fn display_path(path: &Path) -> String {
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok());
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match cwd.as_deref().and_then(|cwd| real.strip_prefix(cwd).ok()) {
        Some(relative) => relative.display().to_string(),
        None => real.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_dot_is_resolved_lexically() {
        assert_eq!(
            lexically_normalized(Path::new("app/src/.generated/../state.rs")),
            PathBuf::from("app/src/state.rs")
        );
        assert!(!is_generated(&lexically_normalized(Path::new(
            "src/.generated/../state.rs"
        ))));
        assert!(is_generated(Path::new("src/.generated/a.rs")));
    }

    #[test]
    fn a_missing_generated_file_is_generated_without_a_map() {
        let mut resolver = DiskResolver::new();
        let resolved = resolver.resolve(
            "src/.generated/nope.rs",
            Path::new("/definitely/not/here/Cargo.toml"),
        );
        assert!(matches!(resolved, Resolved::GeneratedWithoutMap));
        let plain = resolver.resolve("src/other.rs", Path::new("/definitely/not/here/Cargo.toml"));
        assert!(matches!(plain, Resolved::NotGenerated));
    }
}
