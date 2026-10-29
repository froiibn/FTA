//! MCP path restrictions must also hold when the engine opens the file, after IPC validation.
use crate::{Error, Result};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

/// Anchor file lookup inside an allowed directory so swapping a child symlink cannot escape it.
pub fn open_file(path: &Path, allowed_roots: Option<&[PathBuf]>) -> Result<File> {
    if !path.is_absolute() {
        return Err(Error::new("INVALID_PATH", "source path must be absolute"));
    }
    let file = if let Some(roots) = allowed_roots {
        if roots.is_empty() {
            return Err(Error::new(
                "PATH_DENIED",
                "file transfer requires at least one allowed directory",
            ));
        }
        let canonical = std::fs::canonicalize(path)?;
        // MCP captures physical roots at startup. Resolving a changed root again would silently expand authority.
        for root in roots {
            if !root.is_absolute() || std::fs::canonicalize(root)? != *root {
                return Err(Error::new(
                    "PATH_DENIED",
                    "allowed directory changed or is not a physical path",
                ));
            }
        }
        let (root, relative) = roots
            .iter()
            .find_map(|root| {
                canonical
                    .strip_prefix(root)
                    .ok()
                    .map(|relative| (root, relative))
            })
            .ok_or_else(|| {
                Error::new("PATH_DENIED", "source lies outside the allowed directories")
            })?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err(Error::new("PATH_DENIED", "invalid relative source path"));
        }
        let root_meta = std::fs::symlink_metadata(root)?;
        if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
            return Err(Error::new(
                "PATH_DENIED",
                "allowed directory is no longer a real directory",
            ));
        }
        let dir = open_root_nofollow(root)?;
        dir.open(relative)
            .map_err(|_| {
                Error::new(
                    "PATH_DENIED",
                    "source could not be opened within allowed directory",
                )
            })?
            .into_std()
    } else {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(Error::new("INVALID_PATH", "source must be a regular file"));
        }
        File::open(path)?
    };
    if !file.metadata()?.is_file() {
        return Err(Error::new("INVALID_PATH", "source must be a regular file"));
    }
    Ok(file)
}
// Native mobile containers are trust anchors; sandbox policy can deny opening their system ancestors.
#[cfg(any(target_os = "ios", target_os = "android"))]
fn open_root_nofollow(root: &Path) -> Result<cap_std::fs::Dir> {
    #[cfg(target_os = "ios")]
    let (anchor, handle) = fta_platform::ios::source_anchor()?;
    #[cfg(target_os = "android")]
    let (anchor, handle) = fta_platform::android::source_anchor()?;
    open_root_from_anchor(root, &anchor, handle)
}

