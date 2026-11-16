use openidea_yapi::parse::analyze;

#[test]
fn unknown_body_reports_controller_method_route_and_field() {
    let text = r#"@RestController class CallbackController {
        @PostMapping("/internal/v1/callback") public String receive(@RequestBody Input body) { return ""; }
    } class Input { Map<String, MissingValue> result; }"#;
    let error = analyze(
        &[("CallbackController.java".into(), text.into())],
        &["CallbackController.java".into()],
    )
    .unwrap_err();
    for detail in [
        "CallbackController",
        "receive",
        "POST",
        "/internal/v1/callback",
        "请求体",
        "Input.result",
        "MissingValue",
    ] {
        assert!(error.contains(detail), "缺少 {detail}：{error}");
    }
}

#[test]
fn missing_response_source_reports_response_instead_of_request() {
    let text = r#"@RestController class C { @PostMapping("/internal/ack") public ExternalReply<String> receive(@RequestBody String body) { return null; } }"#;
    let error = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap_err();
    assert!(error.contains("响应体"), "{error}");
    assert!(error.contains("ExternalReply"), "{error}");
    assert!(error.contains("/internal/ack"), "{error}");
}

const CONTROLLER: &str = r#"
package demo;
/** 用户接口 */
@RestController
@RequestMapping("/users")
public class UserController {
  /** 创建用户
   * @param body 用户资料
   * @return 用户信息
   */
  @PostMapping("/create")
  public User create(@RequestBody User body, @RequestHeader("X-Tenant") String tenant) { return body; }
  /** 获取用户 */
  @GetMapping("/{id}")
  public User get(@PathVariable("id") Long id, @RequestParam(required = false) String filter) { return null; }
}
"#;
const DTO: &str = r#"package demo;
/** 用户信息 */
public class User {
  /** 用户姓名 */
  @NotBlank
  private String name;
  /** 标签 */
  private java.util.List<String> tags;
}"#;

#[test]
fn controller_becomes_yapi_parameters_and_schemas() {
    let docs = analyze(
        &[
            ("demo/UserController.java".into(), CONTROLLER.into()),
            ("demo/User.java".into(), DTO.into()),
        ],
        &["demo/UserController.java".into()],
    )
    .unwrap();
    assert_eq!(docs.len(), 2);
    let create = &docs[0];
    assert_eq!(create.path, "/users/create");
    assert_eq!(create.method, "POST");
    assert_eq!(create.title, "创建用户");
    assert_eq!(
        create.request_schema.as_ref().unwrap()["properties"]["name"]["description"],
        "用户姓名"
    );
    assert_eq!(
        create.request_schema.as_ref().unwrap()["properties"]["tags"]["items"]["type"],
        "string"
    );
    assert_eq!(create.headers[0]["name"], "X-Tenant");
    assert_eq!(docs[1].path_params[0]["name"], "id");
    assert_eq!(docs[1].query[0]["required"], "0");
}

#[test]
fn unknown_dto_is_reported_instead_of_exporting_empty_schema() {
    let error = analyze(
        &[("UserController.java".into(), CONTROLLER.into())],
        &["UserController.java".into()],
    )
    .unwrap_err();
    assert!(error.contains("User"));
}

#[test]
fn selection_does_not_expand_to_other_controllers() {
    let other = CONTROLLER
        .replace("UserController", "AdminController")
        .replace("/users", "/admin");
    let docs = analyze(
        &[
            ("UserController.java".into(), CONTROLLER.into()),
            ("AdminController.java".into(), other),
            ("User.java".into(), DTO.into()),
        ],
        &["UserController.java".into()],
    )
    .unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.iter().all(|doc| doc.path.starts_with("/users/")));
}

