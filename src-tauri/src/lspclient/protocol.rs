//! LSP 와이어 프로토콜 — 프레이밍·URI·응답 파싱의 순수 함수 계층.
//!
//! 프로세스나 소켓을 알지 못한다. 여기 있는 함수들은 전부 입력→출력이 결정적이라
//! 서버 없이 테스트할 수 있고, `mod.rs`의 클라이언트가 이것들을 조립해 쓴다.

use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// LSP 응답이 가리키는 위치 한 개. 좌표는 LSP 원본 그대로(0-based, UTF-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawLocation {
    pub uri: String,
    pub line: u32,
    pub character: u32,
}

/// JSON-RPC 페이로드에 `Content-Length` 헤더를 붙인다.
/// 길이는 **바이트 수**다 — 한국어 등 멀티바이트가 섞이면 문자 수와 다르다.
pub fn encode_message(payload: &str) -> Vec<u8> {
    let mut out = format!("Content-Length: {}\r\n\r\n", payload.len()).into_bytes();
    out.extend_from_slice(payload.as_bytes());
    out
}

/// 헤더를 읽어 본문 한 건을 꺼낸다. 스트림이 정상 종료되면 `Ok(None)`.
///
/// `Content-Type` 등 다른 헤더는 무시하고 넘긴다. 헤더가 끝나는 빈 줄까지 읽은 뒤
/// 선언된 길이만큼 정확히 읽는다 — 서버는 한 번의 write에 여러 메시지를 담을 수 있으므로
/// 줄 단위로 추측해선 안 된다.
pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None); // EOF — 서버 종료
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // 헤더 끝
        }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            len = rest.trim().parse::<usize>().ok();
        }
    }
    let Some(len) = len else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "LSP 헤더에 Content-Length가 없습니다",
        ));
    };
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// `textDocument/definition` 계열 응답을 위치 목록으로 정규화한다.
///
/// 서버마다 반환 형태가 셋으로 갈린다 — 단일 `Location`, `Location[]`, `LocationLink[]`.
/// (rust-analyzer는 LocationLink, pyright는 Location[]을 준다.) 셋 다 받아준다.
/// LocationLink는 `targetSelectionRange`(심볼 이름 자체)를 우선한다 — 그쪽이 더 정확한
/// 커서 착지점이고, 없을 때만 `targetRange`(본문 전체)로 떨어진다.
pub fn parse_locations(value: &Value) -> Vec<RawLocation> {
    match value {
        Value::Array(items) => items.iter().filter_map(parse_one_location).collect(),
        Value::Object(_) => parse_one_location(value).into_iter().collect(),
        _ => Vec::new(), // null 포함 — 정의를 못 찾은 정상 응답
    }
}

fn parse_one_location(value: &Value) -> Option<RawLocation> {
    if let Some(uri) = value.get("uri").and_then(Value::as_str) {
        let start = value.get("range")?.get("start")?;
        return Some(RawLocation {
            uri: uri.to_string(),
            line: start.get("line")?.as_u64()? as u32,
            character: start.get("character")?.as_u64()? as u32,
        });
    }
    let uri = value.get("targetUri").and_then(Value::as_str)?;
    let range = value
        .get("targetSelectionRange")
        .or_else(|| value.get("targetRange"))?;
    let start = range.get("start")?;
    Some(RawLocation {
        uri: uri.to_string(),
        line: start.get("line")?.as_u64()? as u32,
        character: start.get("character")?.as_u64()? as u32,
    })
}

/// 파일 하나에서 뽑은 심볼. 좌표는 LSP 원본 그대로(0-based, UTF-16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSymbol {
    pub name: String,
    /// LSP `SymbolKind` 원본 숫자. 이름으로 바꾸지 않는다 — 서버마다 부르는 말이 다르고
    /// 숫자는 프로토콜이 고정한다.
    pub kind: u32,
    /// 한 칸 위 심볼의 이름. 최상위면 `None` — 경로가 아니라 직계 부모 하나다.
    pub container: Option<String>,
    /// 심볼 **이름**의 위치(`selectionRange`). 되짚어 열 때의 착지점이다.
    pub sel_line: u32,
    pub sel_char: u32,
    pub sel_end_line: u32,
    pub sel_end_char: u32,
    /// 심볼 본문 `range`의 시작·끝. 커서가 이름 밖 본문에 있을 때의 fallback이다.
    pub body_start_line: u32,
    pub body_start_char: u32,
    pub body_end_line: u32,
    pub body_end_char: u32,
    /// 심볼 **본문**의 끝 줄. 어떤 참조가 이 심볼 안에 들어 있는지 판정하는 데 쓴다.
    /// legacy `code_nodes` 호환 별칭이며 `body_end_line`과 같다.
    pub end_line: u32,
}