/// Open each physical root component relative to the previous handle, never following a replacement link.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn open_root_nofollow(root: &Path) -> Result<cap_std::fs::Dir> {
    let mut anchor = PathBuf::new();
    let mut names = Vec::new();
    for component in root.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                anchor.push(component.as_os_str())
            }
            std::path::Component::Normal(name) => names.push(name),
            _ => {
                return Err(Error::new(
                    "PATH_DENIED",
                    "allowed directory is not a physical absolute path",
                ));
            }
        }
    }
    if !anchor.is_absolute() {
        return Err(Error::new("PATH_DENIED", "missing filesystem root"));
    }
    let mut handle =
        cap_std::fs::Dir::open_ambient_dir(anchor, cap_std::ambient_authority())?.into_std_file();
    for name in names {
        handle = cap_primitives::fs::open_dir_nofollow(&handle, Path::new(name)).map_err(|_| {
            Error::new(
                "PATH_DENIED",
                "allowed directory contains a replaced link or inaccessible component",
            )
        })?;
    }
    Ok(cap_std::fs::Dir::from_std_file(handle))
}
// Only the starting handle differs on mobile. Every grant component below it retains the same link checks.
#[cfg(any(target_os = "ios", target_os = "android", all(test, unix)))]
fn open_root_from_anchor(root: &Path, anchor: &Path, mut handle: File) -> Result<cap_std::fs::Dir> {
    let relative = root.strip_prefix(anchor).map_err(|_| {
        Error::new(
            "PATH_DENIED",
            "allowed directory is outside the filesystem anchor",
        )
    })?;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(Error::new(
                "PATH_DENIED",
                "invalid relative allowed directory",
            ));
        };
        handle = cap_primitives::fs::open_dir_nofollow(&handle, Path::new(name)).map_err(|_| {
            Error::new(
                "PATH_DENIED",
                "allowed directory contains a replaced link or inaccessible component",
            )
        })?;
    }
    Ok(cap_std::fs::Dir::from_std_file(handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn pinned_sandbox_anchor_does_not_open_inaccessible_parents() {
        use std::{io::Read, os::unix::fs::PermissionsExt};
        let temp = tempfile::tempdir().unwrap();
        let parent = temp
            .path()
            .canonicalize()
            .unwrap()
            .join("system-owned-parent");
        let container = parent.join("container");
        let allowed = container.join("Library/Application Support/FTA/Outgoing");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::write(allowed.join("photo.jpg"), b"selected photo bytes").unwrap();
        let anchor = File::open(&container).unwrap();
        let original_permissions = std::fs::metadata(&parent).unwrap().permissions();
        // Search remains permitted, like iOS sandbox lookup, but reading the ancestor is denied.
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o111)).unwrap();
        let ancestor_is_inaccessible = std::fs::read_dir(&parent).is_err();
        let old_walk = open_root_nofollow(&allowed);
        let opened = open_root_from_anchor(&allowed, &container, anchor).and_then(|dir| {
            let mut data = Vec::new();
            dir.open("photo.jpg")?.read_to_end(&mut data)?;
            Ok(data)
        });
        // Restore before assertions so even a regression cannot strand an unreadable temporary tree.
        std::fs::set_permissions(&parent, original_permissions).unwrap();
        // Linux can use O_PATH search handles; Darwin is the platform whose old read-open reproduces this denial.
        if ancestor_is_inaccessible && cfg!(target_os = "macos") {
            assert!(
                old_walk.is_err(),
                "the old absolute ancestor walk must reproduce the denial"
            );
        }
        assert_eq!(opened.unwrap(), b"selected photo bytes");
    }

    #[cfg(unix)]
    #[test]
    fn pinned_sandbox_anchor_still_rejects_replaced_links_and_outside_roots() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let container = temp.path().canonicalize().unwrap();
        let other = outside.path().canonicalize().unwrap();
        std::fs::create_dir(other.join("child")).unwrap();
        std::os::unix::fs::symlink(&other, container.join("changed")).unwrap();
        for denied in [container.join("changed/child"), other] {
            assert_eq!(
                open_root_from_anchor(&denied, &container, File::open(&container).unwrap())
                    .unwrap_err()
                    .code,
                "PATH_DENIED"
            );
        }
    }

    #[test]
    fn allowed_roots_reject_escape_and_empty_grants() {
        let d = tempfile::tempdir().unwrap();
        let allowed = d.path().canonicalize().unwrap().join("allowed");
        std::fs::create_dir(&allowed).unwrap();
        let inside = allowed.join("file");
        let outside = d.path().join("outside");
        std::fs::write(&inside, b"yes").unwrap();
        std::fs::write(&outside, b"no").unwrap();
        assert!(open_file(&inside, Some(std::slice::from_ref(&allowed))).is_ok());
        assert!(open_file(&outside, Some(&[allowed])).is_err());
        assert!(open_file(&inside, Some(&[])).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn root_component_open_refuses_an_ancestor_replaced_with_a_link() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let physical = base.path().canonicalize().unwrap();
        std::fs::create_dir(outside.path().join("child")).unwrap();
        std::os::unix::fs::symlink(outside.path(), physical.join("changed")).unwrap();
        assert!(open_root_nofollow(&physical.join("changed/child")).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_to_another_root_cannot_bypass_containment() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let outside = b.path().join("private");
        std::fs::write(&outside, b"private").unwrap();
        let link = a.path().join("link");
        std::os::unix::fs::symlink(outside, &link).unwrap();
        assert!(open_file(&link, Some(&[a.path().canonicalize().unwrap()])).is_err());
    }
}
