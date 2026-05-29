//! Integration tests for the Unix-socket IPC layer: a real server bound to a
//! temp socket, exercised by the real client. This covers the socket path that
//! the CLI uses when the MCP server is running.

use std::sync::Arc;
use std::time::Duration;

use memori_core::Memory;

use super::client;
use super::proto::{Request, Response, socket_path};
use super::server;

// ---- proto round-trips (pure, no I/O) ----

#[test]
fn request_round_trip_all_variants() {
    let cases = vec![
        Request::List {
            limit: 10,
            cursor: Some("c".into()),
            tags: vec!["a".into()],
            source: None,
        },
        Request::Get {
            id_prefix: "deadbeef".into(),
        },
        Request::Store {
            content: "hi".into(),
            tags: vec![],
            source: Some("test".into()),
        },
        Request::Forget {
            id: Some("11111111-1111-1111-1111-111111111111".into()),
            tags: vec![],
            source: None,
            older_than: None,
        },
    ];
    for req in cases {
        let json = serde_json::to_string(&req).unwrap();
        let back: Request = serde_json::from_str(&json).unwrap();
        // Re-serialize and compare strings — Request has no PartialEq.
        assert_eq!(json, serde_json::to_string(&back).unwrap());
        // "cmd" discriminator must be present and snake_case.
        assert!(json.contains("\"cmd\""), "missing cmd tag: {json}");
    }
}

#[test]
fn response_helpers_serialize_with_status_tag() {
    let ok = Response::ok(serde_json::json!({ "x": 1 }));
    let ok_json = serde_json::to_string(&ok).unwrap();
    assert!(ok_json.contains("\"status\":\"ok\""), "{ok_json}");

    let err = Response::err("boom");
    let err_json = serde_json::to_string(&err).unwrap();
    assert!(err_json.contains("\"status\":\"err\""), "{err_json}");
    assert!(err_json.contains("boom"));
}

// ---- client fallback behavior ----

#[tokio::test]
async fn send_to_missing_socket_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("nope.sock");
    let req = Request::List {
        limit: 5,
        cursor: None,
        tags: vec![],
        source: None,
    };
    let res = client::send_to(&req, &sock).await.unwrap();
    assert!(res.is_none(), "absent socket must yield None (fallback)");
}

#[tokio::test]
async fn send_to_stale_socket_file_returns_none() {
    // A regular file at the socket path simulates a server that crashed without
    // cleaning up. connect() fails; the client must treat this as "not running".
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("stale.sock");
    std::fs::write(&sock, b"not a socket").unwrap();
    let req = Request::List {
        limit: 5,
        cursor: None,
        tags: vec![],
        source: None,
    };
    let res = client::send_to(&req, &sock).await.unwrap();
    assert!(
        res.is_none(),
        "stale socket file must yield None, not an error"
    );
}

// ---- full client <-> server integration ----

async fn wait_for_socket(sock: &std::path::Path) {
    for _ in 0..200 {
        if sock.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("socket never appeared at {}", sock.display());
}

#[tokio::test]
async fn store_list_get_forget_over_socket() {
    let data_dir = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::open(data_dir.path()).await.unwrap());
    let sock = socket_path(data_dir.path());

    let srv_mem = Arc::clone(&memory);
    let srv_sock = sock.clone();
    let handle = tokio::spawn(async move {
        let _ = server::serve_on(srv_mem, &srv_sock).await;
    });
    wait_for_socket(&sock).await;

    // store
    let store_req = Request::Store {
        content: "the sky is blue".into(),
        tags: vec!["fact".into()],
        source: Some("itest".into()),
    };
    let stored = client::send_to(&store_req, &sock).await.unwrap().unwrap();
    let data = match stored {
        Response::Ok { data } => data,
        Response::Err { message } => panic!("store failed: {message}"),
    };
    let id = data["id"].as_str().unwrap().to_string();
    assert_eq!(id.len(), 36, "expected a UUID");

    // list — should contain our record
    let list_req = Request::List {
        limit: 10,
        cursor: None,
        tags: vec![],
        source: None,
    };
    let listed = client::send_to(&list_req, &sock).await.unwrap().unwrap();
    let items = match listed {
        Response::Ok { data } => data["items"].as_array().unwrap().clone(),
        Response::Err { message } => panic!("list failed: {message}"),
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["content"], "the sky is blue");
    assert_eq!(items[0]["source"], "itest");

    // get by prefix
    let prefix = &id[..8];
    let get_req = Request::Get {
        id_prefix: prefix.into(),
    };
    let got = client::send_to(&get_req, &sock).await.unwrap().unwrap();
    match got {
        Response::Ok { data } => assert_eq!(data["id"].as_str().unwrap(), id),
        Response::Err { message } => panic!("get failed: {message}"),
    }

    // forget by id
    let forget_req = Request::Forget {
        id: Some(id),
        tags: vec![],
        source: None,
        older_than: None,
    };
    let forgot = client::send_to(&forget_req, &sock).await.unwrap().unwrap();
    match forgot {
        Response::Ok { data } => assert_eq!(data["deleted_count"].as_u64().unwrap(), 1),
        Response::Err { message } => panic!("forget failed: {message}"),
    }

    // list — now empty
    let listed2 = client::send_to(&list_req, &sock).await.unwrap().unwrap();
    if let Response::Ok { data } = listed2 {
        assert_eq!(data["items"].as_array().unwrap().len(), 0);
    }

    handle.abort();
}

