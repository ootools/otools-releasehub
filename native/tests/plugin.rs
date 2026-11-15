use otools_releasehub_native::PluginRuntime;
use serde_json::json;

#[test]
fn native_dispatch_persists_library_and_rejects_ai_and_unconfirmed_execution() {
    let root = tempfile::tempdir().unwrap();
    let runtime = PluginRuntime::new();
    let invoke = |method: &str, payload| {
        runtime.invoke(json!({ "method": method, "payload": payload, "pluginDataDir": root.path(), "pluginDir": root.path() }))
    };
    assert_eq!(
        invoke("__otools_health__", json!({})).unwrap()["id"],
        "otools-releasehub"
    );
    let exported = json!({"version":1,"projects":[{"repository":"owner/tool","channel":"stable","preference":"auto","assetPattern":null}],"settings":{"theme":"system","checkIntervalMinutes":60,"notifications":false,"autoDownload":false,"maxConcurrentDownloads":2}});
    let imported = invoke(
        "dispatch",
        json!({"action":"import_library","payload":{"contents":exported.to_string()}}),
    )
    .unwrap();
    assert_eq!(imported["imported"], 1);
    let snapshot = invoke("dispatch", json!({"action":"snapshot","payload":{}})).unwrap();
    assert_eq!(snapshot["projects"][0]["repo"], "tool");
    assert_eq!(snapshot["runtime"]["plugin"], true);
    assert!(snapshot.get("buildTasks").is_none());
    assert!(invoke(
        "dispatch",
        json!({"action":"create_build_task","payload":{}})
    )
    .is_err());
    assert!(invoke(
        "dispatch",
        json!({"action":"install_download","payload":{"id":"x"}})
    )
    .is_err());
    let export = invoke("dispatch", json!({"action":"export_library","payload":{}})).unwrap();
    assert!(!export
        .as_str()
        .unwrap()
        .contains(&root.path().to_string_lossy().to_string()));
    assert_eq!(
        invoke("poll_events", json!({})).unwrap()["events"].is_array(),
        true
    );
    runtime.shutdown().unwrap();
    let next = PluginRuntime::new();
    let saved = next.invoke(json!({"method":"dispatch","pluginDataDir":root.path(),"pluginDir":root.path(),"payload":{"action":"snapshot","payload":{}}})).unwrap();
    assert_eq!(saved["projects"].as_array().unwrap().len(), 1);
    next.shutdown().unwrap();
}

#[test]
fn native_adapter_requires_host_selected_absolute_storage() {
    let runtime = PluginRuntime::new();
    assert!(runtime
        .invoke(json!({"method":"dispatch","payload":{"action":"snapshot","payload":{}}}))
        .is_err());
    assert!(runtime.invoke(json!({"method":"dispatch","pluginDataDir":"../../escape","payload":{"action":"snapshot","payload":{}}})).is_err());
    assert!(runtime
        .invoke(json!({"method":"shell","payload":{"command":"echo unsafe"}}))
        .is_err());
}

#[test]
fn cancelling_native_runtime_preserves_background_download_restart_state() {
    let root = tempfile::tempdir().unwrap();
    let runtime = PluginRuntime::new();
    let value=runtime.invoke(json!({"method":"dispatch","pluginDataDir":root.path(),"pluginDir":root.path(),"payload":{"action":"snapshot","payload":{}}})).unwrap();
    assert_eq!(value["settings"]["autoDownload"], false);
    assert!(value["settings"].get("agentEnabled").is_none());
    runtime.shutdown().unwrap();
    assert!(root.path().join("releasehub.sqlite3").is_file());
}
