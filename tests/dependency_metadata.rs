use openidea_yapi::parse::analyze_with_resolver;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
#[test]
fn real_dependency_metadata_cannot_hide_custom_jackson_serialization() {
    let java = std::env::var("YAPI_TEST_JAVA").unwrap_or("java".into());
    let javac = std::path::Path::new(&java).with_file_name(if cfg!(windows) {
        "javac.exe"
    } else {
        "javac"
    });
    let dir = std::env::temp_dir().join(format!("yapi-metadata-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("JsonSerialize.java"), "package com.fasterxml.jackson.databind.annotation; import java.lang.annotation.*; @Retention(RetentionPolicy.RUNTIME) public @interface JsonSerialize {}").unwrap();
    fs::write(dir.join("D.java"), "package fixture; @com.fasterxml.jackson.databind.annotation.JsonSerialize public class D<T> { public T data; }").unwrap();
    let output = Command::new(javac)
        .args(["--release", "17", "-d"])
        .arg(&dir)
        .arg(dir.join("JsonSerialize.java"))
        .arg(dir.join("D.java"))
        .output()
        .expect("JDK 17+ is required for dependency metadata tests");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut child = Command::new(java)
        .args([
            "-jar",
            concat!(env!("CARGO_MANIFEST_DIR"), "/resources/java-types.jar"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{}\nfixture.D", dir.display()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let metadata: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&output.stdout).unwrap();
    let source = r#"import fixture.D; @RestController class C { @GetMapping("/external") public D<String> get(){return null;} }"#;
    let result = analyze_with_resolver(
        &[("C.java".into(), source.into())],
        &["C.java".into()],
        &mut |_| {
            Ok(metadata
                .iter()
                .map(|(k, v)| (format!("dependency:{k}"), v.clone()))
                .collect())
        },
    );
    fs::remove_dir_all(dir).unwrap();
    assert!(result.is_err(), "custom serializer was silently ignored");
    assert!(result.unwrap_err().contains("JsonSerialize"));
}

#[test]
fn json_format_from_compiled_fields_getters_and_record_components_reaches_schema() {
    let java = std::env::var("YAPI_TEST_JAVA").unwrap_or("java".into());
    let javac = std::path::Path::new(&java).with_file_name(if cfg!(windows) {
        "javac.exe"
    } else {
        "javac"
    });
    let dir = std::env::temp_dir().join(format!("yapi-format-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("JsonFormat.java"), r#"package com.fasterxml.jackson.annotation;
        import java.lang.annotation.*;
        @Retention(RetentionPolicy.RUNTIME) @Target({ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER})
        public @interface JsonFormat {
            String pattern() default ""; String timezone() default "";
            Shape shape() default Shape.ANY;
            enum Shape { ANY, STRING }
        }"#).unwrap();
    fs::write(
        dir.join("Stat.java"),
        r#"package fixture;
        import com.fasterxml.jackson.annotation.JsonFormat;
        public class Stat {
            @JsonFormat(pattern="yyyy/MM/dd", timezone="GMT+8") public java.util.Date statDate;
            private Long id;
            @JsonFormat(shape=JsonFormat.Shape.STRING) public Long getId() { return id; }
        }"#,
    )
    .unwrap();
    let compiled = Command::new(&javac)
        .args(["--release", "8", "-d"])
        .arg(&dir)
        .arg(dir.join("JsonFormat.java"))
        .arg(dir.join("Stat.java"))
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    fs::write(dir.join("StatRecord.java"), r#"package fixture;
        import com.fasterxml.jackson.annotation.JsonFormat;
        public record StatRecord<T>(@JsonFormat(pattern="yyyy/MM/dd") java.time.LocalDate statDate, T data) implements java.io.Serializable {}"#).unwrap();
    fs::write(dir.join("PlainRecord.java"), r#"package fixture;
        import com.fasterxml.jackson.annotation.JsonFormat;
        public record PlainRecord(@JsonFormat(pattern="yyyy/MM/dd") java.time.LocalDate statDate, String data) {}"#).unwrap();
    let compiled = Command::new(&javac)
        .args(["--release", "17", "-cp"])
        .arg(&dir)
        .arg("-d")
        .arg(&dir)
        .arg(dir.join("StatRecord.java"))
        .arg(dir.join("PlainRecord.java"))
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let mut child = Command::new(java)
        .args([
            "-jar",
            concat!(env!("CARGO_MANIFEST_DIR"), "/resources/java-types.jar"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(
        child.stdin.take().unwrap(),
        "{}\nfixture.Stat\nfixture.StatRecord\nfixture.PlainRecord",
        dir.display()
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let metadata: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&output.stdout).unwrap();
    fs::remove_dir_all(&dir).unwrap();
    for ty in ["Stat", "StatRecord<String>", "PlainRecord"] {
        let source = format!(
            r#"import fixture.Stat; import fixture.StatRecord; import fixture.PlainRecord;
            @RestController class C {{ @PostMapping("/stats") public {ty} stat(@RequestBody {ty} body) {{ return body; }} }}"#
        );
        let apis = analyze_with_resolver(
            &[("C.java".into(), source)],
            &["C.java".into()],
            &mut |_| {
                Ok(metadata
                    .iter()
                    .map(|(k, v)| (format!("dependency:{k}"), v.clone()))
                    .collect())
            },
        )
        .unwrap_or_else(|error| panic!("{ty}: {error}"));
        for schema in [
            apis[0].request_schema.as_ref().unwrap(),
            &apis[0].response_schema,
        ] {
            let field = &schema["properties"]["statDate"];
            assert_eq!(field["type"], "string");
            assert!(field.get("format").is_none());
            assert!(
                field["description"]
                    .as_str()
                    .unwrap()
                    .contains("yyyy/MM/dd")
            );
            if ty == "Stat" {
                assert_eq!(schema["properties"]["id"]["type"], "string");
                assert!(field["description"].as_str().unwrap().contains("GMT+8"));
            } else {
                assert_eq!(schema["properties"]["data"]["type"], "string");
            }
        }
    }
}
