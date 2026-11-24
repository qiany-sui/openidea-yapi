use crate::java::{self, Ann, JavaClass, JavaFile};
use crate::spring;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Api {
    pub method: String,
    pub path: String,
    pub title: String,
    pub controller: String,
    pub description: String,
    pub query: Vec<Value>,
    pub path_params: Vec<Value>,
    pub headers: Vec<Value>,
    pub request_schema: Option<Value>,
    pub response_schema: Value,
}

struct Source {
    classes: Vec<JavaClass>,
    package: String,
    imports: Vec<String>,
}

struct Sources {
    files: Vec<Source>,
    missing: std::cell::RefCell<Vec<String>>,
}
impl std::ops::Deref for Sources {
    type Target = [Source];
    fn deref(&self) -> &Self::Target {
        &self.files
    }
}

fn source(text: &str) -> Source {
    let java = JavaFile::parse(text);
    let code: String = java.code.iter().collect();
    let mut package = String::new();
    let mut imports = Vec::new();
    for part in code.split(';') {
        let p = part.trim();
        if let Some(p) = p.strip_prefix("package ") {
            package = p.trim().into();
        }
        if let Some(p) = p.strip_prefix("import ") {
            imports.push(p.trim().strip_prefix("static ").unwrap_or(p.trim()).into());
        }
    }
    Source {
        classes: java.classes,
        package,
        imports,
    }
}

// 文件路径仅用于缩小查找范围，真实类型身份以源码声明为准。
pub(crate) fn declared_types(text: &str) -> Vec<String> {
    let s = source(text);
    s.classes
        .iter()
        .map(|c| {
            if s.package.is_empty() {
                c.qualified_name.clone()
            } else {
                format!("{}.{}", s.package, c.qualified_name)
            }
        })
        .collect()
}

pub(crate) fn top_level_type_count(text: &str) -> usize {
    JavaFile::parse(text)
        .classes
        .iter()
        .filter(|c| !c.qualified_name.contains('.'))
        .count()
}

