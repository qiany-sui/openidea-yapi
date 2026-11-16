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
