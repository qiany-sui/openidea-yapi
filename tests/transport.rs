use openidea_plugin::{ProcessOutput, mock::MockHost};
use openidea_yapi::{
    transport::{Client, normalize_base},
    yapi::Transport,
};
use serde_json::json;

#[test]
fn url_rules_allow_intranet_http_without_credentials_or_query() {
    assert_eq!(
        normalize_base("http://yapi.intranet:3000/yapi/").unwrap(),
        "http://yapi.intranet:3000/yapi"
    );
    for url in [
        "file:///etc/passwd",
        "http://u:p@host",
        "http://host/?token=x",
        "https://host/#x",
        "http://host/\nheader=x",
    ] {
        assert!(normalize_base(url).is_err(), "{url}");
    }
}
#[test]
fn curl_receives_secrets_only_on_stdin_and_redacts_failures() {
    let h = MockHost::install().with_capabilities(&["process:curl"]);
    h.on_process("curl", |args, stdin| {
        assert_eq!(args, &["-q", "--config", "-"]);
        assert!(!args.join(" ").contains("private-token"));
        let s = stdin.unwrap();
        assert!(s.contains("private-token"));
        assert!(s.contains("max-time"));
        assert!(!s.contains("insecure"));
        assert!(!s.contains("location ="));
        Ok(ProcessOutput {
            status: Some(0),
            stdout: format!(
                "{}\nOI_YAPI_STATUS:200",
                json!({"errcode":0,"data":{"_id":1}})
            ),
            stderr: String::new(),
        })
    });
    let mut c = Client::new("http://intranet:3000", "private-token").unwrap();
    assert_eq!(
        c.call("GET", "/api/project/get", json!({})).unwrap()["_id"],
        1
    );
    h.on_process("curl", |_, _| {
        Ok(ProcessOutput {
            status: Some(0),
            stdout: format!(
                "{}\nOI_YAPI_STATUS:200",
                json!({"errcode":400,"errmsg":"private-token"})
            ),
            stderr: "private-token".into(),
        })
    });
    let error = c.call("POST", "/api/interface/add", json!({})).unwrap_err();
    assert!(!error.contains("private-token"));
    assert!(error.contains("400"));
}
#[test]
fn denied_process_and_redirect_are_errors() {
    let mut h = MockHost::install().with_capabilities(&[]);
    let mut c = Client::new("http://intranet", "test-token").unwrap();
    assert!(c.call("GET", "/api/project/get", json!({})).is_err());
    h = h.with_capabilities(&["process:curl"]);
    h.on_process("curl", |_, _| {
        Ok(ProcessOutput {
            status: Some(0),
            stdout: "redirect\nOI_YAPI_STATUS:302".into(),
            stderr: String::new(),
        })
    });
    assert!(
        c.call("GET", "/api/project/get", json!({}))
            .unwrap_err()
            .contains("302")
    );
}
#[test]
fn cancelling_task_stops_before_spawning_curl() {
    let h = MockHost::install().with_capabilities(&["process:curl"]);
    h.on_process("curl", |_, _| panic!("已取消任务不能启动 curl"));
    let mut c = Client::new("http://fixture", "fixture-token").unwrap();
    h.cancel_tasks();
    assert!(
        c.call("GET", "/api/project/get", json!({}))
            .unwrap_err()
            .contains("取消")
    );
    assert!(h.process_runs().is_empty());
}
