//! File authorization is an explicit local allowlist, independent of what an AI client requests.

use fta_core::{Error, Result};
use std::path::{Component, Path, PathBuf};

/// Store physical roots once; a tool call cannot expand the directories configured at startup.
#[derive(Clone, Debug)]
pub struct FilePolicy {
    roots: Vec<PathBuf>,
}

impl FilePolicy {
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Result<Self> {
        let mut allowed = Vec::new();
        for root in roots {
            let root = absolute(root)?;
            reject_links(&root)?;
            let root = std::fs::canonicalize(root)?;
            if !root.is_dir() {
                return Err(Error::new(
                    "INVALID_REQUEST",
                    "--allow-dir must name an existing directory",
                ));
            }
            if !allowed.contains(&root) {
                allowed.push(root);
            }
        }
        Ok(Self { roots: allowed })
    }

    pub fn allowed_roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Validate every component before canonicalization so an in-tree link cannot escape authorization.
    pub fn validate_files(&self, paths: &[String]) -> Result<Vec<PathBuf>> {
        if self.roots.is_empty() {
            return Err(Error::new(
                "FILE_ACCESS_DENIED",
                "file sending is disabled; restart fta-mcp with an explicit --allow-dir",
            ));
        }
        if paths.is_empty() || paths.len() > fta_core::config::MAX_ITEMS {
            return Err(Error::new(
                "INVALID_REQUEST",
                "supply between 1 and 100 regular files",
            ));
        }
        paths
            .iter()
            .map(|path| {
                let path = Path::new(path);
                if !path.is_absolute() {
                    return Err(Error::new(
                        "INVALID_REQUEST",
                        "MCP file paths must be absolute",
                    ));
                }
                reject_links(path)?;
                let canonical = std::fs::canonicalize(path)?;
                if !self.roots.iter().any(|root| canonical.starts_with(root)) {
                    return Err(Error::new(
                        "FILE_ACCESS_DENIED",
                        "file is outside the configured MCP allow directories",
                    ));
                }
                let metadata = std::fs::symlink_metadata(&canonical)?;
                if !metadata.file_type().is_file() || is_link(&metadata) {
                    return Err(Error::new(
                        "INVALID_REQUEST",
                        "only regular files may be sent",
                    ));
                }
                Ok(canonical)
            })
            .collect()
    }
}

fn absolute(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn reject_links(path: &Path) -> Result<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        match component {
            // A Windows drive prefix is not a complete filesystem location until its root follows.
            Component::Prefix(_) => {
                prefix.push(component.as_os_str());
                continue;
            }
            Component::ParentDir => {
                return Err(Error::new(
                    "FILE_ACCESS_DENIED",
                    "parent traversal is not allowed in MCP file paths",
                ));
            }
            Component::CurDir => continue,
            _ => prefix.push(component.as_os_str()),
        }
        let metadata = std::fs::symlink_metadata(&prefix)?;
        if is_link(&metadata) {
            return Err(Error::new(
                "FILE_ACCESS_DENIED",
                "symbolic links and reparse points are not allowed in MCP paths",
            ));
        }
    }
    Ok(())
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        // Junctions and other reparse points can redirect traversal even when is_symlink is false.
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_are_explicit_and_sibling_prefixes_do_not_count_as_containment() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("allowed");
        let sibling = base.join("allowed-other");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        let file = root.join("a.txt");
        let outside = sibling.join("a.txt");
        std::fs::write(&file, "inside").unwrap();
        std::fs::write(&outside, "outside").unwrap();
        let policy = FilePolicy::new([root.clone()]).unwrap();
        assert_eq!(
            policy
                .validate_files(&[file.to_str().unwrap().into()])
                .unwrap(),
            vec![file]
        );
        assert!(
            policy
                .validate_files(&[outside.to_str().unwrap().into()])
                .is_err()
        );
        assert!(
            policy
                .validate_files(&[root.to_str().unwrap().into()])
                .is_err()
        );
        assert!(
            FilePolicy::new([])
                .unwrap()
                .validate_files(&[outside.to_str().unwrap().into()])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn final_and_ancestor_symlinks_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("allowed");
        let outside = base.join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let secret = outside.join("secret");
        std::fs::write(&secret, "secret").unwrap();
        std::os::unix::fs::symlink(&secret, root.join("file-link")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("dir-link")).unwrap();
        let policy = FilePolicy::new([root.clone()]).unwrap();
        for path in [root.join("file-link"), root.join("dir-link/secret")] {
            assert_eq!(
                policy
                    .validate_files(&[path.to_str().unwrap().into()])
                    .unwrap_err()
                    .code,
                "FILE_ACCESS_DENIED"
            );
        }
        assert!(FilePolicy::new([root.join("dir-link")]).is_err());
    }
}