fn generic(t: &str) -> Result<(&str, Vec<&str>), String> {
    if let Some(i) = t.find('<') {
        let body = t[i + 1..]
            .strip_suffix('>')
            .ok_or_else(|| format!("泛型格式无法解析：{t}"))?;
        let mut depth = 0;
        let mut start = 0;
        let mut args = Vec::new();
        for (i, c) in body.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => {
                    args.push(body[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        args.push(body[start..].trim());
        Ok((t[..i].trim(), args))
    } else {
        Ok((t.trim(), Vec::new()))
    }
}

fn resolve<'a>(
    name: &str,
    context: &Source,
    scope: &str,
    files: &'a Sources,
) -> Result<(&'a Source, &'a JavaClass), String> {
    let qualify = |s: &Source, c: &JavaClass| {
        if s.package.is_empty() {
            c.qualified_name.clone()
        } else {
            format!("{}.{}", s.package, c.qualified_name)
        }
    };
    let all: Vec<_> = files
        .iter()
        .flat_map(|s| s.classes.iter().map(move |c| (s, c)))
        .collect();
    // 成员类型按词法作用域从内向外查找，不能与其他外部类的同名成员混用。
    let mut enclosing = Some(scope);
    while let Some(owner) = enclosing {
        let local = if context.package.is_empty() {
            format!("{owner}.{name}")
        } else {
            format!("{}.{owner}.{name}", context.package)
        };
        let matches: Vec<_> = all
            .iter()
            .copied()
            .filter(|(s, c)| qualify(s, c) == local)
            .collect();
        match matches.as_slice() {
            [found] => return Ok(*found),
            [] => {}
            _ => return Err(format!("类型 {name} 有同名歧义；未生成文档")),
        }
        enclosing = owner.rsplit_once('.').map(|(parent, _)| parent);
    }
    let (first, suffix) = name
        .split_once('.')
        .map(|(first, rest)| (first, format!(".{rest}")))
        .unwrap_or((name, String::new()));
    let explicit: Vec<_> = context
        .imports
        .iter()
        .filter(|i| i.ends_with(&format!(".{first}")))
        .map(|i| format!("{i}{suffix}"))
        .collect();
    let wanted = if !explicit.is_empty() {
        explicit
    } else {
        let mut v = vec![if context.package.is_empty() {
            name.into()
        } else {
            format!("{}.{name}", context.package)
        }];
        v.extend(
            context
                .imports
                .iter()
                .filter_map(|i| i.strip_suffix('*').map(|i| format!("{i}{name}"))),
        );
        if name.contains('.') && !v.contains(&name.to_owned()) {
            v.push(name.into());
        }
        v
    };
    let matches: Vec<_> = all
        .into_iter()
        .filter(|(s, c)| wanted.contains(&qualify(s, c)))
        .collect();
    match matches.as_slice() {
        [found] => Ok(*found),
        [] => {
            files.missing.replace(wanted);
            Err(format!("找不到类型 {name} 的项目或依赖定义；未生成文档"))
        }
        _ => Err(format!("类型 {name} 有同名歧义；未生成文档")),
    }
}

fn schema(
    ty: &str,
    context: &Source,
    scope: &str,
    files: &Sources,
    vars: &BTreeMap<String, Value>,
    stack: &mut Vec<String>,
    response: bool,
) -> Result<Value, String> {
    let t = ty.trim();
    if let Some(v) = vars.get(t) {
        return Ok(v.clone());
    }
    if let Some(inner) = t.strip_suffix("[]") {
        return Ok(
            json!({"type":"array","items":schema(inner,context,scope,files,vars,stack,response)?}),
        );
    }
    let (base, args) = generic(t)?;
    let short = base.rsplit('.').next().unwrap_or(base);
    let simple = match short {
        "String" | "char" | "Character" | "UUID" => Some(json!({"type":"string"})),
        "boolean" | "Boolean" => Some(json!({"type":"boolean"})),
        "byte" | "Byte" | "short" | "Short" | "int" | "Integer" | "long" | "Long"
        | "BigInteger" => Some(json!({"type":"integer"})),
        "float" | "Float" | "double" | "Double" | "BigDecimal" | "Number" => {
            Some(json!({"type":"number"}))
        }
        "LocalDate" => Some(json!({"type":"string","format":"date"})),
        "Date" | "Instant" | "LocalDateTime" | "OffsetDateTime" | "ZonedDateTime" => {
            Some(json!({"type":"string","format":"date-time"}))
        }
        "void" | "Void" => Some(json!({"type":"null"})),
        _ => None,
    };
    if let Some(s) = simple {
        return Ok(s);
    }
    if short == "Object" {
        return Ok(json!({}));
    }
    if matches!(short, "ResponseEntity" | "HttpEntity") && args.len() == 1 {
        return schema(args[0], context, scope, files, vars, stack, response);
    }
    if matches!(
        short,
        "List" | "Set" | "Collection" | "Iterable" | "ArrayList"
    ) && args.len() == 1
    {
        return Ok(
            json!({"type":"array","items":schema(args[0],context,scope,files,vars,stack,response)?}),
        );
    }
    if matches!(short, "Map" | "HashMap" | "LinkedHashMap")
        && args.len() == 2
        && matches!(args[0], "String" | "java.lang.String")
    {
        return Ok(
            json!({"type":"object","additionalProperties":schema(args[1],context,scope,files,vars,stack,response)?}),
        );
    }
    let (file, cls) = resolve(base, context, scope, files)?;
    if cls.incomplete {
        return Err(format!("类型 {base} 有无法完整解析的字段声明"));
    }
    if !matches!(cls.kind.as_str(), "class" | "record")
        || cls.declaration.split_whitespace().any(|w| w == "extends")
    {
        return Err(format!(
            "类型 {base} 使用了暂不支持的 {} 或继承；请先补充解析支持",
            cls.kind
        ));
    }
    let key = format!("{}.{}<{:?}>", file.package, cls.qualified_name, args);
    if stack.contains(&key) || stack.len() >= 20 {
        return Err(format!("类型 {t} 存在循环引用或嵌套过深"));
    }
    let tail = cls
        .declaration
        .split_once(&cls.name)
        .map(|(_, s)| s.trim())
        .unwrap_or("");
    let params = if tail.starts_with('<') {
        let end = tail.find('>').ok_or("无法解析泛型声明")?;
        tail[1..end].split(',').map(str::trim).collect::<Vec<_>>()
    } else {
        vec![]
    };
    if params.len() != args.len() || params.iter().any(|p| p.contains(' ')) {
        return Err(format!("类型 {t} 的泛型参数无法完整解析"));
    }
    let mut bindings = BTreeMap::new();
    for (p, a) in params.iter().zip(args) {
        bindings.insert(
            (*p).into(),
            schema(a, context, scope, files, vars, stack, response)?,
        );
    }
    let mut filtered = cls.clone();
    for m in &cls.methods {
        let getter = m.is_public
            && !m.is_static
            && m.params_text.trim().is_empty()
            && !m.return_type.is_empty()
            && m.return_type != "void";
        let property = if getter {
            m.name
                .strip_prefix("get")
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    if m.return_type == "boolean" {
                        m.name.strip_prefix("is").filter(|n| !n.is_empty())
                    } else {
                        None
                    }
                })
                .map(|name| {
                    if name.chars().nth(1).is_some_and(char::is_uppercase) {
                        name.to_string()
                    } else {
                        format!("{}{}", name[..1].to_lowercase(), &name[1..])
                    }
                })
        } else {
            None
        };
        for a in &m.anns {
            if a.name.starts_with("Json")
                && !matches!(
                    a.name.as_str(),
                    "JsonCreator" | "JsonIgnore" | "JsonProperty" | "JsonInclude"
                )
            {
                return Err(format!("类型 {base} 的方法使用暂不支持的 @{}", a.name));
            }
        }
        if property.is_none()
            && m.anns.iter().any(|a| {
                matches!(
                    a.name.as_str(),
                    "JsonProperty" | "JsonIgnore" | "JsonInclude"
                )
            })
        {
            return Err(format!(
                "类型 {base} 的非 getter 方法 {} 使用暂不支持的 JSON 属性注解",
                m.name
            ));
        }
        if let Some(name) = property {
            if let Some(field) = filtered.fields.iter_mut().find(|f| f.name == name) {
                field.anns.extend(m.anns.clone());
                if response {
                    field.java_type = m.return_type.clone();
                }
            } else if response {
                filtered.fields.push(java::FieldRaw {
                    name,
                    java_type: m.return_type.clone(),
                    anns: m.anns.clone(),
                    javadoc: m.javadoc.clone(),
                    is_static: false,
                });
            }
        }
        if let Some(creator) = m.anns.iter().find(|a| a.name == "JsonCreator") {
            if java::ann_attr_raw(creator, "mode")
                .is_some_and(|v| !v.ends_with("PROPERTIES") && !v.ends_with("DEFAULT"))
            {
                return Err(format!("类型 {base} 的 JsonCreator 模式暂不支持"));
            }
            if !response {
                for param in java::split_args(&m.params_text) {
                    let chars: Vec<_> = param.chars().collect();
                    let annotations =
                        java::collect_annotations_range(&chars, &chars, 0, chars.len());
                    let property = annotations
                        .iter()
                        .find(|a| a.name == "JsonProperty")
                        .ok_or_else(|| {
                            format!("类型 {base} 的 JsonCreator 参数缺少 JsonProperty 名称")
                        })?;
                    let name = java::ann_string(property, "value")
                        .or_else(|| java::first_string(&property.args))
                        .ok_or("JsonCreator 参数名称无法解析")?;
                    let field = filtered
                        .fields
                        .iter_mut()
                        .find(|f| {
                            f.name == name
                                || f.anns.iter().any(|a| {
                                    a.name == "JsonProperty"
                                        && java::first_string(&a.args).as_deref() == Some(&name)
                                })
                        })
                        .ok_or_else(|| format!("JsonCreator 参数 {name} 没有对应字段"))?;
                    field.anns.push(property.clone());
                }
            }
        }
    }
    for a in &cls.anns {
        if a.name.starts_with("Json") && a.name != "JsonInclude" {
            return Err(format!("类型 {base} 使用暂不支持的 @{}", a.name));
        }
    }
    let fields = std::mem::take(&mut filtered.fields);
    for f in &fields {
        let mut ignored = false;
        for a in &f.anns {
            if a.name == "JsonIgnore" {
                let value = java::ann_attr_raw(a, "value").unwrap_or_else(|| a.args.trim().into());
                ignored = match value.trim() {
                    "" | "true" => true,
                    "false" => false,
                    _ => return Err(format!("字段 {base}.{} 的 JsonIgnore 值无法解析", f.name)),
                };
            } else if a.name == "JsonProperty" {
                if java::split_args(&a.args).iter().any(|part| {
                    part.contains('=')
                        && !matches!(part.split_once('=').unwrap().0.trim(), "value" | "required")
                }) {
                    return Err(format!(
                        "字段 {base}.{} 的 JsonProperty 属性暂不支持",
                        f.name
                    ));
                }
                let raw = java::ann_attr_raw(a, "value").unwrap_or_else(|| a.args.clone());
                if !raw.trim().is_empty() && !literal(&raw) {
                    return Err(format!("字段 {base}.{} 的 JSON 名称不是字面量", f.name));
                }
                if java::ann_attr_raw(a, "required")
                    .is_some_and(|v| !matches!(v.trim(), "true" | "false"))
                {
                    return Err(format!("字段 {base}.{} 的 required 值无法解析", f.name));
                }
            } else if a.name == "JsonInclude" {
                let value = java::ann_attr_raw(a, "value").unwrap_or_else(|| a.args.clone());
                if !matches!(
                    value.trim().rsplit('.').next(),
                    Some("ALWAYS" | "NON_NULL" | "NON_EMPTY" | "NON_DEFAULT" | "USE_DEFAULTS")
                ) {
                    return Err(format!("字段 {base}.{} 的 JsonInclude 暂不支持", f.name));
                }
            } else if a.name.starts_with("Json") {
                return Err(format!("字段 {base}.{} 的 @{} 暂不支持", f.name, a.name));
            }
        }
        if !ignored {
            filtered.fields.push(f.clone());
        }
    }
    let doc = spring::schema_for_class(&filtered);
    if doc.fields.is_empty() {
        return Err(format!("类型 {base} 没有可解析的字段"));
    }
    stack.push(key);
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for f in doc.fields {
        let mut field = schema(
            &f.java_type,
            file,
            &cls.qualified_name,
            files,
            &bindings,
            stack,
            response,
        )
        .map_err(|error| format!("字段 {base}.{}：{error}", f.name))?;
        if !f.desc.is_empty() {
            field["description"] = json!(f.desc);
        }
        let creator_required = !response
            && filtered.fields.iter().any(|raw| {
                let name = raw
                    .anns
                    .iter()
                    .find(|a| a.name == "JsonProperty")
                    .and_then(|a| java::first_string(&a.args))
                    .unwrap_or_else(|| raw.name.clone());
                name == f.name
                    && raw.anns.iter().any(|a| {
                        a.name == "JsonProperty"
                            && java::ann_attr_raw(a, "required").as_deref() == Some("true")
                    })
            });
        if f.required || creator_required {
            required.push(f.name.clone());
        }
        if properties.insert(f.name.clone(), field).is_some() {
            return Err(format!("类型 {base} 字段 {} 重复", f.name));
        }
    }
    stack.pop();
    let mut result = json!({"type":"object","properties":properties,"required":required});
    if !doc.desc.is_empty() {
        result["description"] = json!(doc.desc);
    }
    Ok(result)
}

