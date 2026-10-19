//! `argus mcp` end to end: a real MCP client spawns the binary over stdio, as Claude
//! Desktop does, and drives a temp Library through the tools.

use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::ServiceExt;
use serde_json::{json, Value};
use std::path::Path;

fn write_jpeg(path: &Path, seed: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let data: Vec<u8> = (0..320 * 240 * 3)
        .map(|i| (i as u8).wrapping_mul(seed))
        .collect();
    jpeg_encoder::Encoder::new_file(path, 90)
        .unwrap()
        .encode(&data, 320, 240, jpeg_encoder::ColorType::Rgb)
        .unwrap();
}

fn args(v: Value) -> rmcp::model::JsonObject {
    match v {
        Value::Object(m) => m,
        _ => panic!("arguments must be an object"),
    }
}

fn text(r: &CallToolResult) -> Value {
    match &r.content[0] {
        ContentBlock::Text(t) => {
            serde_json::from_str(&t.text).unwrap_or(Value::String(t.text.clone()))
        }
        c => panic!("expected text first, got {c:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn claude_desktop_style_session_over_stdio() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("2026-10-12_tahoe");
    for i in 0..3 {
        write_jpeg(&src.join(format!("p{i}.jpg")), i * 2 + 3);
    }
    let library = tmp.path().join("Argus.library");

    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_argus")).configure(|c| {
            c.arg("mcp").arg("--library").arg(&library);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "argus");
    assert!(info
        .instructions
        .as_deref()
        .unwrap_or("")
        .contains("Contact sheet"));

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for want in [
        "list_shoots",
        "list_photos",
        "import_folder",
        "get_preview",
        "get_contact_sheet",
        "set_verdict",
        "list_stacks",
        "set_stacks",
        "cull_shoot",
        "read_recipe",
        "write_recipe",
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }

    let call = |name: &'static str, a: Value| {
        let client = &client;
        async move {
            client
                .call_tool(CallToolRequestParams::new(name).with_arguments(args(a)))
                .await
                .unwrap()
        }
    };

    let r = call("import_folder", json!({ "folder": src })).await;
    assert_eq!(text(&r)["imported"], 3, "{r:?}");

    let r = call("get_contact_sheet", json!({ "shoot_id": 1 })).await;
    assert_eq!(text(&r)["photos"][2]["n"], 3);
    match &r.content[1] {
        ContentBlock::Image(img) => assert_eq!(img.mime_type, "image/jpeg"),
        c => panic!("expected the sheet image, got {c:?}"),
    }

    let r = call(
        "set_verdict",
        json!({ "photo_ids": [1, 3], "verdict": "pick", "reason": "sharp" }),
    )
    .await;
    assert_eq!(text(&r)["changed"], json!([1, 3]));
    let r = call("list_photos", json!({ "verdict": "pick" })).await;
    assert_eq!(text(&r).as_array().unwrap().len(), 2);

    // A tool failure is a tool error the model can read, not a protocol error.
    let r = call("get_preview", json!({ "photo_id": 42 })).await;
    assert_eq!(r.is_error, Some(true));
    assert!(text(&r).as_str().unwrap().contains("no Photo with id 42"));

    client.cancel().await.unwrap();
}
