use openidea_yapi::{
    parse,
    yapi::{Transport, execute, preview},
};
use serde_json::{Value, json};

#[derive(Default)]
struct Server {
    interfaces: Vec<Value>,
    writes: Vec<(String, Value)>,
}
impl Transport for Server {
    fn call(&mut self, method: &str, path: &str, body: Value) -> Result<Value, String> {
        if method == "GET" {
            return Ok(match path {
                "/api/project/get" => json!({"_id":1,"name":"测试"}),
                "/api/interface/list_menu" => {
                    json!([{"_id":2,"name":"Demo","list":self.interfaces.iter().map(|i|json!({"_id":i["_id"],"method":i["method"],"path":i["path"]})).collect::<Vec<_>>()}])
                }
                "/api/interface/get" => self
                    .interfaces
                    .iter()
                    .find(|i| i["_id"] == body["id"])
                    .unwrap()
                    .clone(),
                _ => panic!("unexpected GET {path}"),
            });
        }
        self.writes.push((path.into(), body.clone()));
        match path {
            "/api/interface/add" => {
                let mut b = body;
                for field in ["req_query", "req_headers", "req_params"] {
                    for p in b[field].as_array_mut().unwrap() {
                        p["_id"] = json!("generated-id");
                        if field == "req_params" {
                            p.as_object_mut().unwrap().remove("required");
                        }
                    }
                }
                b["_id"] = json!(10);
                self.interfaces.push(b);
            }
            "/api/interface/up" => {
                let i = self
                    .interfaces
                    .iter_mut()
                    .find(|i| i["_id"] == body["id"])
                    .unwrap();
                for (k, v) in body.as_object().unwrap() {
                    i[k] = v.clone();
                }
            }
            _ => panic!("unexpected POST {path}"),
        };
        Ok(json!({}))
    }
}
fn docs() -> Vec<parse::Api> {
    parse::analyze(&[("Demo.java".into(),r#"@RestController public class Demo { /** 查询 */ @GetMapping("/hello") public String hello(@RequestParam String name) {return "";} }"#.into())], &["Demo.java".into()]).unwrap()
}

#[test]
fn preview_does_not_write_and_retry_is_idempotent() {
    let mut s = Server::default();
    let p = preview(&mut s, &docs()).unwrap();
    assert!(s.writes.is_empty());
    assert_eq!(p.items[0].action, "新增");
    execute(&mut s, &p, &[0]).unwrap();
    assert_eq!(s.interfaces.len(), 1);
    let retry = preview(&mut s, &docs()).unwrap();
    assert_eq!(retry.items[0].action, "未变化");
    execute(&mut s, &retry, &[0]).unwrap();
    assert_eq!(s.writes.len(), 1);
}
#[test]
fn update_keeps_manual_fields_and_interface_id() {
    let mut s = Server::default();
    let p = preview(&mut s, &docs()).unwrap();
    execute(&mut s, &p, &[0]).unwrap();
    s.interfaces[0]["status"] = json!("done");
    s.interfaces[0]["markdown"] = json!("手工备注");
    s.interfaces[0]["req_query"][0]["example"] = json!("张三");
    s.interfaces[0]["catid"] = json!(99);
    let mut updated = docs();
    updated[0].title = "新标题".into();
    let p = preview(&mut s, &updated).unwrap();
    execute(&mut s, &p, &[0]).unwrap();
    let last = &s.writes.last().unwrap().1;
    assert_eq!(last["id"], 10);
    assert!(last.get("catid").is_none());
    assert!(last.get("status").is_none());
    assert!(last.get("markdown").is_none());
    assert_eq!(s.interfaces[0]["status"], "done");
    assert_eq!(s.interfaces[0]["markdown"], "手工备注");
    assert_eq!(s.interfaces[0]["req_query"][0]["example"], "张三");
}
#[test]
fn stale_preview_and_empty_selection_do_not_write() {
    let mut s = Server::default();
    let p = preview(&mut s, &docs()).unwrap();
    execute(&mut s, &p, &[]).unwrap();
    assert!(s.writes.is_empty());
    execute(&mut s, &p, &[0]).unwrap();
    let p = preview(&mut s, &docs()).unwrap();
    s.interfaces[0]["title"] = json!("同事修改");
    assert!(execute(&mut s, &p, &[0]).unwrap_err().contains("变化"));
    assert_eq!(s.writes.len(), 1);
}
#[test]
fn duplicate_remote_identity_is_rejected() {
    let mut s = Server::default();
    let p = preview(&mut s, &docs()).unwrap();
    execute(&mut s, &p, &[0]).unwrap();
    s.interfaces.push(s.interfaces[0].clone());
    assert!(preview(&mut s, &docs()).is_err());
}

#[test]
fn yapi_generated_parameter_ids_do_not_cause_repeated_updates() {
    let mut s = Server::default();
    let docs=parse::analyze(&[("C.java".into(),r#"@RestController class C { @GetMapping("/x/{id}") public String x(@PathVariable String id){return "";} }"#.into())],&["C.java".into()]).unwrap();
    // 使用现有分类，避免本测试混入分类创建。
    let mut docs = docs;
    docs[0].controller = "Demo".into();
    let p = preview(&mut s, &docs).unwrap();
    execute(&mut s, &p, &[0]).unwrap();
    assert_eq!(preview(&mut s, &docs).unwrap().items[0].action, "未变化");
}

#[test]
fn review_schema_changes_remove_obsolete_structural_constraints() {
    let mut s = Server::default();
    let mut d = docs();
    d[0].response_schema =
        json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]});
    let p = preview(&mut s, &d).unwrap();
    execute(&mut s, &p, &[0]).unwrap();
    d[0].response_schema = json!({"type":"object","additionalProperties":{"type":"string"}});
    let p = preview(&mut s, &d).unwrap();
    let schema: Value =
        serde_json::from_str(p.items[0].payload["res_body"].as_str().unwrap()).unwrap();
    assert!(schema.get("properties").is_none());
    assert!(schema.get("required").is_none());
    execute(&mut s, &p, &[0]).unwrap();
    d[0].response_schema =
        json!({"type":"object","properties":{"name":{"type":"string"}},"required":[]});
    let p = preview(&mut s, &d).unwrap();
    let schema: Value =
        serde_json::from_str(p.items[0].payload["res_body"].as_str().unwrap()).unwrap();
    assert!(schema.get("additionalProperties").is_none());
}