fn literal(raw: &str) -> bool {
    let raw = raw.trim();
    raw.len() >= 2
        && raw.starts_with('"')
        && raw.ends_with('"')
        && !raw[1..raw.len() - 1].contains(['"', '\\', '\n', '\r'])
}
fn validate_params(raw: &str) -> Result<(), String> {
    for (name, args) in spring::collect_param_anns(raw) {
        if !matches!(
            name.as_str(),
            "RequestParam" | "RequestHeader" | "PathVariable"
        ) {
            continue;
        }
        let a = Ann { name, args };
        for key in ["value", "name", "defaultValue"] {
            if let Some(raw) = java::ann_attr_raw(&a, key)
                && !literal(&raw)
            {
                return Err(format!("@{} 的 {key} 不是可解析的字面量", a.name));
            }
        }
        if !a.args.contains('=') && !a.args.trim().is_empty() && !literal(&a.args) {
            return Err(format!("@{} 的参数名称不是字面量", a.name));
        }
        if let Some(raw) = java::ann_attr_raw(&a, "required")
            && !matches!(raw.trim(), "true" | "false")
        {
            return Err(format!("@{} 的 required 值无法解析", a.name));
        }
    }
    Ok(())
}

fn mappings(anns: &[Ann]) -> Result<Vec<String>, String> {
    if let Some(a) = anns.iter().find(|a| a.name.ends_with("Mapping")) {
        for attr in ["params", "headers", "consumes", "produces"] {
            if java::ann_attr_raw(a, attr).is_some() {
                return Err(format!(
                    "@{} 的 {attr} 条件暂不支持，请先补充解析支持",
                    a.name
                ));
            }
        }
        let raw = java::ann_attr_raw(a, "path")
            .or_else(|| java::ann_attr_raw(a, "value"))
            .unwrap_or_else(|| {
                if a.args.contains('=') {
                    String::new()
                } else {
                    a.args.clone()
                }
            });
        if raw.trim().is_empty() {
            return Ok(vec![String::new()]);
        }
        let raw = raw.trim().trim_start_matches('{').trim_end_matches('}');
        let mut paths = Vec::new();
        for p in java::split_args(raw) {
            let p = p.trim();
            if !(p.starts_with('"') && p.ends_with('"'))
                || p[1..p.len() - 1].contains(['"', '\\', '$', '*', ':'])
            {
                return Err(format!("不支持动态或复杂映射：{p}"));
            }
            paths.push(p[1..p.len() - 1].into());
        }
        return Ok(paths);
    }
    Ok(vec![String::new()])
}

