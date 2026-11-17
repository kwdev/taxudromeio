use std::collections::BTreeSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    ListFolders,
    FolderStatus,
    ListMessages,
    SearchMessages,
    ReadMessage,
    MoveMessages,
    CopyMessages,
    SetFlags,
    SaveDraft,
}

impl Operation {
    pub const ALL: [Self; 9] = [
        Self::ListFolders,
        Self::FolderStatus,
        Self::ListMessages,
        Self::SearchMessages,
        Self::ReadMessage,
        Self::MoveMessages,
        Self::CopyMessages,
        Self::SetFlags,
        Self::SaveDraft,
    ];

    pub fn tool_name(self) -> &'static str {
        match self {
            Self::ListFolders => "mail_list_folders",
            Self::FolderStatus => "mail_folder_status",
            Self::ListMessages => "mail_list_messages",
            Self::SearchMessages => "mail_search",
            Self::ReadMessage => "mail_read_message",
            Self::MoveMessages => "mail_move_messages",
            Self::CopyMessages => "mail_copy_messages",
            Self::SetFlags => "mail_set_flags",
            Self::SaveDraft => "mail_save_draft",
        }
    }

    pub fn from_tool(name: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|op| op.tool_name() == name)
            .ok_or_else(|| anyhow::anyhow!("Unknown or unsupported tool: {name}"))
    }

    pub fn is_read(self) -> bool {
        matches!(
            self,
            Self::ListFolders
                | Self::FolderStatus
                | Self::ListMessages
                | Self::SearchMessages
                | Self::ReadMessage
        )
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    ReadOnly,
    #[default]
    Triage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub profile: Profile,
    /// If present, intersect this list with the profile. Empty exposes no operations.
    pub allowed_operations: Option<BTreeSet<Operation>>,
}

impl Policy {
    pub fn allows(&self, operation: Operation) -> bool {
        let profile_allows = match self.profile {
            Profile::ReadOnly => operation.is_read(),
            Profile::Triage => true,
        };
        profile_allows
            && self
                .allowed_operations
                .as_ref()
                .is_none_or(|allowed| allowed.contains(&operation))
    }

    pub fn require(&self, operation: Operation) -> Result<()> {
        if !self.allows(operation) {
            bail!(
                "Operation {} is disabled for this configuration",
                operation.tool_name()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// anchorbook: A-policy-triage#operation-filter
    #[test]
    fn triage_supports_drafts_and_moves_but_never_sending() {
        for operation in Operation::ALL {
            assert!(Policy::default().allows(operation));
        }
        for name in [
            "mail_send",
            "mail_reply",
            "mail_forward",
            "mail_delete_messages",
        ] {
            assert!(Operation::from_tool(name).is_err());
        }
    }

    /// anchorbook: A-policy-triage#operation-filter
    #[test]
    fn allowlist_can_only_narrow_a_profile_and_empty_denies_all() {
        let policy = Policy {
            profile: Profile::ReadOnly,
            allowed_operations: Some([Operation::ReadMessage, Operation::SaveDraft].into()),
        };
        assert!(policy.allows(Operation::ReadMessage));
        assert!(!policy.allows(Operation::ListFolders));
        assert!(!policy.allows(Operation::SaveDraft));
        let empty = Policy {
            allowed_operations: Some(BTreeSet::new()),
            ..Policy::default()
        };
        assert!(
            Operation::ALL
                .into_iter()
                .all(|operation| !empty.allows(operation))
        );
    }
}
