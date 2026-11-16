use crate::yapi::Transport;
use openidea_plugin::host::{self, Command};
use serde_json::{Value, json};
use url::Url;
use zeroize::Zeroizing;

pub fn normalize_base(raw: &str) -> Result<String, String> {
    if raw.chars().any(char::is_control) {
        return Err("地址不能包含控制字符".into());
    }
    let url = Url::parse(raw.trim()).map_err(|_| "请输入完整的 HTTP(S) YApi 地址")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("YApi 地址仅允许 HTTP(S)，不要包含账号、query 或 # 路径".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}
pub fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > 4096
        || token.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("请输入有效的 YApi 项目 token（不含空白，最长 4096 字节）".into());
    }
    Ok(())
}
fn quoted(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

pub struct Client {
    base: String,
    token: Zeroizing<String>,
    task: host::Task,
}
impl Client {
    pub fn new(base: &str, token: &str) -> Result<Self, String> {
        validate_token(token)?;
        Ok(Self {
            base: normalize_base(base)?,
            token: Zeroizing::new(token.into()),
            task: host::begin_task("YApi：读取或同步接口", true),
        })
    }
}
impl Transport for Client {
    fn call(&mut self, method: &str, path: &str, mut body: Value) -> Result<Value, String> {
        if self.task.is_cancelled() {
            return Err("已取消 YApi 操作".into());
        }
        self.task.report(0, None, Some(path));
        // 固定协议路由，视图不能传任意 curl 参数或目标路径。
        if !matches!(
            (method, path),
            (
                "GET",
                "/api/project/get" | "/api/interface/list_menu" | "/api/interface/get"
            ) | (
                "POST",
                "/api/interface/add" | "/api/interface/up" | "/api/interface/add_cat"
            )
        ) {
            return Err("不支持的 YApi 操作".into());
        }
        let mut url = Url::parse(&format!("{}{path}", self.base)).map_err(|_| "无效地址")?;
        let mut config = Zeroizing::new(String::from(
            "silent\nshow-error\nconnect-timeout = 5\nmax-time = 20\nmax-filesize = 4194304\nproto = \"=http,https\"\nwrite-out = \"\\nOI_YAPI_STATUS:%{http_code}\"\n",
        ));
        if method == "GET" {
            let mut query = url.query_pairs_mut();
            query.append_pair("token", &self.token);
            for (k, v) in body.as_object().ok_or("无效请求")? {
                query.append_pair(
                    k,
                    &v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string()),
                );
            }
        } else {
            body["token"] = json!(&*self.token);
            config.push_str("request = \"POST\"\nheader = \"Content-Type: application/json\"\n");
            config.push_str(&format!("data-binary = {}\n", quoted(&body.to_string())));
        }
        config.push_str(&format!("url = {}\n", quoted(url.as_str())));
        let output = host::run_command(
            &Command::new("curl").args(["-q", "--config", "-"]),
            None,
            Some(&config),
        )
        .map_err(|_| {
            "无法运行 curl；请在插件权限页正常授权 process:curl，并核对可执行文件路径".to_string()
        })?;
        if output.status != Some(0) {
            return Err(format!(
                "curl 请求失败（退出码 {}），请检查地址、网络与 TLS 证书；未自动重试",
                output
                    .status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "signal".into())
            ));
        }
        let (text, status) = output
            .stdout
            .rsplit_once("\nOI_YAPI_STATUS:")
            .ok_or("curl 未返回 HTTP 状态")?;
        let status = status.trim().parse::<u16>().map_err(|_| "无效 HTTP 状态")?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "YApi 返回 HTTP {status}；不自动跟随重定向，请核对最终服务地址"
            ));
        }
        if text.len() > 4 * 1024 * 1024 {
            return Err("YApi 响应超过 4 MiB".into());
        }
        let response: Value = serde_json::from_str(text)
            .map_err(|_| "YApi 返回非 JSON 数据，请检查地址或登录代理")?;
        let code = response["errcode"]
            .as_i64()
            .ok_or("YApi 响应缺少 errcode")?;
        // 服务端错误正文可能回显 token，不传播原始正文和 stderr。
        if code != 0 {
            return Err(format!(
                "YApi 操作失败（errcode={code}），请检查 token、项目权限和接口配置"
            ));
        }
        response
            .get("data")
            .cloned()
            .ok_or_else(|| "YApi 响应缺少 data".into())
    }
}