pub fn analyze(input: &[(String, String)], selected: &[String]) -> Result<Vec<Api>, String> {
    analyze_with_resolver(input, selected, &mut |_| Ok(Vec::new()))
}

pub fn analyze_with_resolver(
    input: &[(String, String)],
    selected: &[String],
    resolver: &mut impl FnMut(&[String]) -> Result<Vec<(String, String)>, String>,
) -> Result<Vec<Api>, String> {
    let mut input = input.to_vec();
    let mut added = BTreeSet::new();
    for _ in 0..64 {
        let files = Sources {
            files: input.iter().map(|(_, s)| source(s)).collect(),
            missing: Default::default(),
        };
        let result = analyze_sources(&input, selected, &files);
        let missing = files.missing.into_inner();
        if result.is_ok() || missing.is_empty() {
            return result;
        }
        let external = resolver(&missing)
            .map_err(|error| format!("{}\n依赖解析：{error}", result.as_ref().unwrap_err()))?;
        if external.is_empty() {
            return result;
        }
        for (path, text) in external {
            if !added.insert(path.clone()) {
                return Err("依赖类型解析没有进展，请核对依赖版本和类型名".into());
            }
            input.push((path, text));
        }
    }
    Err("类型引用超过 64 次，请缩小同步范围".into())
}

