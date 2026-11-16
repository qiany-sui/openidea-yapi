use crate::parse::Api;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub trait Transport {
    fn call(&mut self, method: &str, path: &str, body: Value) -> Result<Value, String>;
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Item {
    pub action: String,
    pub method: String,
    pub path: String,
    pub title: String,
    pub category: String,
    pub payload: Value,
    pub before: Option<Value>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub project_id: i64,
    pub project_name: String,
    pub items: Vec<Item>,
    #[serde(skip)]
    pub apis: Vec<Api>,
    #[serde(skip)]
    categories: BTreeMap<String, i64>,
}

fn id(value: &Value) -> Result<i64, String> {
    value
        .as_i64()
        .filter(|i| *i > 0)
        .ok_or_else(|| "YApi 返回了无效 ID".into())
}
fn list(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| "YApi 返回的数据结构不符合标准接口".into())
}
fn key(v: &Value) -> Result<(String, String), String> {
    let method = v["method"].as_str().ok_or("YApi 接口缺少 method")?;
    let path = v["path"].as_str().ok_or("YApi 接口缺少 path")?;
    Ok((method.to_uppercase(), path.into()))
}

// 保留示例、mock 和没有源码说明的描述，移除已不再出现在源码中的字段。
fn schema_merge(new: &Value, old: &Value) -> Value {
    if !new.is_object() {
        return new.clone();
    }
    let mut out = old.as_object().cloned().unwrap_or_default();
    out.retain(|k, _| {
        matches!(
            k.as_str(),
            "description" | "title" | "example" | "examples" | "default" | "mock"
        )
    });
    if old["type"] != new["type"] {
        out.clear();
    }
    for (k, v) in new.as_object().unwrap() {
        out.insert(
            k.clone(),
            if k == "properties" {
                Value::Object(
                    v.as_object()
                        .unwrap()
                        .iter()
                        .map(|(name, field)| (name.clone(), schema_merge(field, &old[k][name])))
                        .collect(),
                )
            } else if k == "items" || k == "additionalProperties" {
                schema_merge(v, &old[k])
            } else {
                v.clone()
            },
        );
    }
    Value::Object(out)
}

fn params(new: &[Value], old: &Value) -> Value {
    json!(
        new.iter()
            .map(|p| {
                let previous = old
                    .as_array()
                    .and_then(|a| a.iter().find(|o| o["name"] == p["name"]));
                let mut v = previous
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for (k, value) in p.as_object().unwrap() {
                    if value == "" && v.contains_key(k) && matches!(k.as_str(), "desc" | "example")
                    {
                        continue;
                    }
                    v.insert(k.clone(), value.clone());
                }
                Value::Object(v)
            })
            .collect::<Vec<_>>()
    )
}
fn payload(api: &Api, old: Option<&Value>) -> Value {
    let empty = json!({});
    let old = old.unwrap_or(&empty);
    let mut headers = api.headers.clone();
    let mut path_params = api.path_params.clone();
    for param in &mut path_params {
        param.as_object_mut().unwrap().remove("required");
    }
    if api.request_schema.is_some()
        && !headers.iter().any(|h| {
            h["name"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("Content-Type"))
        })
    {
        headers.push(json!({"name":"Content-Type","value":"application/json","required":"1"}));
    }
    let mut p = json!({
        "title":api.title,"method":api.method,"path":api.path,
        "req_query":params(&api.query,&old["req_query"]),
        "req_params":params(&path_params,&old["req_params"]),
        "req_headers":params(&headers,&old["req_headers"]),
        "req_body_type":if api.request_schema.is_some(){"json"}else{"raw"},
        "req_body_form":[],
        "req_body_other":"",
        "req_body_is_json_schema":api.request_schema.is_some(),
        "res_body_type":"json","res_body_is_json_schema":true,
        "switch_notice":false
    });
    for (key, schema) in [
        ("req_body_other", api.request_schema.as_ref()),
        ("res_body", Some(&api.response_schema)),
    ] {
        if let Some(schema) = schema {
            let previous = old[key]
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or(Value::Null);
            p[key] = json!(schema_merge(schema, &previous).to_string());
        }
    }
    p
}

