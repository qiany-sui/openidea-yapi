use openidea_plugin::{
    Plugin, ProcessOutput,
    mock::MockHost,
    views::{ViewEvent, Views},
};
use openidea_yapi::YapiPlugin;
use serde_json::{Value, json};
const JAVA: &str = r#"@RestController public class Demo { @GetMapping("/hello") public String hello(){return "";} }"#;
fn event(p: &mut YapiPlugin, h: &MockHost, v: Value) {
    p.handle("sync", ViewEvent::Message(v.to_string()), &h.invocation())
        .unwrap();
}
fn last(h: &MockHost) -> Value {
    serde_json::from_str(&h.view_messages().last().unwrap().1).unwrap()
}

#[test]
fn command_parse_failure_reaches_panel_and_replaces_stale_selection() {
    let h = MockHost::install().with_capabilities(&[
        "project.read",
        "editor.read",
        "storage",
        "process:curl",
    ]);
    h.add_file("Demo.java", JAVA);
    h.select_paths(&["/project/Demo.java"]);
    let mut p = YapiPlugin::default();
    p.run_command("preview", &h.invocation(), None).unwrap();
    h.on_process("curl", |_, stdin| {
        let data = if stdin.unwrap().contains("/api/project/get") {
            json!({"_id":1,"name":"fixture"})
        } else {
            json!([])
        };
        Ok(ProcessOutput {
            status: Some(0),
            stdout: format!("{}\nOI_YAPI_STATUS:200", json!({"errcode":0,"data":data})),
            stderr: String::new(),
        })
    });
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture","token":"fixture-token"}),
    );
    assert!(last(&h)["plan"].is_object());
    let nonce = last(&h)["nonce"].clone();
    let requests = h.process_runs().len();
    h.add_file("Callback.java", r#"@RestController class Callback {
        @PostMapping("/internal/callback") public String callback(@RequestBody Input body) { return ""; }
    } class Input { Map<String, MissingValue> result; }"#);
    h.select_paths(&["/project/Callback.java"]);
    let error = p.run_command("preview", &h.invocation(), None).unwrap_err();
    event(&mut p, &h, json!({"action":"ready"}));
    assert_eq!(last(&h)["selected"], json!(["/project/Callback.java"]));
    assert!(last(&h)["plan"].is_null());
    assert!(
        last(&h)["status"]
            .as_str()
            .unwrap()
            .contains(&error.to_string())
    );
    assert!(
        last(&h)["status"]
            .as_str()
            .unwrap()
            .contains("MissingValue")
    );
    event(
        &mut p,
        &h,
        json!({"action":"sync","nonce":nonce,"selected":[0]}),
    );
    assert!(last(&h)["status"].as_str().unwrap().contains("预览已失效"));
    assert_eq!(h.process_runs().len(), requests);
}

#[test]
fn preview_from_focused_controller_reports_parse_error_without_network() {
    let h = MockHost::install();
    let text = r#"@RestController class Callback {
        @PostMapping("/internal/callback") public String callback(@RequestBody Missing body) { return ""; }
    }"#;
    h.add_file("Callback.java", text);
    h.open_editor("/project/Callback.java", "java", text, &[]);
    let mut p = YapiPlugin::default();
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture","token":"fixture-token"}),
    );
    assert_eq!(last(&h)["selected"], json!(["/project/Callback.java"]));
    assert!(last(&h)["status"].as_str().unwrap().contains("Missing"));
    assert!(h.process_runs().is_empty());
}
#[test]
fn scanning_is_local_and_token_never_enters_storage_or_view() {
    let h = MockHost::install().with_capabilities(&[
        "project.read",
        "editor.read",
        "storage",
        "process:curl",
    ]);
    h.add_file("Demo.java", JAVA);
    h.select_paths(&["/project/Demo.java"]);
    h.on_process("curl", |_, stdin| {
        let data = if stdin.unwrap().contains("/api/project/get") {
            json!({"_id":1,"name":"fixture"})
        } else {
            json!([])
        };
        Ok(ProcessOutput {
            status: Some(0),
            stdout: format!("{}\nOI_YAPI_STATUS:200", json!({"errcode":0,"data":data})),
            stderr: String::new(),
        })
    });
    let mut p = YapiPlugin::default();
    p.run_command("preview", &h.invocation(), None).unwrap();
    assert!(h.process_runs().is_empty());
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture","token":"sensitive-fixture"}),
    );
    assert_eq!(
        last(&h)["plan"]["items"][0]["action"],
        "新增",
        "{}",
        last(&h)["status"]
    );
    assert!(!format!("{:?}", h.storage()).contains("sensitive-fixture"));
    assert!(!format!("{:?}", h.view_messages()).contains("sensitive-fixture"));
    let count = h.process_runs().len();
    event(
        &mut p,
        &h,
        json!({"action":"sync","nonce":last(&h)["nonce"],"selected":[0]}),
    );
    assert_eq!(h.process_runs().len(), count); // 默认确认框取消，不执行网络写入。
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture"}),
    );
    assert!(last(&h)["plan"].is_object());
    let count = h.process_runs().len();
    drop(p);
    let mut restarted = YapiPlugin::default();
    event(&mut restarted, &h, json!({"action":"ready"}));
    assert_eq!(last(&h)["base"], "http://fixture");
    assert_eq!(last(&h)["hasToken"], false);
    event(
        &mut restarted,
        &h,
        json!({"action":"preview","base":"http://fixture"}),
    );
    assert!(last(&h)["plan"].is_null());
    assert_eq!(h.process_runs().len(), count);
}
#[test]
fn switching_project_does_not_reuse_configuration_or_preview() {
    let h = MockHost::install();
    h.add_file("Demo.java", JAVA);
    h.select_paths(&["/project/Demo.java"]);
    let mut p = YapiPlugin::default();
    p.run_command("preview", &h.invocation(), None).unwrap();
    h.set_project_root(Some("/other"));
    event(&mut p, &h, json!({"action":"ready"}));
    assert_eq!(last(&h)["base"], "");
    assert!(last(&h)["plan"].is_null());
}

