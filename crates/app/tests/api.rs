//! The API workspace end to end: import a definition, open an endpoint for
//! Repeater, and send the endpoints to the lab within the allowed hosts,
//! methods and scope.

#![allow(clippy::unwrap_used)] // test helpers: panicking is the failure report

use chosproxy_app::{ApiSendConfig, Engine};
use chosproxy_core::Source;
use chosproxy_core::paths::AppPaths;
use chosproxy_lab::LabServer;
use chosproxy_storage::{HistoryQuery, TaskRow, TaskStatus};
use std::time::Duration;

const SPEC: &str = r#"
openapi: 3.0.3
info: {title: Lab API, version: "1"}
servers: [{url: /}]
paths:
  /json:
    get: {operationId: getJson}
  /version:
    get:
      operationId: getVersion
      parameters:
        - {name: verbose, in: query, schema: {type: boolean}}
  /items:
    post:
      operationId: addItem
      requestBody:
        content:
          application/json:
            schema: {type: object, properties: {name: {type: string}}}
"#;

async fn setup() -> (tempfile::TempDir, LabServer, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let lab = LabServer::start("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let engine = Engine::new(AppPaths::under(dir.path())).unwrap();
    engine.new_temporary_project().await.unwrap();
    (dir, lab, engine)
}

async fn wait_finished(engine: &Engine, id: i64) -> (TaskRow, Vec<String>) {
    for _ in 0..800 {
        let detail = engine.task_detail(id).await.unwrap();
        if detail.row.status.is_finished() {
            return (
                detail.row,
                detail.logs.into_iter().map(|l| l.message).collect(),
            );
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("task {id} did not finish");
}

#[tokio::test]
async fn imports_definitions_and_sends_allowed_endpoints() {
    let (dir, lab, engine) = setup().await;
    let base = format!("http://{}/", lab.addr());
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());

    // A relative server needs a base URL.
    let file = dir.path().join("lab.yaml");
    std::fs::write(&file, SPEC).unwrap();
    let err = engine.api_import(&file, None).await.unwrap_err();
    assert!(err.to_string().contains("base URL"), "{err}");
    let info = engine.api_import(&file, Some(base.clone())).await.unwrap();
    assert_eq!(
        (info.name.as_str(), info.format.as_str()),
        ("lab.yaml", "openApi3")
    );
    assert_eq!(info.endpoint_count, 3);
    assert_eq!(engine.api_definitions().await.unwrap().len(), 1);

    let detail = engine.api_definition(info.id).await.unwrap();
    assert_eq!(detail.hosts, vec![host.clone()]);
    let request = engine.api_request(info.id, 1).await.unwrap();
    assert_eq!(request.target.port, lab.addr().port());
    assert!(
        request
            .request
            .starts_with("GET /version?verbose=true HTTP/1.1\nHost: ")
    );
    assert!(engine.api_request(info.id, 9).await.is_err());

    // Hosts must be ticked; safe methods only unless asked.
    let mut config = ApiSendConfig {
        definition: info.id,
        endpoints: vec![],
        hosts: vec![],
        include_unsafe: false,
        requests_per_second: 0,
    };
    assert!(engine.api_send_start(config.clone()).await.is_err());
    config.hosts = vec![host.clone()];
    let task = engine.api_send_start(config.clone()).await.unwrap();
    assert_eq!(task.kind, "apiSend");
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.status, TaskStatus::Completed, "{logs:#?}");
    assert_eq!(row.requests, 2, "{logs:#?}");
    assert!(
        logs.iter()
            .any(|l| l.contains("Skipped #2 POST") && l.contains("not a safe method"))
    );
    let page = engine
        .history_page(HistoryQuery {
            limit: 50,
            sources: vec![Source::Scanner],
            ..Default::default()
        })
        .await
        .unwrap();
    let paths: Vec<&str> = page.rows.iter().map(|r| r.path.as_str()).collect();
    assert!(
        paths.contains(&"/json") && paths.contains(&"/version?verbose=true"),
        "{paths:?}"
    );

    // The site map node lists the parameters seen and its requests.
    let node = engine
        .site_node_details(chosproxy_storage::SiteFilter {
            scheme: "http".into(),
            host: lab.addr().ip().to_string(),
            port: lab.addr().port(),
            path: "/".into(),
        })
        .await
        .unwrap();
    assert_eq!(node.total, 2);
    assert_eq!(node.requests.len(), 2);
    let verbose = node.params.iter().find(|p| p.name == "verbose").unwrap();
    assert_eq!((verbose.count, verbose.example.as_str()), (1, "true"));

    // With a scope that leaves the lab out, everything is skipped.
    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "elsewhere.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();
    config.include_unsafe = true;
    config.endpoints = vec![0, 2];
    let task = engine.api_send_start(config).await.unwrap();
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.requests, 0, "{logs:#?}");
    assert!(logs.iter().any(|l| l.contains("not in scope")), "{logs:#?}");

    engine.api_definition_delete(info.id).await.unwrap();
    assert!(engine.api_definitions().await.unwrap().is_empty());
}

