use openidea_yapi::parse::analyze;

#[test]
fn nested_types_keep_enclosing_identity_and_lexical_scope() {
    let input = vec![
        (
            "C.java".into(),
            r#"package demo; @RestController class C {
            @GetMapping("/a") public A a() { return null; }
            @GetMapping("/b") public B b() { return null; }
        }"#
            .into(),
        ),
        (
            "A.java".into(),
            r#"package demo; class A {
            Detail detail;
            static class Detail { Long count; Leaf leaf; }
            static class Leaf { String label; }
        }"#
            .into(),
        ),
        (
            "B.java".into(),
            r#"package demo; class B {
            Detail detail;
            static class Detail { Boolean enabled; }
        }"#
            .into(),
        ),
        (
            "Detail.java".into(),
            "package demo; class Detail { String unrelated; }".into(),
        ),
    ];
    let docs = analyze(&input, &["C.java".into()]).unwrap();
    let a = &docs[0].response_schema["properties"]["detail"]["properties"];
    assert_eq!(a["count"]["type"], "integer");
    assert_eq!(a["leaf"]["properties"]["label"]["type"], "string");
    assert_eq!(a.as_object().unwrap().len(), 2);
    let b = &docs[1].response_schema["properties"]["detail"]["properties"];
    assert_eq!(b["enabled"]["type"], "boolean");
    assert_eq!(b.as_object().unwrap().len(), 1);
}

#[test]
fn nested_types_resolve_qualified_and_imported_names() {
    for (package, imports, ty) in [
        ("web", "", "model.Outer.Detail"),
        ("web", "import model.Outer;", "Outer.Detail"),
        ("model", "", "Outer.Detail"),
        ("web", "import model.Outer.Detail;", "Detail"),
        ("web", "import model.Outer.*;", "Detail"),
    ] {
        let controller = format!(
            r#"package {package}; {imports} @RestController class C {{
            @PostMapping("/detail") public {ty} get(@RequestBody {ty} body) {{ return body; }}
        }}"#
        );
        let docs = analyze(
            &[
                ("C.java".into(), controller),
                (
                    "Outer.java".into(),
                    "package model; class Outer { static class Detail { String name; } }".into(),
                ),
            ],
            &["C.java".into()],
        )
        .unwrap_or_else(|error| panic!("{imports} {ty}: {error}"));
        assert_eq!(
            docs[0].response_schema["properties"]["name"]["type"],
            "string"
        );
        assert_eq!(
            docs[0].request_schema.as_ref().unwrap()["properties"]["name"]["type"],
            "string"
        );
    }
}

#[test]
fn nested_generics_records_and_annotations_keep_their_shapes() {
    let input = vec![
        (
            "C.java".into(),
            r#"package demo; @RestController class C {
            @GetMapping("/all") public Outer get() { return null; }
        }"#
            .into(),
        ),
        (
            "Outer.java".into(),
            r#"package demo; class Outer {
            Box<Item> box;
            Row row;
            @Schema(description = "数据容器")
            static class Box<T> { T value; }
            static class Item { String name; }
            public record Row(Long total, String label) {}
        }"#
            .into(),
        ),
    ];
    let docs = analyze(&input, &["C.java".into()]).unwrap();
    let properties = &docs[0].response_schema["properties"];
    assert_eq!(
        properties["box"]["properties"]["value"]["properties"]["name"]["type"],
        "string"
    );
    assert_eq!(properties["row"]["properties"]["total"]["type"], "integer");
    assert_eq!(properties["row"]["properties"]["label"]["type"], "string");
}

#[test]
fn nested_cycles_still_fail_explicitly() {
    let input = vec![
        (
            "C.java".into(),
            r#"package demo; @RestController class C {
            @GetMapping("/loop") public Outer.Node get() { return null; }
        }"#
            .into(),
        ),
        (
            "Outer.java".into(),
            r#"package demo; class Outer {
            static class Node { Outer.Node next; }
        }"#
            .into(),
        ),
    ];
    let error = analyze(&input, &["C.java".into()]).unwrap_err();
    assert!(error.contains("循环引用"), "{error}");
}

#[test]
fn class_literals_on_methods_are_not_nested_type_declarations() {
    let input = r#"@RestController class C {
        @GetMapping("/literal") @Schema(implementation = String.class)
        public String get() { return ""; }
    }"#;
    let docs = analyze(&[("C.java".into(), input.into())], &["C.java".into()]).unwrap();
    assert_eq!(docs[0].path, "/literal");
    assert_eq!(docs[0].response_schema["type"], "string");
}

#[test]
fn record_parameter_names_are_not_nested_record_declarations() {
    for method in [
        "public String save(@RequestBody String record) { return record; }",
        "public String record(@RequestBody String body) { return body; }",
        "@Marker(record = true) public String save(@RequestBody String body) { return body; }",
    ] {
        let input = format!(
            r#"@RestController class C {{
            @PostMapping("/record") {method}
        }}"#
        );
        let docs = analyze(&[("C.java".into(), input)], &["C.java".into()])
            .unwrap_or_else(|error| panic!("{method}: {error}"));
        assert_eq!(docs[0].path, "/record");
        assert_eq!(docs[0].request_schema.as_ref().unwrap()["type"], "string");
        assert_eq!(docs[0].response_schema["type"], "string");
    }
}

#[test]
fn top_level_controllers_keep_their_own_mapping_arrays() {
    let input = r#"@RestController @RequestMapping(path={"/first", "/first-alt"}) class First {
        @GetMapping("/value") public String get() { return ""; }
    }
    @RestController @RequestMapping(path={"/second", "/second-alt"}) class Second {
        @GetMapping("/value") public String get() { return ""; }
    }"#;
    let docs = analyze(
        &[("Controllers.java".into(), input.into())],
        &["Controllers.java".into()],
    )
    .unwrap();
    let paths: Vec<_> = docs.iter().map(|api| api.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/first/value",
            "/first-alt/value",
            "/second/value",
            "/second-alt/value"
        ]
    );
}

#[test]
fn nested_types_retain_jackson_annotations_on_their_own_header() {
    let input = r#"@RestController class C {
        @GetMapping("/detail") public Outer.Detail get() { return null; }
    }
    class Outer {
        @JsonProperty("date") LocalDate date;
        @JsonFormat(shape=JsonFormat.Shape.ARRAY)
        static class Detail { String name; }
    }"#;
    let error = analyze(&[("C.java".into(), input.into())], &["C.java".into()]).unwrap_err();
    assert!(error.contains("响应体"), "{error}");
    assert!(
        error.contains("类型 Outer.Detail 使用暂不支持的 @JsonFormat"),
        "{error}"
    );
    assert!(!error.contains("JsonProperty"), "{error}");
}
