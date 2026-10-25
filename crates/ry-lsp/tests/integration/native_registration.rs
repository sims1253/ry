use crate::harness;

use harness::{file_uri, join_session, notify_changed, spawn_session};
use ry_testkit::FixtureProject;
use serde_json::Value;

fn unbound(message: &Value, name: &str) -> bool {
    message["params"]["diagnostics"]
        .as_array()
        .is_some_and(|diagnostics| {
            diagnostics.iter().any(|diagnostic| {
                diagnostic["code"] == "RY010"
                    && diagnostic["message"]
                        .as_str()
                        .is_some_and(|text| text.contains(name))
            })
        })
}

#[tokio::test]
async fn native_registration_edits_refresh_open_document_bindings() {
    let fixture = FixtureProject::empty().unwrap();
    fixture
        .write_file("DESCRIPTION", "Package: example\nVersion: 1.0\n")
        .unwrap();
    fixture
        .write_file("NAMESPACE", "useDynLib(example, .registration=TRUE)\n")
        .unwrap();
    let source = "native_entry_xyz\nother_missing\n";
    fixture.write_file("R/main.R", source).unwrap();
    fixture.write_file("src/init.c", "").unwrap();
    let (mut session, server) =
        spawn_session(&[fixture.root()], harness::watching_capabilities(), None).await;
    let watchers = harness::answer_watcher_registration(&mut session).await;
    assert!(
        watchers
            .iter()
            .any(|watcher| watcher["globPattern"] == "**/src/*.{c,cc,cpp,cxx,h,hh,hpp}"),
        "{watchers:?}"
    );

    let uri = file_uri(&fixture.path("R/main.R"));
    let init_uri = file_uri(&fixture.path("src/init.c"));
    let mark = session.publication_mark();
    session.open(&uri, 1, source).await.unwrap();
    session
        .notification_after(mark, "textDocument/publishDiagnostics", |message| {
            unbound(message, "native_entry_xyz") && unbound(message, "other_missing")
        })
        .await
        .unwrap();
    let registration = r#"
static const R_CallMethodDef calls[] = { {"native_entry_xyz", (DL_FUNC)&implementation, 0}, {NULL, NULL, 0} };
void R_init_example(DllInfo *dll) { R_registerRoutines(dll, NULL, calls, NULL, NULL); }
"#;
    for (contents, missing) in [(registration, false), ("", true)] {
        fixture.write_file("src/init.c", contents).unwrap();
        let mark = session.publication_mark();
        notify_changed(&mut session, &init_uri).await;
        session
            .notification_after(mark, "textDocument/publishDiagnostics", |message| {
                unbound(message, "native_entry_xyz") == missing && unbound(message, "other_missing")
            })
            .await
            .unwrap();
    }
    join_session(session, server).await;
}
