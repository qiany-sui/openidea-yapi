use crate::{
    dependencies::{DependencyResolver, MavenConfig},
    parse,
    transport::{Client, normalize_base},
    yapi::{self, Plan},
};
use openidea_plugin::{
    Error, Invocation, Plugin,
    host::{self, Project},
    views::{Body, View, ViewEvent, Viewport, Views},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

#[derive(Default, Serialize, Deserialize)]
struct Config {
    base: String,
    #[serde(default)]
    maven: MavenConfig,
}
#[derive(Default)]
struct State {
    config: Config,
    selected: Vec<String>,
    token: Zeroizing<String>,
    plan: Option<Plan>,
    nonce: u64,
    status: String,
}
#[derive(Default)]
pub struct YapiPlugin {
    projects: BTreeMap<String, State>,
    next_nonce: u64,
}

fn project(ctx: &Invocation) -> Result<(Project, String), Error> {
    let p = ctx.project().ok_or("请先打开并信任 Java 项目")?;
    let root = p.root()?;
    Ok((p, root))
}
fn config_key(root: &str) -> String {
    format!(
        "target.{:x}",
        Sha256::digest(format!("{root}\0config").as_bytes())
    )
}
fn check_top_level_types(path: &str, text: &str) -> Result<(), Error> {
    if parse::top_level_type_count(text) > 1 {
        return Err(format!(
            "大项目按需解析要求每个顶层类型使用独立文件，请拆分 {path} 中的多个顶层类型"
        )
        .into());
    }
    Ok(())
}
fn scan(p: &Project, selected: &[String], config: &MavenConfig) -> Result<Vec<parse::Api>, Error> {
    let task = host::begin_task("YApi：解析 Controller 与 DTO", true);
    if selected.is_empty() {
        return Err("请在 Controller 文件上执行「YApi：预览并同步」".into());
    }
    let mut paths = p.find_files("**/*.java", 5001)?;
    // 小项目保留多顶层类/非标准文件名的解析兼容；大项目只读取接口用到的类型。
    let on_demand = paths.len() > 5000;
    if on_demand {
        paths.clear();
    }
    for path in selected {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    let mut total = 0;
    let mut cache = BTreeMap::<String, String>::new();
    let mut read = |path: &str| -> Result<String, Error> {
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if let Some(text) = cache.get(path) {
            return Ok(text.clone());
        }
        task.report(cache.len() as u32, None, Some("读取接口相关 Java 源码"));
        if cache.len() >= 5000 {
            return Err("接口相关 Java 源码超过 5000 个文件，请减少所选 Controller".into());
        }
        let text = p.read_file(path)?;
        if text.len() > 1024 * 1024 {
            return Err("单个 Java 文件超过 1 MiB，请缩小源码文件后同步".into());
        }
        total += text.len();
        if total > 20 * 1024 * 1024 {
            return Err("Java 源码超过 20 MiB，请打开较小的模块后同步".into());
        }
        cache.insert(path.to_owned(), text.clone());
        Ok(text)
    };
    let mut input = Vec::new();
    for path in paths {
        let text = read(&path)?;
        if on_demand {
            check_top_level_types(&path, &text)?;
        }
        input.push((path, text));
    }
    let mut apis = Vec::new();
    for path in selected {
        let mut resolver = DependencyResolver::new(p, path, config);
        apis.extend(parse::analyze_with_resolver(
            &input,
            std::slice::from_ref(path),
            &mut |names| {
                if task.is_cancelled() {
                    return Err("解析已取消".into());
                }
                if on_demand {
                    let mut candidates = std::collections::BTreeSet::new();
                    for name in names {
                        // 成员类型声明在外部类文件中，例如 dto.Outer.Inner 位于 Outer.java。
                        for simple in name.rsplit('.') {
                            let found = p
                                .find_files(&format!("**/{simple}.java"), 5001)
                                .map_err(|e| e.to_string())?;
                            if found.len() > 5000 {
                                return Err(format!(
                                    "类型 {simple} 的候选源码超过 5000 个，请打开较小的模块"
                                ));
                            }
                            candidates.extend(found);
                        }
                    }
                    let mut sources = Vec::new();
                    for candidate in candidates {
                        let text = read(&candidate).map_err(|e| e.to_string())?;
                        if parse::declared_types(&text)
                            .iter()
                            .any(|name| names.contains(name))
                        {
                            check_top_level_types(&candidate, &text).map_err(|e| e.to_string())?;
                            sources.push((candidate, text));
                        }
                    }
                    if !sources.is_empty() {
                        return Ok(sources);
                    }
                }
                resolver.resolve(names)
            },
        )?);
    }
    let mut identities = std::collections::BTreeSet::new();
    for api in &apis {
        if !identities.insert((&api.method, &api.path)) {
            return Err(
                format!("多个 Controller 定义相同接口：{} {}", api.method, api.path).into(),
            );
        }
    }
    Ok(apis)
}
impl YapiPlugin {
    fn state(&mut self, root: &str) -> Result<&mut State, Error> {
        if !self.projects.contains_key(root) {
            let config = host::storage::get(&config_key(root))?
                .map(|s| serde_json::from_str(&s))
                .transpose()
                .map_err(|_| "YApi 本机配置损坏")?
                .unwrap_or_default();
            self.projects.insert(
                root.into(),
                State {
                    config,
                    ..Default::default()
                },
            );
        }
        Ok(self.projects.get_mut(root).unwrap())
    }
    fn publish(&mut self, root: &str) -> Result<(), Error> {
        let s = self.state(root)?;
        host::post_view_message(
            "sync",
            &json!({
                "base":s.config.base,"maven":s.config.maven,"selected":s.selected,
                "hasToken":!s.token.is_empty(),
                "nonce":s.nonce,"plan":s.plan,"status":s.status
            })
            .to_string(),
        )
    }
    fn message(&mut self, ctx: &Invocation, raw: &str) -> Result<(), Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Message {
            action: String,
            #[serde(default)]
            base: String,
            #[serde(default)]
            token: String,
            #[serde(default)]
            maven: Option<MavenConfig>,
            #[serde(default)]
            nonce: u64,
            #[serde(default)]
            selected: Vec<usize>,
        }
        let mut msg: Message = serde_json::from_str(raw).map_err(|_| "无效的 YApi 操作")?;
        let supplied = Zeroizing::new(std::mem::take(&mut msg.token));
        let (p, root) = project(ctx)?;
        if msg.action == "ready" {
            return self.publish(&root);
        }
        if msg.action == "preview" {
            self.next_nonce += 1;
            let nonce = self.next_nonce;
            let state = self.state(&root)?;
            state.plan = None;
            state.nonce = nonce;
            let base = normalize_base(&msg.base)?;
            if state.config.base != base {
                state.token = Zeroizing::new(String::new());
            }
            state.config.base = base;
            if let Some(maven) = msg.maven {
                state.config.maven = maven;
            }
            if !supplied.is_empty() {
                crate::transport::validate_token(&supplied)?;
                state.token = supplied;
            }
            if state.selected.is_empty()
                && let Some(editor) = ctx.editor()
                && editor.path().ends_with(".java")
            {
                state.selected.push(editor.path());
            }
            let apis = scan(&p, &state.selected, &state.config.maven)?;
            let mut client = Client::new(&state.config.base, &state.token)?;
            let plan = yapi::preview(&mut client, &apis)?;
            host::storage::set(
                &config_key(&root),
                &serde_json::to_string(&state.config).map_err(|_| "无法保存配置")?,
            )?;
            state.status = format!(
                "已预览 {} 个接口；目标项目：{}（{}）",
                plan.items.len(),
                plan.project_name,
                plan.project_id
            );
            state.plan = Some(plan);
        } else if msg.action == "sync" {
            let state = self.state(&root)?;
            if msg.nonce != state.nonce {
                return Err("预览已失效，请重新预览".into());
            }
            let plan = state.plan.take().ok_or("预览已失效，请重新预览")?;
            if msg.selected.is_empty() {
                state.status = "未选择接口，没有写入".into();
                return self.publish(&root);
            }
            if !host::confirm(
                "同步到 YApi",
                &format!(
                    "目标：{}\n项目：{}（{}）\n将同步选中的 {} 个接口。不会删除远端接口。",
                    state.config.base,
                    plan.project_name,
                    plan.project_id,
                    msg.selected.len()
                ),
            )? {
                state.status = "已取消，没有写入".into();
                return self.publish(&root);
            }
            let apis = scan(&p, &state.selected, &state.config.maven)?;
            if serde_json::to_value(&apis).ok() != serde_json::to_value(&plan.apis).ok() {
                return Err("源码已变化，请重新预览".into());
            }
            let mut client = Client::new(&state.config.base, &state.token)?;
            state.status = yapi::execute(&mut client, &plan, &msg.selected)?;
        } else if msg.action == "forget" {
            let state = self.state(&root)?;
            state.token = Zeroizing::new(String::new());
            state.plan = None;
            state.status = "已清除当前项目的会话 token，请重新输入后预览".into();
        } else {
            return Err("未知的 YApi 操作".into());
        }
        self.publish(&root)
    }
}
impl Plugin for YapiPlugin {
    fn run_command(
        &mut self,
        id: &str,
        ctx: &Invocation,
        _arg: Option<&str>,
    ) -> Result<Option<String>, Error> {
        if id != "preview" {
            return Err("未知命令".into());
        }
        let (p, root) = project(ctx)?;
        let mut selected = ctx.selected_paths();
        if selected.is_empty()
            && let Some(e) = ctx.editor()
        {
            selected.push(e.path());
        }
        let s = self.state(&root)?;
        s.selected = selected.clone();
        s.plan = None;
        s.status = "正在解析所选 Controller…".into();
        host::reveal_view("sync")?;
        self.publish(&root)?;
        let result = if selected.is_empty() || selected.iter().any(|s| !s.ends_with(".java")) {
            Err("请选择 Java Controller 文件（不支持直接选择目录）".into())
        } else {
            scan(&p, &selected, &self.state(&root)?.config.maven)
        };
        self.state(&root)?.status = match &result {
            Ok(apis) => format!(
                "解析到 {} 个接口。填写 YApi 地址和项目 token，然后预览。",
                apis.len()
            ),
            Err(error) => format!("解析失败：{error}\n未生成同步预览，没有写入 YApi。"),
        };
        self.publish(&root)?;
        host::refresh_view("sync");
        result.map(|_| None)
    }
}
impl Views for YapiPlugin {
    fn render(
        &mut self,
        _view: &str,
        _viewport: Option<Viewport>,
        ctx: &Invocation,
    ) -> Result<View, Error> {
        if ctx.project().is_none() {
            return Ok(View::message(
                "请先打开 Java 项目，再从 Controller 执行 YApi 同步。",
            ));
        }
        Ok(View::new(Body::Html(include_str!("view.html").into())))
    }
    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if let ViewEvent::Message(raw) = event {
            let raw = Zeroizing::new(raw);
            if let Err(e) = self.message(ctx, &raw) {
                let (_, root) = project(ctx)?;
                let s = self.state(&root)?;
                s.plan = None;
                s.status = e.to_string();
                self.publish(&root)?;
            }
        }
        Ok(())
    }
}
