//! Password protection (AES-256, PDF 2.0 security handler revision 6).

use crate::error::{Error, Result};
use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{EncryptionState, EncryptionVersion, Permissions};
use rand::Rng;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a user who opens the document with the *open* password may do.
/// The owner password always unlocks everything.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowedActions {
    pub print: bool,
    pub print_high_quality: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
    pub fill_forms: bool,
    pub assemble: bool,
}

impl AllowedActions {
    pub const ALL: AllowedActions = AllowedActions {
        print: true,
        print_high_quality: true,
        modify: true,
        copy: true,
        annotate: true,
        fill_forms: true,
        assemble: true,
    };

    fn is_unrestricted(&self) -> bool {
        self.print
            && self.print_high_quality
            && self.modify
            && self.copy
            && self.annotate
            && self.fill_forms
            && self.assemble
    }

    fn to_permissions(self) -> Permissions {
        let mut p = Permissions::COPYABLE_FOR_ACCESSIBILITY; // always granted
        let pairs = [
            (self.print, Permissions::PRINTABLE),
            (
                self.print && self.print_high_quality,
                Permissions::PRINTABLE_IN_HIGH_QUALITY,
            ),
            (self.modify, Permissions::MODIFIABLE),
            (self.copy, Permissions::COPYABLE),
            (self.annotate, Permissions::ANNOTABLE),
            (self.fill_forms, Permissions::FILLABLE),
            (self.assemble, Permissions::ASSEMBLABLE),
        ];
        for (on, flag) in pairs {
            if on {
                p |= flag;
            }
        }
        p
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectOptions {
    /// Required to open the document. Empty = anyone can open it.
    pub user_password: String,
    /// Required to change security or bypass restrictions.
    pub owner_password: String,
    pub allowed: AllowedActions,
}

pub const MAX_PASSWORD_CHARS: usize = 127;

/// Validate options and build an AES-256 security handler.
pub fn build_state(opts: &ProtectOptions) -> Result<EncryptionState> {
    let user = opts.user_password.as_str();
    let mut owner = opts.owner_password.as_str();
    if user.chars().count() > MAX_PASSWORD_CHARS || owner.chars().count() > MAX_PASSWORD_CHARS {
        return Err(Error::invalid_arg(
            "passwords are limited to 127 characters",
        ));
    }
    let restricted = !opts.allowed.is_unrestricted();
    if user.is_empty() && !restricted {
        return Err(Error::invalid_arg(
            "set an open password, restrict at least one action, or both",
        ));
    }
    if restricted && owner.is_empty() {
        return Err(Error::invalid_arg(
            "restrictions need a permissions (owner) password, otherwise anyone could lift them",
        ));
    }
    if restricted && owner == user {
        return Err(Error::invalid_arg(
            "the permissions password must differ from the open password, otherwise the restrictions do not apply",
        ));
    }
    if owner.is_empty() {
        // Only an open password was requested: whoever knows it gets full access.
        owner = user;
    }
    let mut key = [0u8; 32];
    rand::rng().fill(&mut key);
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let version = EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: owner,
        user_password: user,
        permissions: opts.allowed.to_permissions(),
    };
    EncryptionState::try_from(version)
        .map_err(|e| Error::internal(format!("encryption setup failed: {e}")))
}
