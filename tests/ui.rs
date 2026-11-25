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

fn large_project() -> MockHost {
    let h = MockHost::install().with_capabilities(&[
        "project.read",
        "editor.read",
        "storage",
        "process:curl",
    ]);
    for i in 0..5001 {
        h.add_file(&format!("unrelated/Unused{i}.java"), "class Unused {}");
    }
    h
}

#[test]
fn large_project_resolves_only_controller_dependencies_across_modules() {
    let h = large_project();
    h.add_file(
        "api/C.java",
        r#"package api; import dto.Input;
        @RestController class C { @PostMapping("/internal/callback")
        public Input call(@RequestBody Input body) { return body; } }"#,
    );
    h.add_file(
        "model/Input.java",
        "package dto; class Input { Detail detail; }",
    );
    h.add_file(
        "model/Detail.java",
        "package dto; class Detail { String name; }",
    );
    h.add_file(
        "other/Input.java",
        "package other; class Input { Missing wrong; }",
    );
    h.add_file("unrelated/Huge.java", &" ".repeat(1024 * 1024 + 1));
    h.add_file("api/D.java", r#"package api; import dto.Input;
        @RestController class D { @GetMapping("/internal/other") public Input get() { return null; } }"#);
    h.open_editor(
        "/project/model/Detail.java",
        "java",
        "package dto; class Detail { int count; }",
        &[],
    );
    h.select_paths(&["/project/api/C.java", "/project/api/D.java"]);
    let mut p = YapiPlugin::default();
    p.run_command("preview", &h.invocation(), None).unwrap();
    assert!(last(&h)["status"].as_str().unwrap().contains("2"));
    assert!(h.process_runs().is_empty());
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
    let plan = last(&h);
    assert_eq!(plan["plan"]["items"].as_array().unwrap().len(), 2, "{plan}");
    for item in plan["plan"]["items"].as_array().unwrap() {
        let schema: Value =
            serde_json::from_str(item["payload"]["res_body"].as_str().unwrap()).unwrap();
        assert_eq!(
            schema["properties"]["detail"]["properties"]["count"]["type"],
            "integer"
        );
        assert!(schema["properties"].get("wrong").is_none());
    }
}

#[test]
fn large_project_keeps_wildcard_import_ambiguity_and_size_guards() {
    for (imports, dto, expected) in [
        (
            "import a.*; import b.*;",
            "package a; class Input { String name; }".to_string(),
            "同名歧义",
        ),
        ("import a.Input;", " ".repeat(1024 * 1024 + 1), "1 MiB"),
    ] {
        let h = large_project();
        h.add_file("C.java", &format!("{imports} @RestController class C {{ @GetMapping(\"/x\") public Input get() {{ return null; }} }}"));
        h.add_file("a/Input.java", &dto);
        h.add_file("b/Input.java", "package b; class Input { int id; }");
        h.select_paths(&["/project/C.java"]);
        let error = YapiPlugin::default()
            .run_command("preview", &h.invocation(), None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{error}");
        assert!(h.process_runs().is_empty());
    }
}

#[test]
fn small_project_keeps_types_declared_in_non_matching_filenames() {
    let h = MockHost::install();
    h.add_file(
        "C.java",
        r#"@RestController class C { @GetMapping("/x") public Reply get(){ return null; } }"#,
    );
    h.add_file(
        "Models.java",
        "class Reply { String text; } class Other { int id; }",
    );
    h.select_paths(&["/project/C.java"]);
    YapiPlugin::default()
        .run_command("preview", &h.invocation(), None)
        .unwrap();
}

#[test]
fn large_project_rejects_duplicate_fully_qualified_dtos() {
    let h = large_project();
    h.add_file("C.java", r#"import dto.Reply; @RestController class C { @GetMapping("/x") public Reply get(){ return null; } }"#);
    for module in ["one", "two"] {
        h.add_file(
            &format!("{module}/Reply.java"),
            "package dto; class Reply { String text; }",
        );
    }
    h.select_paths(&["/project/C.java"]);
    let e = YapiPlugin::default()
        .run_command("preview", &h.invocation(), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("同名歧义"), "{e}");
}

#[test]
fn large_project_does_not_hide_duplicate_types_in_multi_class_files() {
    let h = large_project();
    h.add_file("C.java", r#"import dto.Input; @RestController class C { @GetMapping("/x") public Input get(){ return null; } }"#);
    h.add_file(
        "Input.java",
        "package dto; class Input { Detail value; } class Detail { String text; }",
    );
    h.add_file("Detail.java", "package dto; class Detail { int id; }");
    h.select_paths(&["/project/C.java"]);
    let e = YapiPlugin::default()
        .run_command("preview", &h.invocation(), None)
        .unwrap_err()
        .to_string();
    assert!(e.contains("独立文件"), "{e}");
}

#[test]
fn issue_3_nested_response_generates_complete_yapi_preview_in_large_and_small_projects() {
    for large in [false, true] {
        let h = if large {
            large_project()
        } else {
            MockHost::install().with_capabilities(&[
                "project.read",
                "editor.read",
                "storage",
                "process:curl",
            ])
        };
        h.add_file(
            "ModelAchievementController.java",
            include_str!("fixtures/nested/ModelAchievementController.java"),
        );
        h.add_file(
            "ModelAchievementOverviewResp.java",
            include_str!("fixtures/nested/ModelAchievementOverviewResp.java"),
        );
        h.select_paths(&["/project/ModelAchievementController.java"]);
        let mut p = YapiPlugin::default();
        p.run_command("preview", &h.invocation(), None).unwrap();
        assert!(h.process_runs().is_empty());
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
        let result = last(&h);
        let items = result["plan"]["items"]
            .as_array()
            .unwrap_or_else(|| panic!("{result}"));
        assert_eq!(items.len(), 1);
        let payload = &items[0]["payload"];
        assert_eq!(payload["path"], "/model/achievement/overview");
        let schema: Value = serde_json::from_str(payload["res_body"].as_str().unwrap()).unwrap();
        assert_eq!(schema["properties"].as_object().unwrap().len(), 6);
        for section in [
            "model",
            "warning",
            "judged",
            "product",
            "battle",
            "instruction",
        ] {
            let props = &schema["properties"][section]["properties"];
            assert_eq!(props.as_object().unwrap().len(), 2);
            assert_eq!(props["total"]["type"], "integer");
            assert_eq!(props["mom"]["type"], "integer");
        }
    }
}

#[test]
fn large_project_finds_direct_nested_response_in_outer_file() {
    for (imports, ty) in [
        ("import dto.Outer;", "Outer.Inner"),
        ("import dto.Outer.Inner;", "Inner"),
        ("import static dto.Outer.Inner;", "Inner"),
        ("", "dto.Outer.Inner"),
    ] {
        let h = large_project();
        h.add_file("C.java", &format!(r#"package api; {imports} @RestController class C {{ @GetMapping("/nested") public {ty} get() {{ return null; }} }}"#));
        h.add_file(
            "Outer.java",
            "package dto; class Outer { static class Inner { String value; } }",
        );
        h.add_file(
            "unrelated/dto.java",
            "package unrelated; class dto {} class Other {}",
        );
        h.select_paths(&["/project/C.java"]);
        YapiPlugin::default()
            .run_command("preview", &h.invocation(), None)
            .unwrap();
        assert!(h.process_runs().is_empty());
    }
}

#[test]
fn issue_5_json_format_reaches_yapi_preview_in_large_and_small_projects() {
    for large in [false, true] {
        let h = if large {
            large_project()
        } else {
            MockHost::install().with_capabilities(&[
                "project.read",
                "editor.read",
                "storage",
                "process:curl",
            ])
        };
        h.add_file(
            "StatController.java",
            r#"@RestController class StatController {
            @GetMapping("/stats") public Stat stat() { return null; }
        }"#,
        );
        h.add_file(
            "Stat.java",
            r#"class Stat {
            @JsonFormat(pattern="yyyy-MM-dd", timezone="GMT+8") LocalDate statDate;
        }"#,
        );
        h.select_paths(&["/project/StatController.java"]);
        let mut p = YapiPlugin::default();
        p.run_command("preview", &h.invocation(), None).unwrap();
        assert!(h.process_runs().is_empty());
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
        let result = last(&h);
        let item = &result["plan"]["items"][0];
        assert_eq!(item["action"], "新增", "{result}");
        assert_eq!(item["payload"]["path"], "/stats");
        let schema: Value =
            serde_json::from_str(item["payload"]["res_body"].as_str().unwrap()).unwrap();
        let field = &schema["properties"]["statDate"];
        assert_eq!(field["type"], "string");
        assert!(field.get("format").is_none());
        assert!(
            field["description"]
                .as_str()
                .unwrap()
                .contains("yyyy-MM-dd")
        );
        assert!(field["description"].as_str().unwrap().contains("GMT+8"));
    }
}