pub fn preview(t: &mut impl Transport, apis: &[Api]) -> Result<Plan, String> {
    if apis.is_empty() || apis.len() > 200 {
        return Err("每次请选择 1～200 个接口".into());
    }
    let project = t.call("GET", "/api/project/get", json!({}))?;
    let project_id = id(&project["_id"])?;
    let menu = t.call(
        "GET",
        "/api/interface/list_menu",
        json!({"project_id":project_id}),
    )?;
    let mut categories = BTreeMap::new();
    let mut remote = BTreeMap::new();
    for cat in list(&menu)? {
        let cid = id(&cat["_id"])?;
        let name = cat["name"].as_str().ok_or("YApi 分类缺少名称")?;
        if categories.insert(name.to_string(), cid).is_some() {
            return Err(format!("YApi 有同名分类 {name}，请先整理"));
        }
        for api in list(&cat["list"])? {
            if remote.insert(key(api)?, id(&api["_id"])?).is_some() {
                return Err("YApi 有重复的 method/path，无法安全同步".into());
            }
        }
    }
    let mut items = Vec::new();
    let mut identities = BTreeSet::new();
    for api in apis {
        if !identities.insert((&api.method, &api.path)) {
            return Err("所选源码有重复接口".into());
        }
        let before = remote
            .get(&(api.method.clone(), api.path.clone()))
            .map(|id| t.call("GET", "/api/interface/get", json!({"id":id})))
            .transpose()?;
        if let Some(old) = &before
            && (key(old)? != (api.method.clone(), api.path.clone())
                || id(&old["project_id"])? != project_id)
        {
            return Err("YApi 接口归属发生变化，请重新预览".into());
        }
        let mut p = payload(api, before.as_ref());
        let action = if let Some(old) = &before {
            let changed = p
                .as_object()
                .unwrap()
                .iter()
                .filter(|(k, _)| k.as_str() != "switch_notice")
                .any(|(k, v)| old.get(k) != Some(v));
            p["id"] = json!(id(&old["_id"])?);
            if changed { "更新" } else { "未变化" }
        } else {
            p["project_id"] = json!(project_id);
            p["status"] = json!("undone");
            p["markdown"] = json!(api.description);
            p["desc"] = json!("");
            "新增"
        };
        items.push(Item {
            action: action.into(),
            method: api.method.clone(),
            path: api.path.clone(),
            title: api.title.clone(),
            category: api.controller.clone(),
            payload: p,
            before,
        });
    }
    Ok(Plan {
        project_id,
        project_name: project["name"].as_str().unwrap_or("").into(),
        items,
        apis: apis.to_vec(),
        categories,
    })
}

pub fn execute(t: &mut impl Transport, plan: &Plan, selected: &[usize]) -> Result<String, String> {
    if selected.is_empty() {
        return Ok("未选择接口，没有写入".into());
    }
    if selected.iter().any(|i| *i >= plan.items.len())
        || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
    {
        return Err("选择无效，请重新预览".into());
    }
    let fresh = preview(t, &plan.apis)?;
    if fresh.project_id != plan.project_id
        || fresh.categories != plan.categories
        || fresh.items != plan.items
    {
        return Err("预览后远端发生变化，未写入。请重新预览".into());
    }
    let mut categories = plan.categories.clone();
    let mut added = 0;
    let mut updated = 0;
    let mut skipped = 0;
    for i in selected {
        let item = &plan.items[*i];
        if item.action == "未变化" {
            skipped += 1;
            continue;
        }
        let result = (|| {
            let mut payload = item.payload.clone();
            let route = if item.before.is_some() {
                "/api/interface/up"
            } else {
                let cid = if let Some(id) = categories.get(&item.category) {
                    *id
                } else {
                    let result = t.call(
                        "POST",
                        "/api/interface/add_cat",
                        json!({"project_id":plan.project_id,"name":item.category,"desc":""}),
                    )?;
                    let cid = id(&result["_id"])?;
                    categories.insert(item.category.clone(), cid);
                    cid
                };
                payload["catid"] = json!(cid);
                "/api/interface/add"
            };
            t.call("POST", route, payload)?;
            Ok::<_, String>(())
        })();
        if let Err(e) = result {
            return Err(format!(
                "已新增 {added}、更新 {updated}；在 {} {} 停止：{e}。本次结果可能包含服务端已接收但未返回的请求，请重新预览后重试",
                item.method, item.path
            ));
        }
        if item.before.is_some() {
            updated += 1;
        } else {
            added += 1;
        }
    }
    Ok(format!(
        "同步完成：新增 {added}，更新 {updated}，未变化 {skipped}"
    ))
}