/// rust-analyzer의 `experimental/serverStatus` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerStatus {
    pub health: String,
    pub quiescent: bool,
    pub message: Option<String>,
}

impl ServerStatus {
    pub fn is_ready(&self) -> bool {
        self.health == "ok" && self.quiescent
    }
}

pub fn parse_server_status(value: &Value) -> Option<ServerStatus> {
    Some(ServerStatus {
        health: value.get("health")?.as_str()?.to_string(),
        quiescent: value.get("quiescent")?.as_bool()?,
        message: value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// jdtls의 `language/status` payload.
///
/// jdtls는 `ProjectStatus: OK` → `Started: "Ready"` → `ServiceReady` 순으로 보낸다.
/// **`type`만 판정 근거다** — `Started`의 `message`가 `"Ready"`인 것에 속으면 안 된다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageStatus {
    pub kind: String,
    pub message: Option<String>,
}

impl LanguageStatus {
    pub fn is_ready(&self) -> bool {
        self.kind == "ServiceReady"
    }
}

pub fn parse_language_status(value: &Value) -> Option<LanguageStatus> {
    Some(LanguageStatus {
        kind: value.get("type")?.as_str()?.to_string(),
        message: value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

/// `$/progress` 알림의 `value.kind` — `"begin"`·`"report"`·`"end"`.
///
/// 토큰 문자열은 보지 않는다. 서버 버전마다 문구가 바뀌므로 매칭하면 조용히 깨진다.
pub fn parse_progress_kind(value: &Value) -> Option<String> {
    Some(value.get("value")?.get("kind")?.as_str()?.to_string())
}

/// `textDocument/documentSymbol` 응답을 평탄한 심볼 목록으로 정규화한다.
///
/// 서버가 두 형태 중 하나를 준다 — 계층형 `DocumentSymbol[]`(children 중첩, rust-analyzer)과
/// 구형 평탄 `SymbolInformation[]`(`location` + `containerName`). 둘 다 받아 **전위 순회**로
/// 펼치므로 부모가 자식보다 항상 먼저 온다 — `contains` 엣지를 만들 때 부모가 이미 있다.
///
/// 좌표를 읽을 수 없는 항목은 **버린다.** 0으로 채우면 파일 첫 줄에 유령 노드가 생기고,
/// 나중에 그 좌표로 되짚을 때 엉뚱한 곳을 연다. 자식은 부모와 독립으로 판정한다 —
/// 부모 하나가 망가졌다고 그 아래를 통째로 잃을 이유는 없다.
pub fn parse_document_symbols(value: &Value) -> Vec<RawSymbol> {
    let Value::Array(items) = value else {
        return Vec::new(); // null 포함 — 심볼이 없는 정상 응답
    };
    let mut out = Vec::new();
    for item in items {
        collect_symbol(item, None, &mut out);
    }
    out
}

fn collect_symbol(value: &Value, parent: Option<&str>, out: &mut Vec<RawSymbol>) {
    let name = value.get("name").and_then(Value::as_str);
    if let (Some(name), Some(symbol)) = (name, read_symbol(value, name_or(parent, value))) {
        out.push(symbol);
        // 자식의 container는 조부가 아니라 **이 심볼**이다.
        for child in value
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            collect_symbol(child, Some(name), out);
        }
    }
}

/// 부모가 있으면 그것을, 없으면 평탄형이 주는 `containerName`을 쓴다.
/// 평탄형에는 부모 관계를 알려주는 통로가 그것뿐이다.
fn name_or<'a>(parent: Option<&'a str>, value: &'a Value) -> Option<&'a str> {
    parent.or_else(|| value.get("containerName").and_then(Value::as_str))
}

fn read_symbol(value: &Value, container: Option<&str>) -> Option<RawSymbol> {
    // 계층형은 `range`를, 평탄형은 `location.range`를 준다.
    let body = value
        .get("range")
        .or_else(|| value.get("location").and_then(|loc| loc.get("range")))?;
    // 이름 위치는 `selectionRange`가 정확하다. 평탄형에는 없으므로 본문으로 떨어진다.
    let selection = value.get("selectionRange").unwrap_or(body);
    let sel_start = selection.get("start")?;
    let sel_end = selection.get("end")?;
    let body_start = body.get("start")?;
    let body_end = body.get("end")?;
    let body_end_line = body_end.get("line")?.as_u64()? as u32;
    Some(RawSymbol {
        name: value.get("name")?.as_str()?.to_string(),
        kind: value.get("kind").and_then(Value::as_u64).unwrap_or(0) as u32,
        container: container.map(str::to_string),
        sel_line: sel_start.get("line")?.as_u64()? as u32,
        sel_char: sel_start.get("character")?.as_u64()? as u32,
        sel_end_line: sel_end.get("line")?.as_u64()? as u32,
        sel_end_char: sel_end.get("character")?.as_u64()? as u32,
        body_start_line: body_start.get("line")?.as_u64()? as u32,
        body_start_char: body_start.get("character")?.as_u64()? as u32,
        body_end_line,
        body_end_char: body_end.get("character")?.as_u64()? as u32,
        end_line: body_end_line,
    })
}

/// 로컬 절대 경로 → `file://` URI. 경로 구분자를 뺀 비예약 문자만 통과시키고
/// 나머지는 percent-encode 한다 (공백·한글 경로 대응).
pub fn path_to_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `file://` URI → 로컬 경로. 스킴이 다르면(예: `untitled:`, `jar:`) `None`.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // `file://localhost/...` 형태의 authority는 쓰지 않지만, 있으면 잘라낸다.
    let rest = match rest.find('/') {
        Some(0) => rest,
        Some(idx) => &rest[idx..],
        None => return None,
    };
    let bytes = rest.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::BufReader;

    #[test]
    fn content_length_counts_bytes_not_chars() {
        let encoded = encode_message("{\"k\":\"한글\"}");
        let text = String::from_utf8(encoded).unwrap();
        // 8글자 + "한글" 6바이트 = 14. 문자 수(10)로 세면 서버가 본문을 잘라 읽는다.
        assert!(text.starts_with("Content-Length: 14\r\n\r\n"), "{text}");
    }

    #[test]
    fn reads_two_messages_from_one_buffer() {
        let mut raw = encode_message("{\"id\":1}");
        raw.extend(encode_message("{\"id\":2}"));
        let mut reader = BufReader::new(&raw[..]);

        let first = read_message(&mut reader).unwrap().unwrap();
        let second = read_message(&mut reader).unwrap().unwrap();

        assert_eq!(first, b"{\"id\":1}");
        assert_eq!(second, b"{\"id\":2}");
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn skips_unknown_headers() {
        let raw =
            b"Content-Length: 8\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{\"id\":1}";
        let mut reader = BufReader::new(&raw[..]);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), b"{\"id\":1}");
    }

    #[test]
    fn eof_before_any_header_is_clean_end() {
        let mut reader = BufReader::new(&b""[..]);
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    #[test]
    fn parses_single_location_object() {
        let value = json!({
            "uri": "file:///w/src/main.rs",
            "range": { "start": { "line": 4, "character": 7 }, "end": { "line": 4, "character": 11 } }
        });
        assert_eq!(
            parse_locations(&value),
            vec![RawLocation {
                uri: "file:///w/src/main.rs".into(),
                line: 4,
                character: 7
            }]
        );
    }

    #[test]
    fn parses_location_link_preferring_selection_range() {
        // rust-analyzer 형태 — targetRange는 함수 본문 전체, targetSelectionRange는 이름.
        let value = json!([{
            "targetUri": "file:///w/src/lib.rs",
            "targetRange": { "start": { "line": 10, "character": 0 }, "end": { "line": 20, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 10, "character": 7 }, "end": { "line": 10, "character": 12 } }
        }]);
        assert_eq!(parse_locations(&value)[0].character, 7);
    }

    #[test]
    fn location_link_falls_back_to_target_range() {
        let value = json!([{
            "targetUri": "file:///w/a.py",
            "targetRange": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 9 } }
        }]);
        assert_eq!(parse_locations(&value)[0].line, 3);
    }

    #[test]
    fn null_response_yields_no_locations() {
        assert!(parse_locations(&Value::Null).is_empty());
        assert!(parse_locations(&json!([])).is_empty());
    }

    #[test]
    fn uri_round_trips_through_spaces_and_hangul() {
        let path = PathBuf::from("/Users/u/내 작업/src/main.rs");
        let uri = path_to_uri(&path);
        assert!(!uri.contains(' '), "{uri}");
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }

    #[test]
    fn non_file_scheme_is_rejected() {
        assert_eq!(uri_to_path("untitled:Untitled-1"), None);
        assert_eq!(uri_to_path("jar:file:///a.jar!/B.class"), None);
    }

    #[test]
    fn parses_hierarchical_document_symbols() {
        // rust-analyzer는 계층형 DocumentSymbol을 준다 (children 중첩).
        let raw = json!([{
            "name": "GoalContract",
            "kind": 23,
            "range": {"start": {"line": 22, "character": 0}, "end": {"line": 36, "character": 1}},
            "selectionRange": {"start": {"line": 22, "character": 11}, "end": {"line": 22, "character": 23}},
            "children": [{
                "name": "validate",
                "kind": 6,
                "range": {"start": {"line": 39, "character": 4}, "end": {"line": 60, "character": 5}},
                "selectionRange": {"start": {"line": 39, "character": 11}, "end": {"line": 39, "character": 19}},
                "children": []
            }]
        }]);
        let symbols = parse_document_symbols(&raw);
        assert_eq!(symbols.len(), 2, "중첩 심볼을 평탄화한다");
        assert_eq!(symbols[1].name, "validate");
        assert_eq!(symbols[1].container, Some("GoalContract".to_string()));
        assert_eq!(
            symbols[1].sel_line, 39,
            "좌표는 LSP 원본(0-based)을 그대로 보관한다"
        );
        assert_eq!(
            symbols[1].end_line, 60,
            "무효화 범위 판정에 본문 끝이 필요하다"
        );
        assert_eq!(
            (symbols[1].body_start_line, symbols[1].body_start_char),
            (39, 4)
        );
        assert_eq!(
            (symbols[1].body_end_line, symbols[1].body_end_char),
            (60, 5)
        );
        assert_eq!((symbols[1].sel_end_line, symbols[1].sel_end_char), (39, 19));
    }

    #[test]
    fn parses_flat_symbol_information() {
        // 일부 서버는 구형 SymbolInformation(평탄 + location)을 준다.
        let raw = json!([{
            "name": "helper",
            "kind": 12,
            "location": {"uri": "file:///tmp/a.rs",
                         "range": {"start": {"line": 3, "character": 0},
                                   "end": {"line": 9, "character": 1}}}
        }]);
        let symbols = parse_document_symbols(&raw);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].sel_line, 3);
        assert_eq!(symbols[0].end_line, 9);
    }

    #[test]
    fn flat_symbol_information_keeps_its_container_name() {
        // 평탄형에서 부모 관계를 주는 유일한 통로가 containerName이다. 계층형의 children과
        // 같은 자리에 넣어야 두 형태가 같은 그래프를 만든다.
        let raw = json!([{
            "name": "validate",
            "kind": 6,
            "containerName": "GoalContract",
            "location": {"uri": "file:///tmp/a.rs",
                         "range": {"start": {"line": 39, "character": 4},
                                   "end": {"line": 60, "character": 5}}}
        }]);
        let symbols = parse_document_symbols(&raw);
        assert_eq!(symbols[0].container, Some("GoalContract".to_string()));
    }

    #[test]
    fn nested_containers_name_their_immediate_parent() {
        // 3단 중첩에서 손자의 container는 조부가 아니라 부모다 — 경로가 아니라 한 칸 위다.
        let raw = json!([{
            "name": "outer", "kind": 23,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 99, "character": 0}},
            "selectionRange": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 11}},
            "children": [{
                "name": "middle", "kind": 23,
                "range": {"start": {"line": 10, "character": 0}, "end": {"line": 90, "character": 0}},
                "selectionRange": {"start": {"line": 10, "character": 6}, "end": {"line": 10, "character": 12}},
                "children": [{
                    "name": "inner", "kind": 6,
                    "range": {"start": {"line": 20, "character": 0}, "end": {"line": 30, "character": 0}},
                    "selectionRange": {"start": {"line": 20, "character": 7}, "end": {"line": 20, "character": 12}},
                    "children": []
                }]
            }]
        }]);
        let symbols = parse_document_symbols(&raw);
        assert_eq!(symbols.len(), 3);
        assert_eq!(symbols[2].name, "inner");
        assert_eq!(symbols[2].container, Some("middle".to_string()));
    }

    #[test]
    fn returns_empty_on_null_response() {
        assert!(parse_document_symbols(&Value::Null).is_empty());
        assert!(parse_document_symbols(&json!([])).is_empty());
    }

    #[test]
    fn a_symbol_without_a_usable_range_is_dropped_not_defaulted() {
        // 좌표가 없는 심볼을 0행으로 채우면 그래프가 파일 첫 줄에 유령 노드를 만든다.
        // 그래프의 좌표는 나중에 되짚을 수 있어야 하므로, 못 읽으면 버린다.
        let raw = json!([
            {"name": "broken", "kind": 12},
            {"name": "fine", "kind": 12,
             "range": {"start": {"line": 1, "character": 0}, "end": {"line": 2, "character": 0}},
             "selectionRange": {"start": {"line": 1, "character": 3}, "end": {"line": 1, "character": 7}},
             "children": []}
        ]);
        let symbols = parse_document_symbols(&raw);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "fine");
    }

    #[test]
    fn jdtls_service_ready_is_the_only_ready_status() {
        // 실제 jdtls 순서 — Started의 message가 "Ready"라 문구로 판정하면 일찍 통과한다.
        let started = parse_language_status(&json!({ "type": "Started", "message": "Ready" }))
            .expect("유효한 language/status");
        assert!(!started.is_ready());
        assert!(
            !parse_language_status(&json!({ "type": "ProjectStatus", "message": "OK" }))
                .unwrap()
                .is_ready()
        );
        assert!(parse_language_status(
            &json!({ "type": "ServiceReady", "message": "ServiceReady" })
        )
        .unwrap()
        .is_ready());
    }

    #[test]
    fn language_status_without_type_is_not_parsed() {
        assert!(parse_language_status(&json!({ "message": "Ready" })).is_none());
    }

    #[test]
    fn parses_tsserver_progress_kind() {
        let begin = json!({
            "token": "1",
            "value": { "kind": "begin", "title": "Initializing JS/TS language features…" }
        });
        assert_eq!(parse_progress_kind(&begin).as_deref(), Some("begin"));
        let end = json!({ "token": "1", "value": { "kind": "end" } });
        assert_eq!(parse_progress_kind(&end).as_deref(), Some("end"));
    }

    #[test]
    fn progress_without_value_is_not_parsed() {
        assert!(parse_progress_kind(&json!({ "token": "1" })).is_none());
    }

    #[test]
    fn parses_rust_analyzer_ready_status() {
        let status = parse_server_status(&json!({
            "health": "ok",
            "quiescent": true,
            "message": "workspace loaded"
        }))
        .expect("유효한 serverStatus");

        assert!(status.is_ready());
        assert_eq!(status.message.as_deref(), Some("workspace loaded"));
    }

    #[test]
    fn rejects_incomplete_server_status() {
        assert!(parse_server_status(&json!({ "health": "ok" })).is_none());
        let busy = parse_server_status(&json!({
            "health": "ok",
            "quiescent": false
        }))
        .unwrap();
        assert!(!busy.is_ready(), "백그라운드 분석 중이면 준비가 아니다");
    }
}
