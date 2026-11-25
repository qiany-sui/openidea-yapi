use openidea_yapi::parse::analyze;
use serde_json::{Value, json};

fn schemas(dto: &str) -> Result<(Value, Value), String> {
    let text = format!(
        r#"@RestController class StatController {{
            @PostMapping("/stats") public Stat stat(@RequestBody Stat input) {{ return input; }}
        }} {dto}"#
    );
    let apis = analyze(
        &[("StatController.java".into(), text)],
        &["StatController.java".into()],
    )?;
    Ok((
        apis[0].request_schema.clone().unwrap(),
        apis[0].response_schema.clone(),
    ))
}

#[test]
fn json_format_stat_date_preserves_custom_pattern_in_request_and_response() {
    for ty in [
        "Date",
        "java.util.Date",
        "LocalDate",
        "LocalDateTime",
        "Instant",
        "OffsetDateTime",
        "ZonedDateTime",
    ] {
        let (request, response) = schemas(&format!(
            r#"class Stat {{
            /** 统计日期 */
            @NotNull @JsonProperty("stat_date")
            @JsonFormat(pattern = "yyyy-MM-dd HH:mm:ss", timezone = "GMT+8", locale = "zh_CN")
            {ty} statDate;
        }}"#
        ))
        .unwrap_or_else(|error| panic!("{ty}: {error}"));
        for schema in [request, response] {
            let field = &schema["properties"]["stat_date"];
            assert_eq!(field["type"], "string");
            assert!(
                field.get("format").is_none(),
                "custom text is not ISO date-time: {field}"
            );
            assert!(
                field.get("pattern").is_none(),
                "Java date patterns are not regexes"
            );
            let desc = field["description"].as_str().unwrap();
            for detail in ["统计日期", "yyyy-MM-dd HH:mm:ss", "GMT+8", "zh_CN"] {
                assert!(desc.contains(detail), "{desc}");
            }
            assert_eq!(schema["required"], json!(["stat_date"]));
        }
    }
}

#[test]
fn json_format_getter_and_record_annotations_reach_the_property() {
    for dto in [
        r#"class Stat { LocalDate statDate; @JsonFormat(pattern="yyyy/MM/dd") public LocalDate getStatDate() { return statDate; } }"#,
        r#"record Stat(@JsonFormat(pattern="yyyy/MM/dd") LocalDate statDate) {}"#,
    ] {
        let (request, response) = schemas(dto).unwrap();
        for schema in [request, response] {
            let field = &schema["properties"]["statDate"];
            assert_eq!(field["type"], "string");
            assert!(field.get("format").is_none());
            assert!(
                field["description"]
                    .as_str()
                    .unwrap()
                    .contains("yyyy/MM/dd")
            );
        }
    }
    let (_, response) = schemas(
        r#"class Stat {
        String name;
        @JsonFormat(pattern="yyyy/MM/dd") public LocalDate getStatDate() { return null; }
    }"#,
    )
    .unwrap();
    assert!(
        response["properties"]["statDate"]["description"]
            .as_str()
            .unwrap()
            .contains("yyyy/MM/dd")
    );
}

#[test]
fn json_format_respects_request_response_precedence_and_quoted_date_literals() {
    let (request, response) = schemas(
        r#"class Stat {
        @JsonFormat(pattern="yyyy/MM/dd", timezone="GMT+8") LocalDateTime statDate;
        @JsonFormat(pattern="yyyy-MM-dd'T'HH:mm:ss", locale="en_GB")
        public LocalDateTime getStatDate() { return statDate; }
    }"#,
    )
    .unwrap();
    let desc = response["properties"]["statDate"]["description"]
        .as_str()
        .unwrap();
    for detail in ["yyyy-MM-dd'T'HH:mm:ss", "en_GB"] {
        assert!(desc.contains(detail), "{desc}");
    }
    assert!(!desc.contains("yyyy/MM/dd"), "{desc}");
    assert!(
        !desc.contains("GMT+8"),
        "getter annotation replaces the field annotation: {desc}"
    );
    let desc = request["properties"]["statDate"]["description"]
        .as_str()
        .unwrap();
    for detail in ["yyyy/MM/dd", "GMT+8"] {
        assert!(desc.contains(detail), "{desc}");
    }
    assert!(!desc.contains("en_GB"), "{desc}");
}

