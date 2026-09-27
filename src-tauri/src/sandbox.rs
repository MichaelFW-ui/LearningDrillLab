use crate::app::ApiSettings;
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use uuid::Uuid;

const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DISPLAY_CHARS: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExperimentRun {
    pub id: Uuid,
    pub exercise_id: Uuid,
    pub code: String,
    pub lang: String,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
    pub session_id: Option<String>,
    pub files: Vec<Value>,
    pub elapsed_ms: u128,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ExecResponse {
    session_id: Option<String>,
    #[serde(default)]
    stdout: String,
    #[serde(default)]
    stderr: String,
    #[serde(default)]
    files: Vec<Value>,
    error: Option<Value>,
    error_type: Option<String>,
}

fn normalized_language(language: &str) -> Option<&'static str> {
    match language.trim().to_ascii_lowercase().as_str() {
        "python" | "py" => Some("py"),
        "javascript" | "js" => Some("js"),
        "typescript" | "ts" => Some("ts"),
        "go" => Some("go"),
        "java" => Some("java"),
        "c" => Some("c"),
        "c++" | "cpp" => Some("cpp"),
        "php" => Some("php"),
        "rust" | "rs" => Some("rs"),
        "r" => Some("r"),
        "fortran" | "f90" => Some("f90"),
        "d" => Some("d"),
        _ => None,
    }
}

pub fn validate_base_url(base: &str) -> Result<(), String> {
    let url = Url::parse(base).map_err(|_| "沙箱地址格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("沙箱地址必须使用 HTTP 或 HTTPS".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("沙箱地址请只填写服务地址；API Key 请单独填写在沙箱 API Key 栏".to_string());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("沙箱地址不能包含查询参数或片段".to_string());
    }
    Ok(())
}

fn truncate(value: String) -> String {
    if value.chars().count() <= MAX_DISPLAY_CHARS {
        return value;
    }
    format!(
        "{}\n[输出已截断]",
        value.chars().take(MAX_DISPLAY_CHARS).collect::<String>()
    )
}

pub async fn execute(
    settings: &ApiSettings,
    exercise_id: Uuid,
    code: String,
    language: String,
    session_id: Option<String>,
) -> Result<ExperimentRun, String> {
    if code.trim().is_empty() || code.len() > MAX_CODE_BYTES {
        return Err("实验代码不能为空，且不能超过 64 KiB".to_string());
    }
    let lang =
        normalized_language(&language).ok_or_else(|| format!("沙箱不支持语言：{language}"))?;
    let base = settings.sandbox_base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("请先在设置页填写 LibreCodeInterpreter 地址".to_string());
    }
    validate_base_url(base)?;
    let url = Url::parse(&format!("{base}/exec")).map_err(|_| "沙箱地址格式无效".to_string())?;
    let http = Client::builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|e| format!("沙箱客户端初始化失败：{e}"))?;
    let mut request = http.post(url).json(&json!({
        "code": code,
        "lang": lang,
        "session_id": session_id,
        "timeout": 30000
    }));
    if !settings.sandbox_api_key.trim().is_empty() {
        request = request.header("x-api-key", settings.sandbox_api_key.trim());
    }
    let started = Instant::now();
    let mut response = request
        .send()
        .await
        .map_err(|e| format!("沙箱请求失败：{e}"))?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("读取沙箱响应失败：{e}"))?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err("沙箱响应超过 1 MiB".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = String::from_utf8_lossy(&bytes);
    if !status.is_success() {
        return Err(format!(
            "沙箱返回 HTTP {status}：{}",
            truncate(body.to_string())
        ));
    }
    let result: ExecResponse =
        serde_json::from_slice(&bytes).map_err(|e| format!("沙箱响应格式错误：{e}"))?;
    let (run_status, stderr) = match result.error {
        Some(error) => (
            result.error_type.unwrap_or_else(|| "error".to_string()),
            format!("{}\n{}", result.stderr, error).trim().to_string(),
        ),
        None if !result.stderr.trim().is_empty() => ("stderr".to_string(), result.stderr),
        None => ("completed".to_string(), String::new()),
    };
    Ok(ExperimentRun {
        id: Uuid::new_v4(),
        exercise_id,
        code,
        lang: lang.to_string(),
        status: run_status,
        stdout: truncate(result.stdout),
        stderr: truncate(stderr),
        session_id: result.session_id,
        files: result.files,
        elapsed_ms: started.elapsed().as_millis(),
        created_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn aliases_and_limits() {
        assert_eq!(normalized_language("Rust"), Some("rs"));
        assert_eq!(normalized_language("C++"), Some("cpp"));
        assert_eq!(normalized_language("sh"), None);
        assert_eq!(normalized_language("unknown"), None);
        assert!(truncate("a".repeat(20_001)).ends_with("[输出已截断]"));
    }

    #[test]
    fn sandbox_url_keeps_credentials_out_of_url() {
        assert!(validate_base_url("https://code.example:8443").is_ok());
        assert!(validate_base_url("https://secret@code.example:8443").is_err());
        assert!(validate_base_url("https://code.example:8443?key=secret").is_err());
    }

    #[test]
    fn response_accepts_librechat_fields_and_stream_whitespace() {
        let value: ExecResponse = serde_json::from_str(
            "  {\"session_id\":\"s1\",\"stdout\":\"42\\n\",\"stderr\":\"\",\"files\":[]}",
        )
        .unwrap();
        assert_eq!(value.session_id.as_deref(), Some("s1"));
        assert_eq!(value.stdout, "42\n");
    }

    #[test]
    fn sends_librechat_request_and_reads_observation() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = socket.read(&mut buffer).unwrap();
                received.extend_from_slice(&buffer[..count]);
                if let Some(header_end) = received.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header =
                        String::from_utf8_lossy(&received[..header_end]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    if received.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let request = String::from_utf8_lossy(&received);
            assert!(request.starts_with("POST /exec HTTP/1.1"));
            assert!(request.to_ascii_lowercase().contains("x-api-key: test-key"));
            assert!(request.contains("\"lang\":\"py\""));
            assert!(request.contains("\"code\":\"print(42)\""));
            let body = "  {\"session_id\":\"session-1\",\"stdout\":\"42\\n\",\"stderr\":\"\",\"files\":[]}";
            write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let mut settings = ApiSettings::default();
        settings.sandbox_base_url = format!("http://{address}");
        settings.sandbox_api_key = "test-key".to_string();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime
            .block_on(execute(
                &settings,
                Uuid::new_v4(),
                "print(42)".to_string(),
                "Python".to_string(),
                None,
            ))
            .unwrap();
        server.join().unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.stdout, "42\n");
        assert_eq!(result.session_id.as_deref(), Some("session-1"));
    }

    #[test]
    fn auth_failure_is_reported_without_key() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).unwrap();
            let body = "{\"error\":\"Invalid API key\"}";
            write!(socket, "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let mut settings = ApiSettings::default();
        settings.sandbox_base_url = format!("http://{address}");
        settings.sandbox_api_key = "secret-test-key".to_string();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(execute(
                &settings,
                Uuid::new_v4(),
                "print(42)".to_string(),
                "py".to_string(),
                None,
            ))
            .unwrap_err();
        server.join().unwrap();
        assert!(error.contains("401"));
        assert!(!error.contains("secret-test-key"));
    }
}
