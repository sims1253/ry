//! A serialized scope the inventory cannot enumerate is reported once per
//! failure, with the same reason the CLI prints.

use crate::harness;

use harness::{ClientSession, file_uri, join_session, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::{Value, json};

fn package(sysdata: &[u8], config: &str) -> FixtureProject {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n")
        .unwrap();
    fixture.write_file("ry.toml", config).unwrap();
    fixture.write_file("R/main.R", "sysdata\n").unwrap();
    fixture.write_file("R/sysdata.rda", sysdata).unwrap();
    fixture
}

fn is_notice(message: &Value, reason: &str) -> bool {
    message["method"] == "window/logMessage"
        && message["params"]["message"]
            .as_str()
            .is_some_and(|text| text.contains(reason))
}

/// The warning notice for `reason`, which must name the serialized file.
async fn notice_after(session: &mut ClientSession, mark: u64, reason: &str) {
    let notice = session
        .notification_after(mark, "window/logMessage", |message| {
            is_notice(message, reason)
        })
        .await
        .unwrap();
    assert_eq!(notice["params"]["type"], 2);
    assert!(
        notice["params"]["message"]
            .as_str()
            .unwrap()
            .contains("R/sysdata.rda"),
        "{notice}"
    );
}

#[tokio::test]
async fn indexing_reports_byte_limit_with_the_source_path() {
    let fixture = package(b"over the configured cap", "max-serialized-bytes = 1\n");
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    notice_after(&mut session, 0, "decoded-byte limit exceeded").await;
    join_session(session, server).await;
}

#[tokio::test]
async fn repaired_serialized_scope_clears_the_notice_and_a_later_failure_reports_again() {
    const LIMIT: &str = "serialized parser resource limit exceeded";
    let nested = include_bytes!("../../../../testdata/serialized/nested-limit.rda");
    let fixture = package(nested, "max-serialized-bytes = 4096\n");
    let data_path = fixture.path("R/sysdata.rda");
    let main = file_uri(&fixture.path("R/main.R"));
    let serialized = file_uri(&data_path);
    let (mut session, server) = spawn_session(&[fixture.root()], json!({}), None).await;
    notice_after(&mut session, 0, LIMIT).await;

    let mark = session.publication_mark();
    session.open(&main, 1, "sysdata\n").await.unwrap();
    session
        .published_diagnostics_after(&main, mark)
        .await
        .unwrap();

    std::fs::write(
        &data_path,
        include_bytes!("../../../../testdata/serialized/empty.rda"),
    )
    .unwrap();
    let mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": serialized, "type": 2}]}),
        )
        .await
        .unwrap();
    // The repaired inventory binds no `sysdata`, so the name is unbound again.
    loop {
        let publish = session
            .published_diagnostics_after(&main, mark)
            .await
            .unwrap();
        let unbound = publish["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diag| {
                diag["code"] == "RY010"
                    && diag["message"]
                        .as_str()
                        .is_some_and(|text| text.contains("sysdata"))
            });
        if unbound {
            break;
        }
    }
    assert!(
        !session.routed_after(mark, |message| is_notice(message, LIMIT)),
        "repair must not repeat the notice"
    );

    std::fs::write(&data_path, nested).unwrap();
    let mark = session.publication_mark();
    session
        .notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes": [{"uri": serialized, "type": 2}]}),
        )
        .await
        .unwrap();
    notice_after(&mut session, mark, LIMIT).await;
    join_session(session, server).await;
}
