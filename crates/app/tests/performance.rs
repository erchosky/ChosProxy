//! Performance view (M31, PLATFORM-006): the counters read this process
//! and the open project, and follow what is stored.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::Engine;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};

#[tokio::test]
async fn counters_follow_the_process_and_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();

    // Without a project: the process only.
    let idle = engine.performance().await.unwrap();
    assert_eq!((idle.db_bytes, idle.stored, idle.tasks_running), (0, 0, 0));
    let process = idle.process.unwrap();
    assert!(process.memory_bytes > 1024 * 1024, "{process:?}");

    engine.new_temporary_project().await.unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    };
    let request = "GET / HTTP/1.1\nHost: lab\n\n".to_string();
    let tab = engine
        .repeater_create(NewRepeaterTab {
            name: String::new(),
            target: target.clone(),
            request: request.clone(),
            head_crlf: true,
            body_crlf: true,
        })
        .await
        .unwrap();
    for _ in 0..3 {
        engine
            .repeater_send(
                tab.id,
                RepeaterTabUpdate {
                    name: tab.name.clone(),
                    target: target.clone(),
                    request: request.clone(),
                    update_content_length: true,
                    reuse_connection: false,
                    use_cookie_jar: false,
                },
            )
            .await
            .unwrap();
    }
    let busy = engine.performance().await.unwrap();
    assert_eq!(busy.stored, 3);
    assert!(busy.db_bytes > 0);
    assert!(busy.at >= idle.at);
    assert_eq!(busy.pool_waiting, 0);
    assert!(busy.process.unwrap().cpu_percent >= 0.0);

    // A new project starts counting again.
    engine.new_temporary_project().await.unwrap();
    assert_eq!(engine.performance().await.unwrap().stored, 0);
}
