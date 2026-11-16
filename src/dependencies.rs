use openidea_plugin::host::{self, Command, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MavenConfig {
    pub settings: String,
    pub repository: String,
}

pub struct DependencyResolver<'a> {
    project: &'a Project,
    selected: &'a str,
    config: &'a MavenConfig,
    classpath: Option<String>,
}

impl<'a> DependencyResolver<'a> {
    pub fn new(project: &'a Project, selected: &'a str, config: &'a MavenConfig) -> Self {
        Self {
            project,
            selected,
            config,
            classpath: None,
        }
    }

    fn run(&self, command: Command, stdin: Option<&str>) -> Result<String, String> {
        let task = host::begin_task("Java YApi：解析 Maven 依赖类型", true);
        if task.is_cancelled() {
            return Err("已取消依赖解析".into());
        }
        task.report(0, None, Some(&command.program));
        let output = host::run_command(&command, Some(self.project), stdin).map_err(|_| {
            format!(
                "无法运行 {}；请在插件权限页授权 process:{} 并核对可执行文件",
                command.program, command.program
            )
        })?;
        if output.status != Some(0) {
            return Err(format!(
                "{} 依赖解析失败；需要 Java 17+，并确保 Maven 依赖及 maven-dependency-plugin 3.8.1 已在本机缓存。请在 IDE 正常刷新 Maven 依赖后重试；未请求 YApi。",
                command.program
            ));
        }
        if output.stdout.len() > 4 * 1024 * 1024 {
            return Err("依赖解析输出超过 4 MiB".into());
        }
        Ok(output.stdout)
    }

    fn maven(&self, pom: &str) -> Result<Command, String> {
        let mut command =
            Command::new("mvn").args(["-o", "-B", "-ntp", "-Dstyle.color=never", "-f", pom]);
        for path in [&self.config.settings, &self.config.repository] {
            if path.chars().any(char::is_control) {
                return Err("Maven 路径不能含控制字符".into());
            }
        }
        if !self.config.settings.is_empty() {
            command = command.args(["-s", &self.config.settings]);
        }
        if !self.config.repository.is_empty() {
            command = command.arg(&format!("-Dmaven.repo.local={}", self.config.repository));
        }
        Ok(command)
    }

    fn classpath(&mut self) -> Result<&str, String> {
        if self.classpath.is_none() {
            let root = self
                .project
                .root()
                .map_err(|e| e.to_string())?
                .replace('\\', "/");
            let selected = self.selected.replace('\\', "/");
            let relative = selected
                .strip_prefix(&(root.trim_end_matches('/').to_owned() + "/"))
                .ok_or("Controller 不在当前项目内")?;
            let mut directory = relative
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or("")
                .to_string();
            let mut poms = Vec::new();
            loop {
                let path = if directory.is_empty() {
                    "pom.xml".into()
                } else {
                    format!("{directory}/pom.xml")
                };
                if self.project.exists(&path).map_err(|e| e.to_string())? {
                    poms.push(path);
                }
                if directory.is_empty() {
                    break;
                }
                directory = directory
                    .rsplit_once('/')
                    .map(|(d, _)| d)
                    .unwrap_or("")
                    .into();
            }
            let module = poms
                .first()
                .ok_or("外部类型解析当前仅支持 Maven；Controller 上级目录未找到 pom.xml")?;
            let reactor = poms.last().unwrap();
            let reactor_dir = reactor.strip_suffix("pom.xml").unwrap();
            let module_dir = module.strip_suffix("/pom.xml").unwrap_or("");
            let selected_module = module_dir.strip_prefix(reactor_dir).unwrap_or(module_dir);
            let mut cmd = self.maven(reactor)?;
            if module == reactor {
                cmd = cmd.arg("-N");
            } else {
                cmd = cmd.args(["-pl", selected_module, "-am"]);
            }
            cmd = cmd.args([
                "org.apache.maven.plugins:maven-dependency-plugin:3.8.1:tree",
                "-DoutputType=json",
                "-Dskip=false",
                "-DoutputEncoding=UTF-8",
            ]);
            let output = self.run(cmd, None)?;
            // Maven 按 reactor 顺序输出 JSON；-pl/-am 的最后一项是当前模块。
            // 直接消费本次 stdout，避免跳过目标时复用旧缓存。
            let mut current = None;
            let mut tree = None;
            for line in output.lines() {
                let line = line.strip_prefix("[INFO] ").unwrap_or(line);
                if line == "{" {
                    current = Some(String::from("{\n"));
                } else if let Some(text) = current.as_mut() {
                    text.push_str(line);
                    text.push('\n');
                    if line == "}" {
                        tree = Some(
                            serde_json::from_str::<Value>(text)
                                .map_err(|_| "Maven 依赖树格式无效")?,
                        );
                        current = None;
                    }
                }
            }
            if current.is_some() {
                return Err("Maven 依赖树输出不完整".into());
            }
            let tree = tree.ok_or("Maven 没有输出依赖树，请核对 dependency:tree 配置")?;
            let repository = if self.config.repository.is_empty() {
                self.run(
                    self.maven(reactor)?.args([
                        "-N",
                        "-q",
                        "help:evaluate",
                        "-Dexpression=settings.localRepository",
                        "-DforceStdout",
                    ]),
                    None,
                )?
                .trim()
                .to_string()
            } else {
                self.config.repository.clone()
            };
            if repository.is_empty() || repository.chars().any(char::is_control) {
                return Err("无法读取 Maven 本地仓库路径，请在插件中填写本地仓库路径".into());
            }
            let separator =
                if repository.as_bytes().get(1) == Some(&b':') || repository.starts_with("\\\\") {
                    ";"
                } else {
                    ":"
                };
            let mut jars = Vec::new();
            collect_jars(&tree, &repository.replace('\\', "/"), &mut jars)?;
            self.classpath = Some(jars.join(separator));
        }
        Ok(self.classpath.as_deref().unwrap())
    }