#[test]
fn json_format_shape_controls_scalar_wire_type() {
    for (ty, args, expected) in [
        ("Long", "shape=JsonFormat.Shape.STRING", "string"),
        (
            "BigDecimal",
            "shape=com.fasterxml.jackson.annotation.JsonFormat.Shape.STRING",
            "string",
        ),
        ("Date", "shape=JsonFormat.Shape.NUMBER", "integer"),
        (
            "java.util.Date",
            "shape=JsonFormat.Shape.NUMBER_INT, pattern=\"yyyy-MM-dd\"",
            "integer",
        ),
        ("LocalDate", "shape=JsonFormat.Shape.STRING", "string"),
    ] {
        let (request, response) = schemas(&format!(
            "class Stat {{ @JsonFormat({args}) {ty} statDate; }}"
        ))
        .unwrap();
        for schema in [request, response] {
            let field = &schema["properties"]["statDate"];
            assert_eq!(field["type"], expected, "{ty}: {args}");
            if expected == "integer" {
                assert!(field.get("format").is_none());
                assert!(
                    !field["description"]
                        .as_str()
                        .unwrap()
                        .contains("yyyy-MM-dd")
                );
            }
        }
    }
}

#[test]
fn json_format_does_not_guess_dynamic_patterns_or_unsupported_shapes() {
    for (ty, args, detail) in [
        ("Date", "pattern=Formats.DATE", "pattern"),
        ("Date", "pattern=\"yyyy\" + \"-MM-dd\"", "pattern"),
        ("Date", "timezone=Zones.DEFAULT", "timezone"),
        ("Date", "shape=Shapes.SELECTED", "shape"),
        ("LocalDate", "shape=JsonFormat.Shape.ARRAY", "shape"),
        ("LocalDate", "shape=JsonFormat.Shape.NUMBER", "shape"),
        (
            "List<Date>",
            "with=JsonFormat.Feature.WRITE_SINGLE_ELEM_ARRAYS_UNWRAPPED",
            "with",
        ),
        ("Date", "lenient=Flags.ENABLED", "lenient"),
    ] {
        let error = schemas(&format!(
            "class Stat {{ @JsonFormat({args}) {ty} statDate; }}"
        ))
        .unwrap_err();
        for part in ["statDate", "JsonFormat", detail, "/stats"] {
            assert!(error.contains(part), "{error}");
        }
    }
}

#[test]
fn json_format_defaults_do_not_discard_existing_schema() {
    for args in [
        "",
        "shape=JsonFormat.Shape.ANY, pattern=\"\", timezone=\"##default\", locale=\"##default\", lenient=OptBoolean.DEFAULT",
        "lenient=OptBoolean.FALSE",
    ] {
        let (request, response) = schemas(&format!(
            "class Stat {{ @JsonFormat({args}) LocalDate statDate; }}"
        ))
        .unwrap();
        for schema in [request, response] {
            assert_eq!(schema["properties"]["statDate"]["format"], "date");
        }
    }
}

#[test]
fn json_format_qualified_annotations_and_explicit_record_accessor_are_read() {
    for dto in [
        r#"class Stat { @com.fasterxml.jackson.annotation.JsonFormat(pattern="yyyy/MM/dd") LocalDate statDate; }"#,
        r#"class Stat { LocalDate statDate; @com.fasterxml.jackson.annotation.JsonFormat(pattern="yyyy/MM/dd") public LocalDate getStatDate() { return statDate; } }"#,
        r#"record Stat(LocalDate statDate) { @JsonFormat(pattern="yyyy/MM/dd") public LocalDate statDate() { return statDate; } }"#,
    ] {
        let (_, response) = schemas(dto).unwrap();
        assert!(
            response["properties"]["statDate"]["description"]
                .as_str()
                .unwrap()
                .contains("yyyy/MM/dd")
        );
    }
}

