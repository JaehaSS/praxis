use sha2::{Digest, Sha256};

use super::model::{
    CodeWikiPageState, Metadata, NodeRow, MAX_RELATIONS, MAX_SYMBOLS, SCHEMA_VERSION,
};

pub(super) fn page(
    source_path: &str,
    source_hash: &str,
    fingerprint: &str,
    run_id: i64,
    now: i64,
    nodes: &[NodeRow],
    edges: &[super::model::EdgeRow],
    edge_state: Option<&str>,
) -> String {
    let mut body = format!("# `{}`\n\nSource: [`{}`]({})\n\nThis page reflects the source fingerprint recorded at generation time. Put notes in a separate file; editing this generated page marks it as a conflict.\n", escape(source_path), escape(source_path), source_link(source_path, source_path));
    body.push_str("\n## Symbols\n");
    for node in nodes.iter().take(MAX_SYMBOLS) {
        let container = node
            .container
            .as_deref()
            .map(|v| format!(" in `{}`", escape(v)))
            .unwrap_or_default();
        body.push_str(&format!(
            "- `{}` — kind {}, line {}:{}{}\n",
            escape(&node.name),
            node.kind,
            node.sel_start_line + 1,
            node.sel_start_char + 1,
            container
        ));
    }
    if nodes.len() > MAX_SYMBOLS {
        body.push_str("\nSymbol list truncated at 200 entries.\n");
    }
    body.push_str("\n## Direct references\n");
    if let Some(reason) = edge_state {
        body.push_str(&format!(
            "\nReference analysis did not run for this file: {}. An empty list below means the references are unknown, not that none exist.\n",
            escape(reason)
        ));
    }
    for edge in edges.iter().take(MAX_RELATIONS) {
        body.push_str(&format!(
            "- [`{}` at {}:{}]({}) references [`{}` at {}:{}]({})\n",
            escape(&edge.src_name),
            escape(&edge.src_path),
            edge.src_line + 1,
            source_link(source_path, &edge.src_path),
            escape(&edge.dst_name),
            escape(&edge.dst_path),
            edge.dst_line + 1,
            source_link(source_path, &edge.dst_path)
        ));
    }
    if edges.len() > MAX_RELATIONS {
        body.push_str("\nReference list truncated at 100 entries.\n");
    }
    document(
        "module",
        Some(source_path),
        Some(source_hash),
        fingerprint,
        run_id,
        now,
        body,
    )
}

pub(super) fn index(
    fingerprint: &str,
    run_id: i64,
    now: i64,
    modules: &[(String, String, CodeWikiPageState)],
    orphans: &[(String, String)],
) -> String {
    let mut body = String::from("# Code Wiki\n\nGenerated from the active code graph. Source fingerprints describe generation time; check status before relying on a page.\n\n## Modules\n");
    for (source, page, state) in modules {
        match state {
            CodeWikiPageState::Missing => {
                body.push_str(&format!("- `{}` — missing\n", escape(source)))
            }
            CodeWikiPageState::Ready => {
                body.push_str(&format!("- [`{}`]({})\n", escape(source), url_path(page)))
            }
            CodeWikiPageState::Stale | CodeWikiPageState::Conflict => body.push_str(&format!(
                "- [`{}`]({}) — {}\n",
                escape(source),
                url_path(page),
                state.as_str()
            )),
            CodeWikiPageState::Orphaned => body.push_str(&format!(
                "- [`{}`]({}) — orphaned\n",
                escape(source),
                url_path(page)
            )),
        }
    }
    if !orphans.is_empty() {
        body.push_str("\n## Orphaned generated pages\n");
        for (source, page) in orphans {
            body.push_str(&format!("- [`{}`]({})\n", escape(source), url_path(page)));
        }
    }
    document("index", None, None, fingerprint, run_id, now, body)
}

fn document(
    kind: &str,
    source_path: Option<&str>,
    source_hash: Option<&str>,
    fingerprint: &str,
    run_id: i64,
    now: i64,
    body: String,
) -> String {
    let mut metadata = Metadata {
        kind: kind.into(),
        source_path: source_path.map(str::to_owned),
        source_hash: source_hash.map(str::to_owned),
        source_fingerprint: fingerprint.into(),
        run_id,
        generated_at: now,
        checksum: String::new(),
    };
    metadata.checksum = checksum(&metadata, &body);
    format!("{}{}", header(&metadata), body)
}

pub(super) fn header(meta: &Metadata) -> String {
    let source_path = meta.source_path.as_deref().unwrap_or("");
    let source_hash = meta.source_hash.as_deref().unwrap_or("");
    format!("<!-- praxis-codewiki\nschemaVersion: {}\nkind: {}\nsourcePath: {}\nsourceHash: {}\nsourceFingerprint: {}\nrunId: {}\ngeneratedAt: {}\nchecksum: {}\n-->\n\n", SCHEMA_VERSION, meta.kind, source_path, source_hash, meta.source_fingerprint, meta.run_id, meta.generated_at, meta.checksum)
}

pub(super) fn checksum(meta: &Metadata, body: &str) -> String {
    let data = format!("schemaVersion: {}\nkind: {}\nsourcePath: {}\nsourceHash: {}\nsourceFingerprint: {}\nrunId: {}\ngeneratedAt: {}\n\n{}", SCHEMA_VERSION, meta.kind, meta.source_path.as_deref().unwrap_or(""), meta.source_hash.as_deref().unwrap_or(""), meta.source_fingerprint, meta.run_id, meta.generated_at, body);
    format!("{:x}", Sha256::digest(data.as_bytes()))
}

pub(super) fn parse_document(text: &str) -> Option<(Metadata, &str)> {
    let end = text.find("-->\n\n")? + 5;
    let header = &text[..end];
    let mut values = header.lines().skip(1);
    let schema = values
        .next()?
        .strip_prefix("schemaVersion: ")?
        .parse::<u8>()
        .ok()?;
    if schema != SCHEMA_VERSION {
        return None;
    }
    let kind = values.next()?.strip_prefix("kind: ")?.to_owned();
    let source_path = empty(values.next()?.strip_prefix("sourcePath: ")?);
    let source_hash = empty(values.next()?.strip_prefix("sourceHash: ")?);
    let source_fingerprint = values
        .next()?
        .strip_prefix("sourceFingerprint: ")?
        .to_owned();
    let run_id = values.next()?.strip_prefix("runId: ")?.parse().ok()?;
    let generated_at = values.next()?.strip_prefix("generatedAt: ")?.parse().ok()?;
    let checksum_value = values.next()?.strip_prefix("checksum: ")?.to_owned();
    if values.next()? != "-->" {
        return None;
    }
    let meta = Metadata {
        kind,
        source_path,
        source_hash,
        source_fingerprint,
        run_id,
        generated_at,
        checksum: checksum_value,
    };
    (header == super::render::header(&meta) && meta.checksum == checksum(&meta, &text[end..]))
        .then_some((meta, &text[end..]))
}

fn empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}
fn escape(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        if matches!(
            character,
            '\\' | '`' | '[' | ']' | '(' | ')' | '*' | '_' | '#'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
        escaped
    })
}
fn source_link(from: &str, target: &str) -> String {
    let parents = std::path::Path::new(from)
        .parent()
        .map(|path| path.components().count())
        .unwrap_or(0);
    format!(
        "{}{}",
        "../".repeat(parents + 3),
        target
            .split('/')
            .map(url_component)
            .collect::<Vec<_>>()
            .join("/")
    )
}
fn url_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
fn url_path(value: &str) -> String {
    value
        .split('/')
        .map(url_component)
        .collect::<Vec<_>>()
        .join("/")
}