#[test]
fn generic_envelopes_are_resolved_from_source_with_imports() {
    let input=vec![
      ("C.java".into(),r#"package web; import model.User; import model.Result; @RestController class C { @GetMapping("/u") public ResponseEntity<Result<List<User>>> get(){return null;} }"#.into()),
      ("Result.java".into(),"package model; class Result<T> { int code; T data; }".into()),
      ("User.java".into(),DTO.replace("package demo;","package model;")),
      ("Other.java".into(),"package other; class User { int ignored; }".into())
    ];
    let d = analyze(&input, &["C.java".into()]).unwrap();
    assert_eq!(
        d[0].response_schema["properties"]["data"]["items"]["properties"]["name"]["type"],
        "string"
    );
}

#[test]
fn optional_headers_and_swagger_descriptions_are_kept() {
    let input = r#"@RestController class C { @Operation(summary="查询名字") @GetMapping("/x") public String x(@RequestHeader(required=false) String trace){return "";} }"#;
    let d = analyze(&[("C.java".into(), input.into())], &["C.java".into()]).unwrap();
    assert_eq!(d[0].title, "查询名字");
    assert_eq!(d[0].headers[0]["required"], "0");
}
#[test]
fn unresolved_parameter_and_field_serialization_are_rejected() {
    for input in [
        r#"@RestController class C { @GetMapping("/x") public String x(@RequestParam Missing q){return "";} }"#,
        r#"@RestController class C { @GetMapping("/x") public User x(){return null;} } class User { private String a; @JsonValue public String value(){return a;} }"#,
    ] {
        assert!(analyze(&[("C.java".into(), input.into())], &["C.java".into()]).is_err());
    }
}

#[test]
fn initialized_fields_are_not_silently_dropped() {
    let text = r#"@RestController class C { @GetMapping("/x") public D x(){return null;} } class D { String name; List<String> tags = new ArrayList<>(); }"#;
    let d = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    assert_eq!(d[0].response_schema["properties"]["tags"]["type"], "array");
}
#[test]
fn unsupported_field_declarations_do_not_generate_partial_schema() {
    let text = r#"@RestController class C { @GetMapping("/x") public D x(){return null;} } class D { String name; String tags[]; }"#;
    assert!(analyze(&[("C.java".into(), text.into())], &["C.java".into()]).is_err());
}

#[test]
fn review_multiple_initialized_fields_are_rejected_instead_of_truncated() {
    let text = r#"@RestController class C { @GetMapping("/x") public D x(){return null;} } class D { String first="", second=""; }"#;
    assert!(analyze(&[("C.java".into(), text.into())], &["C.java".into()]).is_err());
}
#[test]
fn review_json_ignore_false_preserves_the_field() {
    let text = r#"@RestController class C { @GetMapping("/x") public D x(){return null;} } class D { String first; @JsonIgnore(value = false) String second; }"#;
    let d = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    assert_eq!(
        d[0].response_schema["properties"]["second"]["type"],
        "string"
    );
}
#[test]
fn review_unsupported_json_property_access_is_rejected() {
    let text = r#"@RestController class C { @GetMapping("/x") public D x(){return null;} } class D { String name; @JsonProperty(access = JsonProperty.Access.WRITE_ONLY) String password; }"#;
    assert!(analyze(&[("C.java".into(), text.into())], &["C.java".into()]).is_err());
}
#[test]
fn review_dynamic_parameter_alias_is_rejected() {
    for ann in ["RequestParam", "RequestHeader", "PathVariable"] {
        let text = format!(
            r#"@RestController class C {{ @GetMapping("/x") public String x(@{ann}(name=Names.QUERY) String q){{return "";}} }}"#
        );
        assert!(
            analyze(&[("C.java".into(), text)], &["C.java".into()]).is_err(),
            "{ann}"
        );
    }
}
#[test]
fn review_query_and_path_parameters_can_share_a_name() {
    let text = r#"@RestController class C { @GetMapping("/x/{id}") public String x(@RequestParam("id") String query, @PathVariable("id") String id){return "";} }"#;
    let d = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    assert_eq!(d[0].query.len(), 1);
    assert_eq!(d[0].path_params.len(), 1);
    assert_eq!(d[0].query[0]["name"], "id");
}

#[test]
fn dynamic_json_values_remain_unconstrained() {
    let text = r#"@RestController class C {
      @PostMapping("/dynamic") public Object post(@RequestBody Input body) { return null; }
    } class Input { @NotNull Map<String, Object> result; Object data; }"#;
    let apis = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    let body = apis[0].request_schema.as_ref().unwrap();
    assert_eq!(body["properties"]["result"]["type"], "object");
    assert_eq!(
        body["properties"]["result"]["additionalProperties"],
        serde_json::json!({})
    );
    assert_eq!(body["properties"]["data"], serde_json::json!({}));
    assert!(
        body["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("result"))
    );
    assert_eq!(apis[0].response_schema, serde_json::json!({}));
}

#[test]
fn records_preserve_components_and_generic_response_data() {
    let text = r#"@RestController class C {
      @PostMapping("/record") public Reply<Ack> post(@RequestBody Ack body) { return null; }
    } record Ack(String id, int attempt, boolean accepted) {}
    class Reply<T> { String code; T data; public boolean isSuccess() { return true; } }"#;
    let apis = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    let body = apis[0].request_schema.as_ref().unwrap();
    assert_eq!(body["properties"]["id"]["type"], "string");
    assert_eq!(body["properties"]["attempt"]["type"], "integer");
    let reply = &apis[0].response_schema;
    assert_eq!(
        reply["properties"]["data"]["properties"]["accepted"]["type"],
        "boolean"
    );
    assert_eq!(reply["properties"]["success"]["type"], "boolean");
}

