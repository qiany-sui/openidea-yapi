use openidea_plugin::{ProcessOutput, mock::MockHost};
use openidea_yapi::{transport::Client, yapi::Transport};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::{Command, Stdio},
};

#[test]
fn real_curl_sends_get_token_and_json_body_over_http() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("本机 HTTP 协议测试需要回环监听");
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for n in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut data = Vec::new();
            let boundary = loop {
                let mut buf = [0u8; 1024];
                let count = socket.read(&mut buf).unwrap();
                assert!(count > 0);
                data.extend_from_slice(&buf[..count]);
                if let Some(i) = data.windows(4).position(|s| s == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let headers = String::from_utf8(data[..boundary].to_vec()).unwrap();
            if n == 0 {
                assert!(headers.starts_with("GET /api/project/get?token=fixture-token "));
            } else {
                assert!(headers.starts_with("POST /api/interface/add "));
                let length = headers
                    .lines()
                    .find_map(|s| {
                        s.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while data.len() - boundary < length {
                    let mut buf = [0u8; 1024];
                    let count = socket.read(&mut buf).unwrap();
                    assert!(count > 0);
                    data.extend_from_slice(&buf[..count]);
                }
                let body: serde_json::Value =
                    serde_json::from_slice(&data[boundary..boundary + length]).unwrap();
                assert_eq!(body["token"], "fixture-token");
                assert_eq!(body["title"], "中文 \"标题\"\n下一行");
            }
            let body = json!({"errcode":0,"data":{"_id":1}}).to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
        }
    });
    let h = MockHost::install().with_capabilities(&["process:curl"]);
    h.on_process("curl", |args, stdin| {
        let mut child = Command::new("curl")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.unwrap().as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        Ok(ProcessOutput {
            status: out.status.code(),
            stdout: String::from_utf8(out.stdout).unwrap(),
            stderr: String::from_utf8(out.stderr).unwrap(),
        })
    });
    let mut c = Client::new(&format!("http://{address}"), "fixture-token").unwrap();
    c.call("GET", "/api/project/get", json!({})).unwrap();
    c.call(
        "POST",
        "/api/interface/add",
        json!({"title":"中文 \"标题\"\n下一行"}),
    )
    .unwrap();
    server.join().unwrap();
}