fn analyze_sources(
    input: &[(String, String)],
    selected: &[String],
    files: &Sources,
) -> Result<Vec<Api>, String> {
    let mut out = Vec::new();
    let mut identities = BTreeSet::new();
    for ((path, _), file) in input
        .iter()
        .zip(files.iter())
        .filter(|((p, _), _)| selected.contains(p))
    {
        for cls in file.classes.iter().filter(|c| spring::is_controller(c)) {
            if cls
                .declaration
                .split_whitespace()
                .any(|w| w == "extends" || w == "implements")
            {
                return Err(format!("{path}：Controller 继承映射暂不支持"));
            }
            let bases = mappings(&cls.anns)?;
            for m in &cls.methods {
                if !m.anns.iter().any(|a| a.name.ends_with("Mapping")) {
                    continue;
                }
                if m.params_text.contains("@RequestPart") || m.params_text.contains("@CookieValue")
                {
                    return Err(format!("{}：暂不支持文件上传或 Cookie 参数", m.name));
                }
                validate_params(&m.params_text)?;
                let paths = mappings(&m.anns)?;
                let endpoints = spring::parse_endpoints(cls, m);
                if endpoints.is_empty() {
                    return Err(format!("{}：请明确声明 HTTP 方法", m.name));
                }
                for ep in endpoints {
                    let routes: Vec<_> = bases
                        .iter()
                        .flat_map(|base| {
                            paths.iter().map(move |sub| {
                                let path = format!(
                                    "/{}/{}",
                                    base.trim_matches('/'),
                                    sub.trim_matches('/')
                                )
                                .replace("//", "/");
                                if path.len() > 1 {
                                    path.trim_end_matches('/').to_string()
                                } else {
                                    path
                                }
                            })
                        })
                        .collect();
                    let context = format!(
                        "{}.{} · {} {}",
                        cls.name,
                        m.name,
                        ep.method,
                        routes.join(", ")
                    );
                    let request_schema = ep
                        .request_body
                        .as_ref()
                        .map(|t| {
                            schema(
                                t,
                                file,
                                &cls.qualified_name,
                                files,
                                &BTreeMap::new(),
                                &mut vec![],
                                false,
                            )
                        })
                        .transpose()
                        .map_err(|error| format!("{context}\n请求体：{error}"))?;
                    let response_schema = schema(
                        &m.return_type,
                        file,
                        &cls.qualified_name,
                        files,
                        &BTreeMap::new(),
                        &mut vec![],
                        true,
                    )
                    .map_err(|error| format!("{context}\n响应体：{error}"))?;
                    let mut api = Api {
                        method: ep.method,
                        path: String::new(),
                        title: ep.summary,
                        controller: if ep.controller_desc.is_empty() {
                            cls.name.clone()
                        } else {
                            ep.controller_desc
                        },
                        description: ep.description,
                        query: vec![],
                        path_params: vec![],
                        headers: vec![],
                        request_schema,
                        response_schema,
                    };
                    for p in ep.params {
                        if p.location == "object" {
                            if matches!(
                                p.java_type.rsplit('.').next().unwrap_or(""),
                                "HttpServletRequest"
                                    | "HttpServletResponse"
                                    | "BindingResult"
                                    | "Principal"
                            ) {
                                continue;
                            }
                            let s = schema(
                                &p.java_type,
                                file,
                                &cls.qualified_name,
                                files,
                                &BTreeMap::new(),
                                &mut vec![],
                                false,
                            )?;
                            let props = s["properties"]
                                .as_object()
                                .ok_or("对象参数不是可展开 DTO")?;
                            for (name, f) in props {
                                if !matches!(
                                    f["type"].as_str(),
                                    Some("string" | "integer" | "number" | "boolean")
                                ) {
                                    return Err(format!("复杂查询参数 {name} 暂不支持"));
                                }
                                api.query.push(json!({"name":name,"desc":f["description"].as_str().unwrap_or(""),"required":if s["required"].as_array().is_some_and(|a|a.contains(&json!(name))){"1"}else{"0"}}));
                            }
                            continue;
                        }
                        let shape = schema(
                            &p.java_type,
                            file,
                            &cls.qualified_name,
                            files,
                            &BTreeMap::new(),
                            &mut vec![],
                            false,
                        )?;
                        if shape["type"] == "object"
                            || (shape["type"] == "array"
                                && matches!(
                                    shape["items"]["type"].as_str(),
                                    Some("object" | "array")
                                ))
                        {
                            return Err(format!("参数 {} 不是可解析的简单类型", p.name));
                        }
                        let v = json!({"name":p.name,"desc":p.desc,"required":if p.required {"1"}else{"0"},"example":p.default.unwrap_or_default()});
                        match p.location.as_str() {
                            "query" => api.query.push(v),
                            "path" => api.path_params.push(v),
                            "header" => api.headers.push(v),
                            _ => return Err("无法识别参数类型".into()),
                        }
                    }
                    for path in routes {
                        let mut item = api.clone();
                        item.path = path;
                        if !identities.insert((item.method.clone(), item.path.clone())) {
                            return Err(format!("存在重复映射 {} {}", item.method, item.path));
                        }
                        out.push(item);
                    }
                }
            }
        }
    }
    if out.is_empty() {
        return Err("所选 Java 文件中没有可同步的 Spring Controller 接口".into());
    }
    Ok(out)
}