#[test]
fn creator_required_property_allows_dynamic_null_value() {
    let text = r#"@RestController class C {
      @PostMapping("/creator") public Input post(@RequestBody Input body) { return body; }
    } @JsonInclude(JsonInclude.Include.NON_NULL) class Input {
      @NotBlank String id;
      @JsonInclude(JsonInclude.Include.ALWAYS) Object data;
      @JsonCreator public Input(@JsonProperty(value="data", required=true) Object data) { this.data=data; }
    }"#;
    let apis = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    let body = apis[0].request_schema.as_ref().unwrap();
    assert_eq!(body["properties"]["data"], serde_json::json!({}));
    assert!(
        body["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("data"))
    );
    assert!(
        !apis[0].response_schema["required"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("data"))
    );
}

#[test]
fn computed_getters_are_response_properties_only() {
    let text = r#"@RestController class C {
      @PostMapping("/getter") public Input post(@RequestBody Input body) { return body; }
    } class Input {
      String id;
      public boolean isSuccess() { return true; }
      private String getSecret() { return ""; }
      public static String getStatic() { return ""; }
      @JsonIgnore public String getIgnored() { return ""; }
    }"#;
    let apis = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    assert!(
        apis[0].request_schema.as_ref().unwrap()["properties"]
            .get("success")
            .is_none()
    );
    let props = apis[0].response_schema["properties"].as_object().unwrap();
    assert_eq!(props.len(), 2);
    assert_eq!(props["success"]["type"], "boolean");
}

#[test]
fn external_generic_types_resolve_by_import_without_replacing_project_sources() {
    let input = vec![(
        "C.java".into(),
        r#"package app; import external.Reply;
      @RestController class C { @GetMapping("/external") public Reply<Ack> get() { return null; } }
      record Ack(String id) {}"#
            .into(),
    )];
    let mut requested = Vec::new();
    let apis = openidea_yapi::parse::analyze_with_resolver(&input, &["C.java".into()], &mut |names| {
        requested.extend_from_slice(names);
        Ok(vec![("dependency:external.Reply".into(), "package external; class Reply<T> { String code; T data; public boolean isSuccess() { return true; } }".into())])
    }).unwrap();
    assert_eq!(requested, ["external.Reply"]);
    assert_eq!(
        apis[0].response_schema["properties"]["data"]["properties"]["id"]["type"],
        "string"
    );
    assert_eq!(
        apis[0].response_schema["properties"]["success"]["type"],
        "boolean"
    );
}

#[test]
fn getter_annotations_apply_to_existing_request_fields() {
    let text = r#"@RestController class C { @PostMapping("/annotations") public D post(@RequestBody D body){ return body; } }
    class D { String id; String secret;
      @JsonProperty("wire") public String getId(){return id;}
      @JsonIgnore public String getSecret(){return secret;}
    }"#;
    let apis = analyze(&[("C.java".into(), text.into())], &["C.java".into()]).unwrap();
    for shape in [
        apis[0].request_schema.as_ref().unwrap(),
        &apis[0].response_schema,
    ] {
        let props = shape["properties"].as_object().unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props["wire"]["type"], "string");
    }
}

#[test]
fn unsupported_annotated_non_getter_is_rejected_without_partial_model() {
    let text = r#"@RestController class C { @GetMapping("/annotations") public D get(){return null;} }
    class D { String id; @JsonProperty("computed") public String value(){return id;} }"#;
    assert!(analyze(&[("C.java".into(), text.into())], &["C.java".into()]).is_err());
}

#[test]
fn unsupported_class_serialization_is_rejected() {
    for annotation in [
        "JsonSerialize",
        "JsonDeserialize",
        "JsonNaming",
        "JsonAutoDetect",
    ] {
        let text = format!(
            r#"@RestController class C {{ @GetMapping("/custom") public D get(){{return null;}} }} @{annotation} class D {{ String id; }}"#
        );
        assert!(
            analyze(&[("C.java".into(), text)], &["C.java".into()]).is_err(),
            "{annotation}"
        );
    }
}
