use anyhow::{Result, bail};

use crate::config::safe_text;

/// Names exposed to MCP are relative to root using the server's hierarchy delimiter.
#[derive(Clone)]
pub struct FolderScope {
    root: String,
    delimiter: Option<char>,
}

impl FolderScope {
    pub fn new(root: String, delimiter: Option<char>) -> Result<Self> {
        if !safe_text(&root) || root.contains(['*', '%']) {
            bail!("Invalid root folder");
        }
        if delimiter.is_some_and(|d| root.ends_with(d) || d.is_control()) {
            bail!("Invalid root or hierarchy delimiter");
        }
        Ok(Self { root, delimiter })
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn delimiter(&self) -> Option<char> {
        self.delimiter
    }

    pub fn resolve(&self, relative: &str) -> Result<String> {
        if relative == "." {
            return Ok(self.root.clone());
        }
        if !safe_text(relative) || relative.contains(['*', '%']) {
            bail!("Folder must be '.' or a relative descendant of the configured root");
        }
        let delimiter = self
            .delimiter
            .ok_or_else(|| anyhow::anyhow!("Root has no folder hierarchy"))?;
        if relative
            .split(delimiter)
            .any(|s| s.is_empty() || s == "." || s == "..")
        {
            bail!("Absolute, empty and traversal folder paths are not allowed");
        }
        let absolute = format!("{}{delimiter}{relative}", self.root);
        if self.relative(&absolute).is_none() {
            bail!("Folder is outside the configured root");
        }
        Ok(absolute)
    }

    pub fn relative(&self, absolute: &str) -> Option<String> {
        // IMAP only reserves case-insensitive matching for the mailbox named INBOX.
        if absolute == self.root
            || (self.root.eq_ignore_ascii_case("INBOX") && absolute.eq_ignore_ascii_case("INBOX"))
        {
            return Some(".".into());
        }
        let prefix = format!("{}{}", self.root, self.delimiter?);
        let suffix = absolute.strip_prefix(&prefix)?;
        if suffix.is_empty() || !safe_text(suffix) || suffix.contains(['*', '%']) {
            return None;
        }
        let delimiter = self.delimiter?;
        if suffix
            .split(delimiter)
            .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return None;
        }
        Some(suffix.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// anchorbook: A-folder-confined#root-boundaries
    #[test]
    fn root_scope_respects_hierarchy_boundaries_and_nil_delimiters() {
        let scope = FolderScope::new("Projects".into(), Some('/')).unwrap();
        assert_eq!(scope.resolve(".").unwrap(), "Projects");
        assert_eq!(
            scope.resolve("Work/Drafts").unwrap(),
            "Projects/Work/Drafts"
        );
        for outside in [
            "Projects-other",
            "INBOX",
            "ProjectsOther/Work",
            "Projects/",
            "Projects/../INBOX",
        ] {
            assert_eq!(scope.relative(outside), None);
        }
        for invalid in [
            "",
            "/Work",
            "..",
            "../INBOX",
            "Work/..",
            "Work//Drafts",
            "Work\nUID FETCH 1",
            "*",
        ] {
            assert!(scope.resolve(invalid).is_err(), "{invalid:?}");
        }
        let flat = FolderScope::new("INBOX".into(), None).unwrap();
        assert!(flat.resolve("Drafts").is_err());
        assert_eq!(flat.relative("inbox").as_deref(), Some("."));
    }

    /// anchorbook: A-folder-confined#root-boundaries
    #[test]
    fn dot_delimiters_do_not_make_sibling_names_accessible() {
        let scope = FolderScope::new("INBOX.Projects".into(), Some('.')).unwrap();
        assert_eq!(
            scope.resolve("Work.Drafts").unwrap(),
            "INBOX.Projects.Work.Drafts"
        );
        assert_eq!(scope.relative("INBOX.Projects2.Work"), None);
        assert!(scope.resolve("..").is_err());
    }
}
