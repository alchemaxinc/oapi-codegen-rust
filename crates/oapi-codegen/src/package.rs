//! The generated output as a set of files, and the file operations over it.
//!
//! A run that emits operations produces a small module tree instead of one
//! file: a root file the consumer mounts, and a companion directory beside it
//! holding one module per concern. The root file keeps its path, so a consumer
//! that already mounts it needs no change, and the root re-exports every child,
//! so every generated name stays where it was.

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::emit::GENERATED_MARKER;
use crate::emit::HEADER;
use crate::error::Error;
use crate::error::Result;

/// One file of a [`GeneratedPackage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    /// Where the file goes, relative to the directory holding the root file.
    path: PathBuf,
    /// The complete source, header included.
    source: String,
}

impl GeneratedFile {
    /// Build a file from its relative path and its complete source.
    pub fn new(path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        return Self {
            path: path.into(),
            source: source.into(),
        };
    }

    /// Where the file goes, relative to the directory holding the root file.
    pub fn path(&self) -> &Path {
        return &self.path;
    }

    /// The complete source, header included.
    pub fn source(&self) -> &str {
        return &self.source;
    }

    /// The source with the generated-file header removed.
    fn body(&self) -> &str {
        return self.source.strip_prefix(HEADER).unwrap_or(&self.source);
    }
}

/// Everything one generator run produces.
///
/// A models-only run has no children and behaves exactly like the single file
/// it has always written. A run with operations adds the children, and the root
/// becomes a facade of `#[path]` module declarations and re-exports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedPackage {
    /// The source of the file at the configured output path.
    root: String,
    /// The companion module files, relative to the root file's directory.
    children: Vec<GeneratedFile>,
}

impl GeneratedPackage {
    /// Build a package from the root source and its companion files.
    pub fn new(root: impl Into<String>, children: Vec<GeneratedFile>) -> Self {
        return Self {
            root: root.into(),
            children,
        };
    }

    /// The source of the file at the configured output path.
    pub fn root_source(&self) -> &str {
        return &self.root;
    }

    /// The companion module files, relative to the root file's directory.
    pub fn children(&self) -> &[GeneratedFile] {
        return &self.children;
    }

    /// How many files a write produces.
    pub fn file_count(&self) -> usize {
        return self.children.len().saturating_add(1);
    }

    /// Every file's body behind one header, for a scan that has to see all of
    /// the generated code at once.
    ///
    /// Dependency detection and the empty-output check both read the code as
    /// text. Splitting the output across files must not change what either one
    /// concludes, so both read this instead of any single file.
    pub fn combined_source(&self) -> String {
        let mut combined = String::from(HEADER);
        combined.push_str(self.root.strip_prefix(HEADER).unwrap_or(&self.root));
        for child in &self.children {
            combined.push_str(child.body());
        }
        return combined;
    }
}

/// What a comparison of a generated package against the files on disk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageDrift {
    /// Every file on disk holds the generated code, and no other file does.
    None,
    /// A file generation would write does not exist.
    Absent(PathBuf),
    /// A file exists and holds different content.
    Differs(PathBuf),
    /// A file an earlier run wrote is still there, and this run does not
    /// produce it.
    Stale(PathBuf),
}

/// The companion directory beside `output_path`, named after its file stem.
///
/// The root file declares each child with an explicit `#[path]` relative to its
/// own directory, so this is where those declarations point.
///
/// Gives `None` when the path has no file stem. Use [`companion_of`] where the
/// directory has to be usable: an extensionless path yields the output file
/// itself, which cannot hold the children.
fn companion_directory(output_path: &Path) -> Option<PathBuf> {
    let stem = output_path.file_stem()?;
    let parent = output_path.parent().unwrap_or_else(|| return Path::new(""));
    return Some(parent.join(stem));
}

/// Write every file of `package`, then remove the files an earlier run left in
/// the companion directory that this run does not produce.
///
/// The companion directory is audited before anything is written, so a run that
/// refuses leaves every file it would have replaced intact.
///
/// # Errors
///
/// Returns [`Error::WriteOutput`] when a file cannot be written, and
/// [`Error::UnownedOutput`] when the companion directory holds a file the
/// generator did not write. Refusing is deliberate: the directory belongs to
/// the generator, and deleting a hand-written file that landed in it would lose
/// work.
pub fn write_package(output_path: &Path, package: &GeneratedPackage) -> Result<()> {
    let stale = audit(output_path, package)?;

    crate::write_output(output_path, package.root_source())?;
    let parent = output_path.parent().unwrap_or_else(|| return Path::new(""));
    for child in package.children() {
        crate::write_output(&parent.join(child.path()), child.source())?;
    }
    for path in stale {
        std::fs::remove_file(&path).map_err(|source| {
            return Error::WriteOutput {
                path: path.display().to_string(),
                source,
            };
        })?;
    }
    if let Some(directory) = companion_directory(output_path) {
        remove_empty_directories(&directory);
    }
    return Ok(());
}

/// Compare `package` with the files on disk and report the first difference.
///
/// # Errors
///
/// Returns [`Error::ReadOutput`] when a file exists and cannot be read, and
/// [`Error::UnownedOutput`] when the companion directory holds a file the
/// generator did not write.
pub fn check_package(output_path: &Path, package: &GeneratedPackage) -> Result<PackageDrift> {
    // The audit comes first so that a file the generator does not own is
    // reported as such, rather than as drift against what would replace it.
    let stale = audit(output_path, package)?;
    // The root comes next, because it is the file a consumer mounts and the
    // one a reader looks at first.
    if let Some(drift) = compare(output_path, package.root_source())? {
        return Ok(drift);
    }
    let parent = output_path.parent().unwrap_or_else(|| return Path::new(""));
    for child in package.children() {
        if let Some(drift) = compare(&parent.join(child.path()), child.source())? {
            return Ok(drift);
        }
    }
    if let Some(path) = stale.into_iter().next() {
        return Ok(PackageDrift::Stale(path));
    }
    return Ok(PackageDrift::None);
}