    pub fn resolve(&mut self, names: &[String]) -> Result<Vec<(String, String)>, String> {
        let classpath = self.classpath()?.to_string();
        let helper = include_bytes!("../resources/java-types.jar");
        let dir = host::fs::plugin_dir().map_err(|e| e.to_string())?;
        let path = format!("{dir}/java-types-{:x}.jar", Sha256::digest(helper));
        if host::fs::read_bytes(&path).ok().as_deref() != Some(helper) {
            host::fs::write_bytes(&path, helper).map_err(|e| e.to_string())?;
        }
        let input = format!("{classpath}\n{}\n", names.join("\n"));
        let output = self.run(Command::new("java").args(["-jar", &path]), Some(&input))?;
        let sources: BTreeMap<String, String> =
            serde_json::from_str(&output).map_err(|_| "Java 类型读取器返回无效数据")?;
        if sources.keys().any(|n| !names.contains(n)) {
            return Err("Java 类型读取器返回了未请求的类型".into());
        }
        Ok(sources
            .into_iter()
            .map(|(name, source)| (format!("dependency:{name}"), source))
            .collect())
    }
}

fn collect_jars(tree: &Value, repository: &str, out: &mut Vec<String>) -> Result<(), String> {
    if tree["scope"] == "test" {
        return Ok(());
    }
    let value = |key: &str| -> Result<&str, String> {
        let s = tree[key].as_str().ok_or("Maven 依赖坐标不完整")?;
        if s.is_empty()
            || s == "."
            || s.contains("..")
            || !s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("Maven 依赖坐标无效".into());
        }
        Ok(s)
    };
    if matches!(tree["type"].as_str(), Some("jar" | "bundle" | "test-jar")) {
        let group = value("groupId")?.replace('.', "/");
        let artifact = value("artifactId")?;
        let version = value("version")?;
        let classifier = if tree["classifier"].as_str().is_some_and(|s| !s.is_empty()) {
            format!("-{}", value("classifier")?)
        } else if tree["type"] == "test-jar" {
            "-tests".into()
        } else {
            String::new()
        };
        out.push(format!(
            "{repository}/{group}/{artifact}/{version}/{artifact}-{version}{classifier}.jar"
        ));
    }
    if let Some(children) = tree["children"].as_array() {
        for child in children {
            collect_jars(child, repository, out)?;
        }
    }
    Ok(())
}
