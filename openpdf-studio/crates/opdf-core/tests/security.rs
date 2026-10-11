mod common;

use common::*;
use opdf_core::fixtures;
use opdf_core::security::{AllowedActions, ProtectOptions};
use opdf_core::session::{DocSession, Op, OpenRequest};
use opdf_core::{load, ErrorCode};

fn open(bytes: Vec<u8>, password: Option<&str>) -> opdf_core::Result<DocSession> {
    DocSession::open(
        1,
        OpenRequest {
            bytes,
            password: password.map(str::to_owned),
            name: "t.pdf".into(),
            path: None,
            store: None,
            label: "Opened".into(),
        },
    )
}

fn protect(user: &str, owner: &str, allowed: AllowedActions) -> Op {
    Op::Protect {
        options: ProtectOptions {
            user_password: user.into(),
            owner_password: owner.into(),
            allowed,
        },
    }
}

#[test]
fn protect_then_reopen_requires_password() {
    let mut s = open(fixtures::simple(2), None).unwrap();
    s.apply(&protect("open sesame", "", AllowedActions::ALL))
        .unwrap();
    let bytes = s.bytes().to_vec();
    assert!(!contains(&bytes, b"(Page 1)"), "content must be encrypted");
    assert_eq!(
        load(&bytes, None).unwrap_err().code,
        ErrorCode::PasswordRequired
    );
    assert_eq!(
        load(&bytes, Some("nope")).unwrap_err().code,
        ErrorCode::WrongPassword
    );
    let pdf = load(&bytes, Some("open sesame")).unwrap();
    assert_eq!(labels(&pdf.doc), vec!["Page 1", "Page 2"]);
    assert!(
        pdf.permissions().owner_access,
        "single password = full access"
    );
    qpdf_check(&bytes, Some("open sesame"));
}

#[test]
fn restrictions_need_distinct_owner_password() {
    let mut s = open(fixtures::simple(1), None).unwrap();
    let restricted = AllowedActions {
        modify: false,
        assemble: false,
        ..AllowedActions::ALL
    };
    assert_eq!(
        s.apply(&protect("", "", restricted)).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        s.apply(&protect("x", "x", restricted)).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
    assert_eq!(
        s.apply(&protect("", "", AllowedActions::ALL))
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn restricted_document_blocks_edits_without_owner_password() {
    let mut s = open(fixtures::simple(3), None).unwrap();
    let restricted = AllowedActions {
        modify: false,
        assemble: false,
        ..AllowedActions::ALL
    };
    s.apply(&protect("", "boss", restricted)).unwrap();
    let bytes = s.bytes().to_vec();
    qpdf_check(&bytes, None);

    // Anyone can open it (empty user password) but not rearrange pages.
    let mut viewer = open(bytes.clone(), None).unwrap();
    let p = viewer.permissions().unwrap();
    assert!(p.encrypted && !p.owner_access && !p.assemble && !p.modify && p.print);
    let err = viewer
        .apply(&Op::Rotate {
            pages: vec![0],
            degrees: 90,
        })
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::PermissionDenied);
    assert_eq!(
        viewer.apply(&Op::Unprotect).unwrap_err().code,
        ErrorCode::PermissionDenied
    );

    // The owner password unlocks everything, and edits keep the protection.
    let mut owner = open(bytes, Some("boss")).unwrap();
    assert!(owner.permissions().unwrap().owner_access);
    owner
        .apply(&Op::Rotate {
            pages: vec![0],
            degrees: 90,
        })
        .unwrap();
    let edited = owner.bytes().to_vec();
    let reopened = open(edited.clone(), None).unwrap().permissions().unwrap();
    assert!(!reopened.assemble, "restrictions survive editing");
    qpdf_check(&edited, Some("boss"));

    owner.apply(&Op::Unprotect).unwrap();
    let plain = owner.bytes().to_vec();
    assert!(contains(&plain, b"(Page 1)"));
    assert!(!load(&plain, None).unwrap().permissions().encrypted);
}

#[test]
fn edits_keep_existing_encryption_from_other_tools() {
    // Encrypt with qpdf using three different handlers and make sure we can
    // open, edit and save each one without losing the protection.
    let src = fixtures::simple(3);
    for (args, label) in [
        (vec!["--encrypt", "user", "owner", "256", "--"], "AES-256"),
        (
            vec!["--encrypt", "user", "owner", "128", "--use-aes=y", "--"],
            "AES-128",
        ),
        (
            vec![
                "--allow-weak-crypto",
                "--encrypt",
                "user",
                "owner",
                "128",
                "--use-aes=n",
                "--",
            ],
            "RC4-128",
        ),
    ] {
        let Some(enc) = qpdf(&args, &src) else { return };
        assert_eq!(
            load(&enc, None).unwrap_err().code,
            ErrorCode::PasswordRequired,
            "{label}"
        );
        let mut s = open(enc, Some("user")).unwrap();
        assert!(
            !s.permissions().unwrap().owner_access,
            "{label}: user password is not owner"
        );
        s.apply(&Op::Delete { pages: vec![1] }).unwrap();
        let out = s.bytes().to_vec();
        assert_eq!(
            load(&out, None).unwrap_err().code,
            ErrorCode::PasswordRequired,
            "{label}: still protected"
        );
        assert_eq!(
            labels_of(&out, Some("user")),
            vec!["Page 1", "Page 3"],
            "{label}"
        );
        assert!(
            load(&out, Some("owner"))
                .unwrap()
                .permissions()
                .owner_access,
            "{label}"
        );
        qpdf_check(&out, Some("user"));
    }
}

#[test]
fn owner_password_is_recognised_for_qpdf_files() {
    let Some(enc) = qpdf(
        &["--encrypt", "", "secret", "256", "--modify=none", "--"],
        &fixtures::simple(1),
    ) else {
        return;
    };
    let as_user = load(&enc, None).unwrap();
    assert!(!as_user.permissions().owner_access);
    assert!(!as_user.permissions().modify);
    let as_owner = load(&enc, Some("secret")).unwrap();
    assert!(as_owner.permissions().owner_access);
}