/// Compare one file, giving `None` when it already holds `source`.
fn compare(path: &Path, source: &str) -> Result<Option<PackageDrift>> {
    return match crate::check_output(path, source)? {
        crate::Drift::None => Ok(None),
        crate::Drift::Absent => Ok(Some(PackageDrift::Absent(path.to_path_buf()))),
        crate::Drift::Differs => Ok(Some(PackageDrift::Differs(path.to_path_buf()))),
    };
}

/// The companion directory a package with children needs beside `output_path`.
///
/// # Errors
///
/// Returns [`Error::UnsplittableOutput`] when the directory would be the output
/// file itself, which is what an output path with no extension asks for.
pub(crate) fn companion_of(output_path: &Path) -> Result<PathBuf> {
    return companion_directory(output_path)
        .filter(|directory| return directory != output_path)
        .ok_or_else(|| {
            return Error::UnsplittableOutput {
                path: output_path.display().to_string(),
            };
        });
}

/// Check every existing file in the companion directory and return the
/// generated ones `package` does not produce, sorted so a report names the same
/// file on every run.
///
/// Auditing the whole directory up front is what lets a write be safe. Every
/// file the generator would replace or delete is inspected first, so a run that
/// meets somebody else's work stops before touching anything.
fn audit(output_path: &Path, package: &GeneratedPackage) -> Result<Vec<PathBuf>> {
    // A run without children still has to clear the companion directory an
    // earlier run left behind, so only a path that cannot have one is skipped.
    let directory = match companion_of(output_path) {
        Ok(directory) => directory,
        Err(_) if package.children().is_empty() => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let parent = output_path.parent().unwrap_or_else(|| return Path::new(""));
    for child in package.children() {
        let path = child.path();
        // Every step below writes or deletes under the companion directory and
        // trusts that each child lands there. The emitter always builds such a
        // path, but the package types are public and can be built by hand.
        let contained = path
            .components()
            .all(|component| return matches!(component, Component::Normal(_)))
            && parent.join(path).starts_with(&directory);
        if !contained {
            return Err(Error::OutsideOutput {
                path: path.display().to_string(),
                directory: directory.display().to_string(),
            });
        }
    }
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() => {}
        // A companion directory that is a file or a link is not the generator's
        // work, and the run must not write children through it.
        Ok(_) => {
            return Err(Error::UnownedOutput {
                path: directory.display().to_string(),
            });
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(source) => {
            return Err(Error::ReadOutput {
                path: directory.display().to_string(),
                source,
            });
        }
    }
    let expected: Vec<PathBuf> = package
        .children()
        .iter()
        .map(|child| return parent.join(child.path()))
        .collect();
    let mut stale = Vec::new();
    collect_stale(&directory, &expected, &mut stale)?;
    stale.sort();
    return Ok(stale);
}

/// Walk `directory`, rejecting every file the generator does not own and adding
/// the generated ones `expected` does not list.
fn collect_stale(directory: &Path, expected: &[PathBuf], stale: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(directory).map_err(|source| {
        return Error::ReadOutput {
            path: directory.display().to_string(),
            source,
        };
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| {
            return Error::ReadOutput {
                path: directory.display().to_string(),
                source,
            };
        })?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|source| {
            return Error::ReadOutput {
                path: path.display().to_string(),
                source,
            };
        })?;
        // The generator writes no link. Following one would take the walk out of
        // the directory this run owns, so a link is somebody's work whatever it
        // points at.
        if kind.is_symlink() {
            return Err(Error::UnownedOutput {
                path: path.display().to_string(),
            });
        }
        if kind.is_dir() {
            collect_stale(&path, expected, stale)?;
            continue;
        }
        // Reading a device or a pipe can block for ever, and the generator
        // writes neither, so anything but a regular file is somebody else's.
        //
        // Ownership is settled before the expected set is consulted, so a
        // hand-written file sitting where a generated one belongs stops the run
        // instead of being overwritten.
        if !kind.is_file() || !is_generated(&path)? {
            return Err(Error::UnownedOutput {
                path: path.display().to_string(),
            });
        }
        if expected.iter().any(|candidate| return *candidate == path) {
            continue;
        }
        stale.push(path);
    }
    return Ok(());
}

/// Whether `path` carries the marker every generated file opens with.
///
/// Only the marker is read. The file may be anything at all, and a run must not
/// have to hold it in memory to decide it is not the generator's.
fn is_generated(path: &Path) -> Result<bool> {
    let read = |source| {
        return Error::ReadOutput {
            path: path.display().to_string(),
            source,
        };
    };
    let marker = GENERATED_MARKER.as_bytes();
    let mut file = std::fs::File::open(path).map_err(read)?;
    let mut opening = vec![0_u8; marker.len()];
    return match std::io::Read::read_exact(&mut file, &mut opening) {
        Ok(()) => Ok(opening == marker),
        // A file shorter than the marker cannot carry it.
        Err(source) if source.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(source) => Err(read(source)),
    };
}

/// Remove `directory` and every directory under it that holds nothing.
///
/// A removal that fails leaves an empty directory behind, which costs nothing
/// and breaks no later run, so this reports no error.
fn remove_empty_directories(directory: &Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        // `file_type` does not follow links, so the walk stays inside the
        // directory this run owns.
        if entry.file_type().map(|kind| return kind.is_dir()).unwrap_or(false) {
            remove_empty_directories(&entry.path());
        }
    }
    let _ = std::fs::remove_dir(directory);
}