#[test]
fn clear_session_token_invalidates_preview_without_external_tools() {
    let h = MockHost::install().with_capabilities(&[
        "project.read",
        "editor.read",
        "storage",
        "process:curl",
    ]);
    h.add_file("Demo.java", JAVA);
    h.select_paths(&["/project/Demo.java"]);
    h.on_process("curl", |_, stdin| {
        let data = if stdin.unwrap().contains("/api/project/get") {
            json!({"_id":1,"name":"fixture"})
        } else {
            json!([])
        };
        Ok(ProcessOutput {
            status: Some(0),
            stdout: format!("{}\nOI_YAPI_STATUS:200", json!({"errcode":0,"data":data})),
            stderr: String::new(),
        })
    });
    let mut p = YapiPlugin::default();
    p.run_command("preview", &h.invocation(), None).unwrap();
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture","token":"session-secret"}),
    );
    assert!(last(&h)["plan"].is_object());
    let count = h.process_runs().len();
    event(&mut p, &h, json!({"action":"forget"}));
    assert_eq!(last(&h)["hasToken"], false);
    assert!(last(&h)["plan"].is_null());
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture"}),
    );
    assert!(last(&h)["plan"].is_null());
    assert_eq!(h.process_runs().len(), count);
    assert!(!format!("{:?}", h.storage()).contains("session-secret"));
    assert!(!format!("{:?}", h.view_messages()).contains("session-secret"));
}

#[test]
fn recreated_instance_ignores_legacy_remember_configuration() {
    use openidea_plugin::host;
    use sha2::{Digest, Sha256};
    let h = MockHost::install().with_capabilities(&["storage", "project.read"]);
    let key = format!("target.{:x}", Sha256::digest(b"/project\0config"));
    host::storage::set(&key, r#"{"base":"http://fixture","remember":true}"#).unwrap();
    let mut p = YapiPlugin::default();
    event(&mut p, &h, json!({"action":"ready"}));
    assert_eq!(last(&h)["base"], "http://fixture");
    assert_eq!(last(&h)["hasToken"], false);
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture"}),
    );
    assert!(last(&h)["plan"].is_null());
    assert!(h.process_runs().is_empty());
    assert_eq!(
        host::storage::get(&key).unwrap().unwrap(),
        r#"{"base":"http://fixture","remember":true}"#
    );
}

#[test]
fn dependency_failure_keeps_context_and_never_requests_yapi() {
    let h = MockHost::install().with_capabilities(&[
        "project.read",
        "editor.read",
        "storage",
        "process:curl",
    ]);
    h.add_file("pom.xml", "<project/>");
    h.add_file("C.java", r#"import external.Reply; @RestController class C { @GetMapping("/external") public Reply<String> get() { return null; } }"#);
    h.select_paths(&["/project/C.java"]);
    let mut p = YapiPlugin::default();
    assert!(p.run_command("preview", &h.invocation(), None).is_err());
    event(
        &mut p,
        &h,
        json!({"action":"preview","base":"http://fixture","token":"secret","maven":{"settings":"/custom/settings.xml","repository":"/custom/repo"}}),
    );
    let status = last(&h)["status"].as_str().unwrap().to_string();
    assert!(status.contains("process:mvn"), "{status}");
    assert!(status.contains("Reply"), "{status}");
    assert_eq!(last(&h)["maven"]["settings"], "/custom/settings.xml");
    assert!(last(&h)["plan"].is_null());
    assert!(!h.process_runs().iter().any(|r| r.program == "curl"));
}
