//! Vault-level storage: directory loader and cross-file block-id index.
//! File watcher and backlink index arrive in later milestones.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use closure_core::{BlockId, Document};
use thiserror::Error;

/// A loaded vault: every `*.org` file under a directory parsed into
/// [`Document`]s with a shared block-id index.
#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
    documents: HashMap<PathBuf, Document>,
    by_id: HashMap<BlockId, PathBuf>,
}

/// Errors while operating on a vault.
#[derive(Debug, Error)]
pub enum VaultError {
    /// Filesystem error.
    #[error("io: {0}")]
    Io(#[from] io::Error),
    /// A document failed to parse.
    #[error("parse error in {path}")]
    Parse {
        /// The file that failed.
        path: PathBuf,
    },
}

impl Vault {
    /// Open the vault at `root`, loading every `*.org` file underneath.
    pub fn open(root: &Path) -> Result<Self, VaultError> {
        let root = root.to_path_buf();
        let mut documents: HashMap<PathBuf, Document> = HashMap::new();
        let mut by_id: HashMap<BlockId, PathBuf> = HashMap::new();
        walk_org(&root, &mut |path| {
            let src = fs::read_to_string(path)?;
            let doc =
                Document::load_str(&src).map_err(|_| VaultError::Parse { path: path.into() })?;
            for id in doc.all_block_ids() {
                by_id.insert(id, path.to_path_buf());
            }
            documents.insert(path.to_path_buf(), doc);
            Ok(())
        })?;
        Ok(Self {
            root,
            documents,
            by_id,
        })
    }

    /// Root directory of the vault.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Number of `*.org` files loaded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    /// Whether the vault has zero documents.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// All loaded file paths in sorted order.
    #[must_use]
    pub fn paths(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.documents.keys().cloned().collect();
        v.sort();
        v
    }

    /// Iterate the stored file paths as borrowed references (no order).
    pub fn paths_iter(&self) -> impl Iterator<Item = &Path> {
        self.documents.keys().map(PathBuf::as_path)
    }

    /// Iterate `(path, document)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &Document)> {
        self.documents.iter().map(|(p, d)| (p.as_path(), d))
    }

    /// Lookup a document by its full filesystem path.
    #[must_use]
    pub fn document(&self, path: &Path) -> Option<&Document> {
        self.documents.get(path)
    }

    /// Lookup a headline and its owning file by block id.
    #[must_use]
    pub fn find_by_id(&self, id: &BlockId) -> Option<(&closure_core::DocHeadline, &Path)> {
        let path = self.by_id.get(id)?;
        let doc = self.documents.get(path)?;
        let h = doc.headline_by_id(id)?;
        Some((h, path.as_path()))
    }

    /// Atomically write `source` to `path`. Writes to a sibling
    /// `<name>.tmp` first then renames.
    pub fn save(&self, path: &Path, source: &str) -> Result<(), VaultError> {
        let tmp = path.with_extension("org.tmp");
        fs::write(&tmp, source)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn walk_org<F>(dir: &Path, f: &mut F) -> io::Result<()>
where
    F: FnMut(&Path) -> Result<(), VaultError>,
{
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ty = entry.file_type()?;
        if ty.is_dir() {
            walk_org(&path, f)?;
        } else if ty.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("org")
            && let Err(e) = f(&path)
        {
            return Err(io::Error::other(e.to_string()));
        }
    }
    Ok(())
}