#[tokio::test]
async fn recall_and_update_over_socket() {
    let data_dir = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::open(data_dir.path()).await.unwrap());
    let sock = socket_path(data_dir.path());

    let srv_mem = Arc::clone(&memory);
    let srv_sock = sock.clone();
    let handle = tokio::spawn(async move {
        let _ = server::serve_on(srv_mem, &srv_sock).await;
    });
    wait_for_socket(&sock).await;

    // seed a record
    let store_req = Request::Store {
        content: "rust is a systems programming language".into(),
        tags: vec!["lang".into()],
        source: Some("seed".into()),
    };
    let id = match client::send_to(&store_req, &sock).await.unwrap().unwrap() {
        Response::Ok { data } => data["id"].as_str().unwrap().to_string(),
        Response::Err { message } => panic!("store: {message}"),
    };

    // recall finds it
    let recall_req = Request::Recall {
        query: "systems programming".into(),
        top_k: 5,
        tags: vec![],
    };
    match client::send_to(&recall_req, &sock).await.unwrap().unwrap() {
        Response::Ok { data } => {
            let results = data["results"].as_array().unwrap();
            assert!(!results.is_empty(), "recall returned nothing");
            assert_eq!(results[0]["id"].as_str().unwrap(), id);
            assert!(results[0]["score"].as_f64().unwrap() > 0.0);
        }
        Response::Err { message } => panic!("recall: {message}"),
    }

    // update content in place
    let update_req = Request::Update {
        id: id.clone(),
        content: "python is interpreted".into(),
    };
    match client::send_to(&update_req, &sock).await.unwrap().unwrap() {
        Response::Ok { data } => assert_eq!(data["id"].as_str().unwrap(), id),
        Response::Err { message } => panic!("update: {message}"),
    }

    // get reflects the new content, same id
    let get_req = Request::Get {
        id_prefix: id[..8].into(),
    };
    match client::send_to(&get_req, &sock).await.unwrap().unwrap() {
        Response::Ok { data } => {
            assert_eq!(data["id"].as_str().unwrap(), id);
            assert_eq!(data["content"].as_str().unwrap(), "python is interpreted");
        }
        Response::Err { message } => panic!("get: {message}"),
    }

    handle.abort();
}

#[tokio::test]
async fn update_invalid_uuid_returns_err() {
    let data_dir = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::open(data_dir.path()).await.unwrap());
    let sock = socket_path(data_dir.path());

    let srv_mem = Arc::clone(&memory);
    let srv_sock = sock.clone();
    let handle = tokio::spawn(async move {
        let _ = server::serve_on(srv_mem, &srv_sock).await;
    });
    wait_for_socket(&sock).await;

    let req = Request::Update {
        id: "not-a-uuid".into(),
        content: "x".into(),
    };
    match client::send_to(&req, &sock).await.unwrap().unwrap() {
        Response::Err { message } => assert!(message.contains("UUID")),
        Response::Ok { .. } => panic!("expected err for bad uuid"),
    }

    handle.abort();
}

#[tokio::test]
async fn get_unknown_prefix_returns_err_response() {
    let data_dir = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::open(data_dir.path()).await.unwrap());
    let sock = socket_path(data_dir.path());

    let srv_mem = Arc::clone(&memory);
    let srv_sock = sock.clone();
    let handle = tokio::spawn(async move {
        let _ = server::serve_on(srv_mem, &srv_sock).await;
    });
    wait_for_socket(&sock).await;

    let req = Request::Get {
        id_prefix: "ffffffff".into(),
    };
    let resp = client::send_to(&req, &sock).await.unwrap().unwrap();
    match resp {
        Response::Err { message } => assert!(message.contains("no memory")),
        Response::Ok { .. } => panic!("expected Err for unknown prefix"),
    }

    handle.abort();
}

#[tokio::test]
async fn malformed_request_yields_parse_error() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let data_dir = tempfile::tempdir().unwrap();
    let memory = Arc::new(Memory::open(data_dir.path()).await.unwrap());
    let sock = socket_path(data_dir.path());

    let srv_mem = Arc::clone(&memory);
    let srv_sock = sock.clone();
    let handle = tokio::spawn(async move {
        let _ = server::serve_on(srv_mem, &srv_sock).await;
    });
    wait_for_socket(&sock).await;

    let stream = UnixStream::connect(&sock).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    writer.write_all(b"{ not valid json }\n").await.unwrap();
    writer.shutdown().await.unwrap();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    let resp: Response = serde_json::from_str(line.trim()).unwrap();
    match resp {
        Response::Err { message } => assert!(message.contains("parse error")),
        Response::Ok { .. } => panic!("expected parse error"),
    }

    handle.abort();
}