#[test]
fn json_format_preserves_docs_and_does_not_rename_record_accessors() {
    let (request, response) = schemas(
        r#"record Stat(
        @JsonFormat(pattern="yyyy/MM/dd") LocalDate getDate,
        @JsonFormat(shape=JsonFormat.Shape.STRING) boolean isReady) {
        @JsonFormat(pattern="yyyy/MM/dd") public LocalDate getDate() { return getDate; }
        @JsonFormat(shape=JsonFormat.Shape.STRING) public boolean isReady() { return isReady; }
    }"#,
    )
    .unwrap();
    for schema in [request, response] {
        let fields = schema["properties"].as_object().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields["getDate"]["type"], "string");
        assert_eq!(fields["isReady"]["type"], "string");
    }
    let (_, response) = schemas(r#"class Stat {
        /** 统计日期 */
        @com.fasterxml.jackson.annotation.JsonFormat(shape=JsonFormat.Shape.STRING, pattern="yyyy/MM/dd")
        java.time.LocalDate statDate;
    }"#).unwrap();
    let desc = response["properties"]["statDate"]["description"]
        .as_str()
        .unwrap();
    for part in ["统计日期", "yyyy/MM/dd"] {
        assert!(desc.contains(part), "{desc}");
    }
}

#[test]
fn json_format_creator_parameter_overrides_only_the_request_format() {
    let (request, response) = schemas(r#"class Stat {
        @JsonFormat(pattern="yyyy/MM/dd") LocalDate statDate;
        @JsonCreator public Stat(@JsonProperty("statDate") @JsonFormat(pattern="yyyy-MM-dd") LocalDate date) {}
    }"#).unwrap();
    assert!(
        request["properties"]["statDate"]["description"]
            .as_str()
            .unwrap()
            .contains("yyyy-MM-dd")
    );
    assert!(
        response["properties"]["statDate"]["description"]
            .as_str()
            .unwrap()
            .contains("yyyy/MM/dd")
    );
}

#[test]
fn json_format_is_not_lost_when_getter_keeps_the_field_json_alias() {
    let (request, response) = schemas(
        r#"class Stat {
        @JsonProperty("stat_date") @JsonFormat(pattern="yyyy/MM/dd") LocalDate statDate;
        @JsonProperty public LocalDate getStatDate() { return statDate; }
    }"#,
    )
    .unwrap();
    for schema in [request, response] {
        let field = &schema["properties"]["stat_date"];
        assert!(field.get("format").is_none(), "{field}");
        assert!(
            field["description"]
                .as_str()
                .unwrap()
                .contains("yyyy/MM/dd")
        );
    }
}

#[test]
fn json_format_creator_is_a_response_fallback_independent_of_method_order() {
    let creator = r#"@JsonCreator public Stat(@JsonProperty("statDate") @JsonFormat(pattern="yyyy/MM/dd") Date date) {}"#;
    for getter in [
        "",
        "public Date getStatDate() { return statDate; }",
        r#"@JsonFormat(pattern="yyyy-MM-dd") public Date getStatDate() { return statDate; }"#,
    ] {
        for methods in [format!("{creator} {getter}"), format!("{getter} {creator}")] {
            let (request, response) =
                schemas(&format!("class Stat {{ Date statDate; {methods} }}")).unwrap();
            let request_date = &request["properties"]["statDate"];
            assert!(request_date.get("format").is_none());
            assert!(
                request_date["description"]
                    .as_str()
                    .unwrap()
                    .contains("yyyy/MM/dd")
            );
            let response_date = &response["properties"]["statDate"];
            assert!(response_date.get("format").is_none(), "{response_date}");
            let expected = if getter.contains("JsonFormat") {
                "yyyy-MM-dd"
            } else {
                "yyyy/MM/dd"
            };
            assert!(
                response_date["description"]
                    .as_str()
                    .unwrap()
                    .contains(expected)
            );
        }
    }
}