#[tokio::test]
async fn refuses_files_that_are_not_definitions() {
    let (dir, _lab, engine) = setup().await;
    let file = dir.path().join("notes.json");
    std::fs::write(&file, "{\"hello\": 1}").unwrap();
    assert!(engine.api_import(&file, None).await.is_err());
    let binary = dir.path().join("x.bin");
    std::fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    assert!(engine.api_import(&binary, None).await.is_err());
    assert!(engine.api_definitions().await.unwrap().is_empty());
}

#[tokio::test]
async fn graphql_introspection_becomes_a_definition() {
    use chosproxy_storage::{NewRepeaterTab, RepeaterTabUpdate, RepeaterTarget};
    let (_dir, lab, engine) = setup().await;
    let target = RepeaterTarget {
        scheme: "http".into(),
        host: lab.addr().ip().to_string(),
        port: lab.addr().port(),
    };
    let send = |request: &'static str| {
        let engine = engine.clone();
        let target = target.clone();
        async move {
            let tab = engine
                .repeater_create(NewRepeaterTab {
                    name: String::new(),
                    target: target.clone(),
                    request: request.into(),
                    head_crlf: true,
                    body_crlf: true,
                })
                .await
                .unwrap();
            engine
                .repeater_send(
                    tab.id,
                    RepeaterTabUpdate {
                        name: tab.name.clone(),
                        target,
                        request: request.into(),
                        update_content_length: true,
                        reuse_connection: false,
                        use_cookie_jar: false,
                    },
                )
                .await
                .unwrap()
                .send
                .exchange_id
        }
    };
    // A request that is not GraphQL is refused.
    let plain = send("GET /json HTTP/1.1\nHost: lab\n\n").await;
    assert!(engine.graphql_introspect(plain).await.is_err());

    let gql = send(
        "POST /graphql HTTP/1.1\nHost: lab\nAuthorization: Bearer abc\nContent-Type: application/json\n\n\
         {\"query\":\"{ items { id } }\"}",
    )
    .await;
    let info = engine.graphql_introspect(gql).await.unwrap();
    assert_eq!(info.format, "graphql");
    assert_eq!(info.endpoint_count, 3, "item, items, rename");
    let detail = engine.api_definition(info.id).await.unwrap();
    let rename = detail
        .endpoints
        .iter()
        .find(|e| e.name == "rename")
        .unwrap();
    assert_eq!(rename.group, "Mutation");
    assert_eq!(rename.read_only, Some(false));
    assert!(
        rename
            .headers
            .contains(&("Authorization".into(), "Bearer abc".into()))
    );
    let item = detail.endpoints.iter().find(|e| e.name == "item").unwrap();
    assert!(
        item.body
            .as_deref()
            .unwrap()
            .contains("query Item($id: ID!)")
    );

    let schema = engine.graphql_schema(info.id).await.unwrap();
    assert_eq!(schema.mutation_type.as_deref(), Some("Mutation"));
    let op = engine
        .graphql_build(info.id, "query", "items")
        .await
        .unwrap();
    assert!(op.query.contains("items {"));
    let req = engine
        .graphql_request(info.id, &op.query, &op.variables)
        .await
        .unwrap();
    assert!(req.request.starts_with("POST /graphql HTTP/1.1\n"));
    assert!(req.request.contains("Authorization: Bearer abc\n"));

    // Bulk sends: queries go, the mutation stays unless asked.
    let host = format!("{}:{}", lab.addr().ip(), lab.addr().port());
    let task = engine
        .api_send_start(ApiSendConfig {
            definition: info.id,
            endpoints: vec![],
            hosts: vec![host],
            include_unsafe: false,
            requests_per_second: 0,
        })
        .await
        .unwrap();
    let (row, logs) = wait_finished(&engine, task.id).await;
    assert_eq!(row.requests, 2, "{logs:#?}");
    assert!(
        logs.iter()
            .any(|l| l.contains("POST") && l.contains("not a safe method"))
    );

    // Out of scope: no introspection is sent.
    let mut scope = engine.scope_config();
    scope.include.push(chosproxy_core::scope::ScopeRule {
        host: "elsewhere.test".into(),
        ..Default::default()
    });
    engine.save_scope_config(scope).unwrap();
    let err = engine.graphql_introspect(gql).await.unwrap_err();
    assert!(err.to_string().contains("not in scope"), "{err}");
}
