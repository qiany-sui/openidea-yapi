use openidea_plugin::{ProcessOutput, mock::MockHost};
use openidea_yapi::dependencies::{DependencyResolver, MavenConfig};
use serde_json::json;

#[test]
fn resolves_external_generic_from_reactor_dependency_tree() {
    let h = MockHost::install().with_capabilities(&["project.read", "process:mvn", "process:java"]);
    h.add_file("pom.xml", "<project/>");
    h.add_file("web/pom.xml", "<project/>");
    h.on_process("mvn", |cmd, _| {
        assert!(cmd.contains(&"-o".into()));
        let stdout = if cmd.iter().any(|a| a == "-Dexpression=settings.localRepository") {
            "/repo with spaces".into()
        } else {
            assert!(cmd.windows(2).any(|w| w == ["-pl", "web"]));
            assert!(!cmd.iter().any(|a| a.starts_with("-DoutputFile=")));
            format!("[INFO] {{\n[INFO]   \"artifactId\": \"parent\"\n[INFO] }}\n[INFO] {{\n[INFO]   \"groupId\":\"fixture\",\"artifactId\":\"web\",\"version\":\"1\",\"type\":\"jar\",\"children\": [{}]\n[INFO] }}\n[INFO] BUILD SUCCESS", json!({"groupId":"external","artifactId":"core","version":"2-SNAPSHOT","type":"jar","scope":"compile"}))
        };
        Ok(ProcessOutput {status:Some(0),stdout,stderr:String::new()})
    });
    h.on_process("java", |cmd, stdin| {
        assert_eq!(cmd[0], "-jar");
        let text = stdin.unwrap();
        assert!(text.contains("/repo with spaces/external/core/2-SNAPSHOT/core-2-SNAPSHOT.jar"));
        assert!(text.ends_with("\nexternal.Reply\n"));
        Ok(ProcessOutput {
            status: Some(0),
            stdout: json!({"external.Reply":"package external; class Reply<T> { T data; }"})
                .to_string(),
            stderr: String::new(),
        })
    });
    let project = h.invocation().project().unwrap();
    let config = MavenConfig::default();
    let mut resolver = DependencyResolver::new(&project, "/project/web/src/C.java", &config);
    let found = resolver.resolve(&["external.Reply".into()]).unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0].1.contains("T data"));
    resolver.resolve(&["external.Reply".into()]).unwrap();
    assert_eq!(
        h.process_runs()
            .iter()
            .filter(|r| r.program == "mvn")
            .count(),
        2
    );
}

#[test]
fn denied_dependency_process_does_not_fall_back_or_disclose_output() {
    let h = MockHost::install().with_capabilities(&["project.read"]);
    h.add_file("pom.xml", "<project/>");
    let project = h.invocation().project().unwrap();
    let config = MavenConfig::default();
    let mut resolver = DependencyResolver::new(&project, "/project/src/C.java", &config);
    let error = resolver.resolve(&["external.Reply".into()]).unwrap_err();
    assert!(error.contains("process:mvn"));
    assert!(!error.contains("token"));
}
