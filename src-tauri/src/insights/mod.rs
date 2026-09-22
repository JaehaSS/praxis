//! 사용량 인사이트 — `~/.claude/projects/*/*.jsonl` 전체를 스캔해 세션/메시지/토큰/모델 통계를 집계.
//! Tauri 비의존(단위 테스트 가능). 여러 모델을 사용하는 환경을 가정해 **모델별 분해**를 함께 제공.
//!
//! 시간 버킷(날짜/시간/요일)은 로컬 시간대 기준. 외부 의존성(chrono 등) 없이 ISO8601 타임스탬프를
//! 직접 파싱한다. 프로젝트별 분해는 메시지의 `cwd` 필드를 키로 삼는다.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::Serialize;

mod agent_skills;
mod outcomes;
mod patterns;
pub use agent_skills::{
    compute_agent_skills, AgentSkillUsage, AgentSlice, AgentStat, SkillSlice, SkillStat,
};
pub use outcomes::{compute_outcomes, OutcomeInsights};
pub use patterns::{compute_patterns, TaskPatterns};

/// 모델 한 종에 대한 사용량 집계.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ModelStat {
    pub model: String,
    /// 모델 제공자 — 지금은 Claude 트랜스크립트만 스캔하므로 항상 "claude".
    pub provider: String,
    pub messages: u64,
    pub sessions: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 캐시 쓰기(생성) 토큰 — 비용 계산 시 입력가×1.25.
    pub cache_creation_tokens: u64,
    /// 캐시 읽기 토큰 — 비용 계산 시 입력가×0.1.
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    /// 24칸 — 이 모델의 시간대별 메시지 수.
    pub hours: Vec<u64>,
}

/// 하루치 활동량(히트맵용).
#[derive(Debug, Clone, Serialize)]
pub struct DayStat {
    /// YYYY-MM-DD (로컬)
    pub date: String,
    pub messages: u64,
    pub tokens: u64,
}

/// 프로젝트(cwd) 한 곳의 사용량 집계.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectStat {
    /// cwd 원본 경로.
    pub path: String,
    /// 표시명 — 마지막 세그먼트. 다른 프로젝트와 충돌하면 상위 세그먼트를 붙여 구분한다.
    pub name: String,
    pub sessions: u64,
    pub messages: u64,
    pub total_tokens: u64,
    /// 마지막 활동일 YYYY-MM-DD.
    pub last_active: String,
}

/// 직전 동일 길이 구간 요약 — 증감(Δ) 표시용.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PeriodSummary {
    pub sessions: u64,
    pub messages: u64,
    pub total_tokens: u64,
    pub active_days: u64,
}

/// 인사이트 전체 — 개요 지표 + 히트맵(days) + 시간대(hours) + 모델별/프로젝트별 분해.
#[derive(Debug, Clone, Serialize, Default)]
pub struct Insights {
    pub sessions: u64,
    pub messages: u64,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 캐시 생성 토큰 전체 합계.
    pub cache_creation_tokens: u64,
    /// 캐시 읽기 토큰 전체 합계.
    pub cache_read_tokens: u64,
    pub active_days: u64,
    pub current_streak: u64,
    pub longest_streak: u64,
    /// 메시지가 가장 많은 시간대(0~23, 로컬). 데이터 없으면 None.
    pub peak_hour: Option<u32>,
    /// 메시지 기준 가장 많이 쓴 모델.
    pub favorite_model: Option<String>,
    /// 날짜 오름차순 — 히트맵.
    pub days: Vec<DayStat>,
    /// 24칸 — 시간대별 메시지 수.
    pub hours: Vec<u64>,
    /// 168칸 — 요일(일=0) × 시간. 인덱스 = weekday * 24 + hour.
    pub weekday_hours: Vec<u64>,
    /// 메시지 많은 순 — 모델별 분해.
    pub models: Vec<ModelStat>,
    /// 토큰 많은 순 — 프로젝트별 분해.
    pub projects: Vec<ProjectStat>,
    /// 직전 동일 길이 구간. range="all"이면 비교 대상이 없어 None.
    pub prev: Option<PeriodSummary>,
}

/// 1970-01-01부터의 일수 (Howard Hinnant days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// days_from_civil의 역 — 일수 → (year, month, day).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 로컬 day 번호 → "YYYY-MM-DD".
fn date_string(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// day 번호 → 요일(일=0 … 토=6). 1970-01-01(day 0)은 목요일이므로 +4.
fn weekday(day: i64) -> usize {
    (day + 4).rem_euclid(7) as usize
}

/// 로컬 시간대로 환산된 타임스탬프 — day_num / hour / "YYYY-MM-DD" + 원본 UTC epoch.
struct Ts {
    /// UTC epoch(초) — cutoff 비교용(시간대 무관).
    epoch: i64,
    day: i64,
    hour: u32,
    date: String,
}

/// 메시지 한 건에서 뽑은 집계 입력.
struct Msg<'a> {
    session: &'a str,
    cwd: &'a str,
    model: Option<&'a str>,
    input: u64,
    output: u64,
    cache_c: u64,
    cache_r: u64,
}

impl Msg<'_> {
    fn total(&self) -> u64 {
        self.input + self.output + self.cache_c + self.cache_r
    }
}

/// "2026-06-30T01:03:44.931Z" → UTC epoch(초). 형식이 어긋나면 None(베스트 에포트).
/// 시간대는 여기서 다루지 않는다 — 캐시에 담기는 값이라 호출 시점의 오프셋과 무관해야 한다.
fn parse_epoch(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> { s.get(a..z)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

/// 타임스탬프 문자열 → 로컬 버킷. 캐시를 쓰지 않는 `agent_skills`가 이 짝을 그대로 쓴다.
fn parse_ts(s: &str, offset_secs: i64) -> Option<Ts> {
    parse_epoch(s).map(|e| ts_from_epoch(e, offset_secs))
}

/// UTC epoch → 로컬 버킷(offset_secs = 로컬 UTC 오프셋, KST=+32400).
fn ts_from_epoch(epoch: i64, offset_secs: i64) -> Ts {
    let local = epoch + offset_secs;
    let day = local.div_euclid(86400);
    Ts {
        epoch,
        day,
        hour: (local.rem_euclid(86400) / 3600) as u32,
        date: date_string(day),
    }
}

/// range 문자열("7d"|"30d"|"all"…) → 구간 길이(초). all/미인식이면 None.
fn range_span(range: &str) -> Option<i64> {
    match range {
        "7d" => Some(7 * 86400),
        "30d" => Some(30 * 86400),
        _ => None,
    }
}

/// range별 cutoff epoch(이상만 포함). all/미인식이면 0.
fn cutoff(range: &str, now: i64) -> i64 {
    range_span(range).map(|span| now - span).unwrap_or(0)
}

/// `~/.claude/projects` 하위 모든 *.jsonl 경로. 세션 하위의 서브에이전트 트랜스크립트는
/// 대상이 아니다 — 사용량 개요는 메인 세션 기준이고, 서브에이전트는 `agent_skills`가 따로 센다.
fn transcript_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(projects) = std::fs::read_dir(root) else {
        return out;
    };
    for proj in projects.flatten() {
        let p = proj.path();
        if !p.is_dir() {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(&p) {
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().map(|e| e == "jsonl").unwrap_or(false) {
                    out.push(fp);
                }
            }
        }
    }
    out
}

/// 프로젝트 한 곳의 누적 상태.
#[derive(Default)]
struct ProjectAcc {
    messages: u64,
    total_tokens: u64,
    sessions: HashSet<String>,
    /// 마지막 활동 day 번호.
    last_day: i64,
}

/// 누적기 — 파일 스캔 중 상태. 테스트에서 직접 호출 가능하도록 분리.
#[derive(Default)]
struct Acc {
    messages: u64,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    sessions: HashSet<String>,
    day_msgs: BTreeMap<i64, (String, u64, u64)>, // day_num → (date, messages, tokens)
    active_days: BTreeSet<i64>,
    hours: [u64; 24],
    /// [요일(일=0)][시간] — 고정 크기 168 배열은 Default가 없어 2차원으로 둔다.
    weekday_hours: [[u64; 24]; 7],
    models: HashMap<String, ModelStat>,
    model_sessions: HashMap<String, HashSet<String>>,
    projects: HashMap<String, ProjectAcc>,
    /// cwd 원본 → 저장소 루트. compute 한 번 동안만 유효(파일시스템 상태가 바뀔 수 있다).
    repo_roots: HashMap<String, String>,
}

/// 직전 구간 누적기 — 요약 4지표만 필요하므로 경량.
#[derive(Default)]
struct PrevAcc {
    messages: u64,
    total_tokens: u64,
    sessions: HashSet<String>,
    active_days: BTreeSet<i64>,
}

impl PrevAcc {
    fn add(&mut self, ts: &Ts, m: &Msg) {
        self.messages += 1;
        self.total_tokens += m.total();
        if !m.session.is_empty() {
            self.sessions.insert(m.session.to_string());
        }
        self.active_days.insert(ts.day);
    }

    fn finish(self) -> PeriodSummary {
        PeriodSummary {
            sessions: self.sessions.len() as u64,
            messages: self.messages,
            total_tokens: self.total_tokens,
            active_days: self.active_days.len() as u64,
        }
    }
}

impl Acc {
    /// 메시지 한 건 반영.
    fn add(&mut self, ts: &Ts, m: &Msg) {
        let total = m.total();
        self.messages += 1;
        self.input_tokens += m.input;
        self.output_tokens += m.output;
        self.cache_creation_tokens += m.cache_c;
        self.cache_read_tokens += m.cache_r;
        self.total_tokens += total;
        if !m.session.is_empty() {
            self.sessions.insert(m.session.to_string());
        }
        self.active_days.insert(ts.day);
        if (ts.hour as usize) < 24 {
            self.hours[ts.hour as usize] += 1;
            self.weekday_hours[weekday(ts.day)][ts.hour as usize] += 1;
        }
        let e = self
            .day_msgs
            .entry(ts.day)
            .or_insert_with(|| (ts.date.clone(), 0, 0));
        e.1 += 1;
        e.2 += total;
        if !m.cwd.is_empty() {
            let root = memo_repo_root(&mut self.repo_roots, m.cwd);
            let p = self.projects.entry(root).or_default();
            p.messages += 1;
            p.total_tokens += total;
            p.last_day = p.last_day.max(ts.day);
            if !m.session.is_empty() {
                p.sessions.insert(m.session.to_string());
            }
        }
        if let Some(name) = m.model {
            let ms = self
                .models
                .entry(name.to_string())
                .or_insert_with(|| ModelStat {
                    model: name.to_string(),
                    provider: "claude".to_string(),
                    hours: vec![0; 24],
                    ..Default::default()
                });
            ms.messages += 1;
            ms.input_tokens += m.input;
            ms.output_tokens += m.output;
            ms.cache_creation_tokens += m.cache_c;
            ms.cache_read_tokens += m.cache_r;
            ms.total_tokens += total;
            if (ts.hour as usize) < 24 {
                ms.hours[ts.hour as usize] += 1;
            }
            if !m.session.is_empty() {
                self.model_sessions
                    .entry(name.to_string())
                    .or_default()
                    .insert(m.session.to_string());
            }
        }
    }
}

/// 경로를 구분자(`/`, `\`)로 쪼갠 세그먼트. 빈 조각은 버린다.
fn split_segments(path: &str) -> Vec<&str> {
    path.split(['/', '\\']).filter(|s| !s.is_empty()).collect()
}

/// `<repo>/.praxis/worktrees/<name>/…` → `<repo>`. 그 패턴이 없으면 원본 그대로.
fn strip_worktree_suffix(cwd: &str) -> &str {
    let bytes = cwd.as_bytes();
    let mut segs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for i in 0..=bytes.len() {
        if i == bytes.len() || bytes[i] == b'/' || bytes[i] == b'\\' {
            if i > start {
                segs.push((start, i));
            }
            start = i + 1;
        }
    }
    for w in segs.windows(2) {
        if &cwd[w[0].0..w[0].1] != ".praxis" || &cwd[w[1].0..w[1].1] != "worktrees" {
            continue;
        }
        let head = &cwd[..w[0].0];
        let trimmed = head.trim_end_matches(['/', '\\']);
        return if trimmed.is_empty() { head } else { trimmed };
    }
    cwd
}

/// cwd → 그 작업이 속한 저장소 루트. worktree 경로를 원본 체크아웃으로 접고,
/// 거기서 조상으로 올라가며 첫 저장소 루트를 고른다. 못 찾으면 접기만 한 경로.
pub fn normalize_repo_root(cwd: &str, is_repo_root: &dyn Fn(&Path) -> bool) -> String {
    if cwd.is_empty() {
        return String::new();
    }
    let base = strip_worktree_suffix(cwd);
    for anc in Path::new(base).ancestors() {
        if is_repo_root(anc) {
            return anc.to_string_lossy().into_owned();
        }
    }
    base.to_string()
}

/// 같은 cwd를 반복해 stat하지 않도록 메모를 낀 `normalize_repo_root`.
fn memo_repo_root(memo: &mut HashMap<String, String>, cwd: &str) -> String {
    if let Some(hit) = memo.get(cwd) {
        return hit.clone();
    }
    let root = normalize_repo_root(cwd, &|p| p.join(".git").exists());
    memo.insert(cwd.to_string(), root.clone());
    root
}

/// 뒤에서 depth개 세그먼트를 `/`로 이어붙인 표시명.
fn tail_name(segs: &[&str], depth: usize) -> String {
    let start = segs.len().saturating_sub(depth.max(1));
    segs[start..].join("/")
}

/// 경로 목록 → 표시명 목록. 마지막 세그먼트로 시작해, 충돌한 항목만
/// 더 붙일 세그먼트가 남아 있는 한 상위 세그먼트를 한 단계씩 앞에 붙인다.
fn display_names(paths: &[String]) -> Vec<String> {
    let segs: Vec<Vec<&str>> = paths.iter().map(|p| split_segments(p)).collect();
    let mut depth = vec![1usize; paths.len()];
    loop {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, s) in segs.iter().enumerate() {
            groups.entry(tail_name(s, depth[i])).or_default().push(i);
        }
        let mut changed = false;
        for idxs in groups.values() {
            if idxs.len() < 2 {
                continue;
            }
            for &i in idxs {
                if depth[i] < segs[i].len() {
                    depth[i] += 1;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    segs.iter()
        .enumerate()
        .map(|(i, s)| tail_name(s, depth[i]))
        .collect()
}

/// 파일 한 줄에서 뽑은 원시 레코드 — cutoff·시간대와 무관해 파일 단위로 캐시할 수 있다.
#[derive(Clone)]
struct Rec {
    /// UTC epoch(초).
    epoch: i64,
    session: String,
    /// 원본 cwd. 저장소 루트 해석은 파일시스템 상태에 달려 있어 캐시에 담지 않는다.
    cwd: String,
    model: Option<String>,
    input: u64,
    output: u64,
    cache_c: u64,
    cache_r: u64,
}

/// 파일 한 개의 파싱 결과 + 그때의 mtime·size. 둘 중 하나라도 다르면 다시 읽는다.
struct CachedFile {
    mtime: Option<SystemTime>,
    size: u64,
    recs: Vec<Rec>,
}

static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedFile>>> = OnceLock::new();

/// 파일을 실제로 읽은 횟수 — 캐시가 스캔을 건너뛰는지 테스트가 확인한다.
static FILE_READS: AtomicUsize = AtomicUsize::new(0);

/// 지금까지 읽은 파일 수 — 테스트 전용 관측점.
pub fn file_read_count() -> usize {
    FILE_READS.load(Ordering::Relaxed)
}

/// range별 인사이트 집계. offset_secs = 로컬 UTC 오프셋(KST=32400). 베스트 에포트.
pub fn compute(range: &str, offset_secs: i64) -> Insights {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    compute_in(
        &crate::sessionhome::projects_root().unwrap_or_default(),
        range,
        offset_secs,
        now,
    )
}

/// 트랜스크립트 루트와 현재 시각을 주입받는 본체 — 테스트가 가짜 루트로 부른다.
fn compute_in(root: &Path, range: &str, offset_secs: i64, now: i64) -> Insights {
    let cut = cutoff(range, now);
    // 직전 구간 = 현재 구간과 같은 길이로 그 앞. all이면 비교 대상 없음.
    let prev_cut = range_span(range).map(|span| cut - span).unwrap_or(0);
    let mut prev = range_span(range).map(|_| PrevAcc::default());
    let mut acc = Acc::default();
    let files = transcript_files(root);
    let cell = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cell.lock().unwrap_or_else(|e| e.into_inner());
    let live: HashSet<&PathBuf> = files.iter().collect();
    cache.retain(|path, _| live.contains(path));
    for path in &files {
        let recs = cached_recs(&mut cache, path);
        apply_records(&mut acc, &mut prev, recs, cut, prev_cut, offset_secs);
    }
    drop(cache);
    // 로컬 오늘 = (now + offset)의 day.
    let today = (now + offset_secs).div_euclid(86400);
    finalize(acc, prev, today)
}

/// 캐시가 파일의 현재 mtime·size와 맞으면 그대로, 아니면 읽어서 갱신한 뒤 돌려준다.
fn cached_recs<'a>(cache: &'a mut HashMap<PathBuf, CachedFile>, path: &Path) -> &'a [Rec] {
    let meta = std::fs::metadata(path).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok());
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let fresh = cache
        .get(path)
        .map(|c| c.mtime == mtime && c.size == size)
        .unwrap_or(false);
    if !fresh {
        FILE_READS.fetch_add(1, Ordering::Relaxed);
        let recs = std::fs::read_to_string(path)
            .map(|c| parse_records(&c))
            .unwrap_or_default();
        cache.insert(path.to_path_buf(), CachedFile { mtime, size, recs });
    }
    cache.get(path).map(|c| c.recs.as_slice()).unwrap_or(&[])
}

/// JSONL 본문 → 원시 레코드. user/assistant 외의 줄과 깨진 줄은 버린다.
fn parse_records(content: &str) -> Vec<Rec> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ty = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if ty != "user" && ty != "assistant" {
            continue;
        }
        let Some(epoch) = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(parse_epoch)
        else {
            continue;
        };
        let str_of = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let (mut input, mut output, mut cache_c, mut cache_r) = (0u64, 0u64, 0u64, 0u64);
        let mut model: Option<String> = None;
        if ty == "assistant" {
            if let Some(m) = v.get("message") {
                model = m.get("model").and_then(|x| x.as_str()).map(str::to_string);
                if let Some(u) = m.get("usage") {
                    let g = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    input = g("input_tokens");
                    output = g("output_tokens");
                    cache_c = g("cache_creation_input_tokens");
                    cache_r = g("cache_read_input_tokens");
                }
            }
        }
        out.push(Rec {
            epoch,
            session: str_of("sessionId"),
            cwd: str_of("cwd"),
            model,
            input,
            output,
            cache_c,
            cache_r,
        });
    }
    out
}

/// 레코드를 누적기에 반영. cutoff 이상은 acc로, [prev_cutoff, cutoff)는 prev로 분배한다.
fn apply_records(
    acc: &mut Acc,
    prev: &mut Option<PrevAcc>,
    recs: &[Rec],
    cutoff: i64,
    prev_cutoff: i64,
    offset_secs: i64,
) {
    for r in recs {
        if r.epoch < prev_cutoff {
            continue;
        }
        let ts = ts_from_epoch(r.epoch, offset_secs);
        let msg = Msg {
            session: &r.session,
            cwd: &r.cwd,
            model: r.model.as_deref(),
            input: r.input,
            output: r.output,
            cache_c: r.cache_c,
            cache_r: r.cache_r,
        };
        if r.epoch >= cutoff {
            acc.add(&ts, &msg);
        } else if let Some(p) = prev.as_mut() {
            p.add(&ts, &msg);
        }
    }
}

/// 누적기 → 최종 Insights (스트릭/피크/정렬 계산). today = 로컬 오늘의 day 번호.
fn finalize(acc: Acc, prev: Option<PrevAcc>, today: i64) -> Insights {
    // 스트릭: 연속 활동일.
    let set = &acc.active_days;
    let mut current_streak = 0u64;
    let mut d = today;
    if !set.contains(&d) {
        d -= 1; // 오늘 미활동이면 어제부터 카운트.
    }
    while set.contains(&d) {
        current_streak += 1;
        d -= 1;
    }
    let mut longest_streak = 0u64;
    let mut run = 0u64;
    let mut prev_day: Option<i64> = None;
    for &dn in set.iter() {
        run = match prev_day {
            Some(p) if dn == p + 1 => run + 1,
            _ => 1,
        };
        longest_streak = longest_streak.max(run);
        prev_day = Some(dn);
    }

    let peak_hour = acc
        .hours
        .iter()
        .enumerate()
        .filter(|(_, &c)| c > 0)
        .max_by_key(|(_, &c)| c)
        .map(|(h, _)| h as u32);

    let days: Vec<DayStat> = acc
        .day_msgs
        .values()
        .map(|(date, m, t)| DayStat {
            date: date.clone(),
            messages: *m,
            tokens: *t,
        })
        .collect();

    let mut models: Vec<ModelStat> = acc
        .models
        .into_iter()
        .map(|(name, mut s)| {
            s.sessions = acc
                .model_sessions
                .get(&name)
                .map(|set| set.len() as u64)
                .unwrap_or(0);
            s
        })
        .collect();
    models.sort_by(|a, b| b.messages.cmp(&a.messages));
    let favorite_model = models.first().map(|m| m.model.clone());

    // 프로젝트: 토큰 내림차순 정렬 후 표시명 부여(충돌 시 상위 세그먼트 추가).
    let mut projects: Vec<ProjectStat> = acc
        .projects
        .into_iter()
        .map(|(path, p)| ProjectStat {
            path,
            name: String::new(),
            sessions: p.sessions.len() as u64,
            messages: p.messages,
            total_tokens: p.total_tokens,
            last_active: date_string(p.last_day),
        })
        .collect();
    projects.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.path.cmp(&b.path))
    });
    let paths: Vec<String> = projects.iter().map(|p| p.path.clone()).collect();
    for (p, name) in projects.iter_mut().zip(display_names(&paths)) {
        p.name = name;
    }

    Insights {
        sessions: acc.sessions.len() as u64,
        messages: acc.messages,
        total_tokens: acc.total_tokens,
        input_tokens: acc.input_tokens,
        output_tokens: acc.output_tokens,
        cache_creation_tokens: acc.cache_creation_tokens,
        cache_read_tokens: acc.cache_read_tokens,
        active_days: acc.active_days.len() as u64,
        current_streak,
        longest_streak,
        peak_hour,
        favorite_model,
        days,
        hours: acc.hours.to_vec(),
        weekday_hours: acc.weekday_hours.iter().flatten().copied().collect(),
        models,
        projects,
        prev: prev.map(PrevAcc::finish),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 캐시와 FILE_READS는 전역이다 — 파일을 읽는 테스트는 전부 직렬화한다.
    fn guard() -> MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 가짜 트랜스크립트 루트(`…/projects`). 테스트마다 다른 경로를 쓴다.
    fn tmp_projects(name: &str) -> PathBuf {
        let root = crate::testtmp::dir()
            .join(format!("insights-{}-{name}", std::process::id()))
            .join("projects");
        std::fs::create_dir_all(root.join("proj")).unwrap();
        root
    }

    fn write_jsonl(root: &Path, file: &str, content: &str) {
        std::fs::write(root.join("proj").join(file), content).unwrap();
    }

    fn line(ty: &str, ts: &str, session: &str, model: &str, input: u64, output: u64) -> String {
        if ty == "assistant" {
            format!(
                r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"{session}","message":{{"model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
            )
        } else {
            format!(
                r#"{{"type":"user","timestamp":"{ts}","sessionId":"{session}","message":{{"content":"hi"}}}}"#
            )
        }
    }

    /// cwd 포함 assistant 라인.
    fn line_cwd(ts: &str, session: &str, cwd: &str, tokens: u64) -> String {
        let cwd = cwd.replace('\\', "\\\\");
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"{session}","cwd":"{cwd}","message":{{"model":"claude-opus-4-8","usage":{{"input_tokens":{tokens},"output_tokens":0,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        )
    }

    /// 테스트 기본값 — 직전 구간 비활성.
    fn feed(acc: &mut Acc, content: &str, cutoff: i64, offset_secs: i64) {
        apply_records(
            acc,
            &mut None,
            &parse_records(content),
            cutoff,
            0,
            offset_secs,
        );
    }

    #[test]
    fn aggregates_messages_tokens_models() {
        let content = [
            line("user", "2026-06-30T08:00:00.000Z", "s1", "", 0, 0),
            line(
                "assistant",
                "2026-06-30T08:00:01.000Z",
                "s1",
                "claude-opus-4-8",
                100,
                50,
            ),
            line(
                "assistant",
                "2026-06-30T08:30:01.000Z",
                "s1",
                "claude-opus-4-8",
                20,
                10,
            ),
            line(
                "assistant",
                "2026-06-29T08:00:01.000Z",
                "s2",
                "claude-sonnet-4-6",
                10,
                5,
            ),
        ]
        .join("\n");
        let mut acc = Acc::default();
        feed(&mut acc, &content, 0, 0);
        let today = days_from_civil(2026, 6, 30);
        let ins = finalize(acc, None, today);
        assert_eq!(ins.messages, 4);
        assert_eq!(ins.sessions, 2);
        assert_eq!(ins.total_tokens, 150 + 30 + 15);
        assert_eq!(ins.models.len(), 2);
        assert_eq!(ins.favorite_model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(ins.active_days, 2);
        assert_eq!(ins.current_streak, 2); // 6/30, 6/29 연속
        assert_eq!(ins.peak_hour, Some(8));
        assert!(ins.prev.is_none());
    }

    #[test]
    fn cutoff_filters_old_messages() {
        let content = [
            line("assistant", "2026-06-30T08:00:01.000Z", "s1", "m", 100, 50),
            line("assistant", "2020-01-01T08:00:01.000Z", "s2", "m", 100, 50),
        ]
        .join("\n");
        let now = days_from_civil(2026, 6, 30) * 86400 + 9 * 3600;
        let mut acc = Acc::default();
        feed(&mut acc, &content, cutoff("7d", now), 0);
        let ins = finalize(acc, None, days_from_civil(2026, 6, 30));
        assert_eq!(ins.messages, 1);
        assert_eq!(ins.sessions, 1);
    }

    #[test]
    fn local_offset_shifts_day_and_hour() {
        // UTC 16:00 + KST(+9h) = 익일 01:00 로컬.
        let content = line("assistant", "2026-06-29T16:00:00.000Z", "s1", "m", 1, 1);
        let mut acc = Acc::default();
        feed(&mut acc, &content, 0, 9 * 3600);
        let ins = finalize(acc, None, days_from_civil(2026, 6, 30));
        assert_eq!(ins.days[0].date, "2026-06-30");
        assert_eq!(ins.peak_hour, Some(1));
    }

    #[test]
    fn civil_roundtrip() {
        for &d in &[0i64, 1, 10957, 20000, -100] {
            let (y, m, day) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, day), d);
        }
    }

    #[test]
    fn days_from_civil_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 1, 1), 10957);
    }

    #[test]
    fn aggregates_projects_by_cwd() {
        let content = [
            line_cwd("2026-06-30T08:00:00.000Z", "s1", "/home/me/alpha", 100),
            line_cwd("2026-06-30T09:00:00.000Z", "s1", "/home/me/alpha", 50),
            line_cwd("2026-06-28T09:00:00.000Z", "s2", "/home/me/beta", 400),
        ]
        .join("\n");
        let mut acc = Acc::default();
        feed(&mut acc, &content, 0, 0);
        let ins = finalize(acc, None, days_from_civil(2026, 6, 30));
        assert_eq!(ins.projects.len(), 2);
        // 토큰 내림차순 — beta(400)가 먼저.
        assert_eq!(ins.projects[0].name, "beta");
        assert_eq!(ins.projects[0].total_tokens, 400);
        assert_eq!(ins.projects[0].sessions, 1);
        assert_eq!(ins.projects[0].last_active, "2026-06-28");
        assert_eq!(ins.projects[1].name, "alpha");
        assert_eq!(ins.projects[1].messages, 2);
        assert_eq!(ins.projects[1].last_active, "2026-06-30");
    }

    #[test]
    fn project_names_disambiguate_on_collision() {
        // 같은 마지막 세그먼트(app) 두 개 + 충돌 없는 하나.
        let names = display_names(&[
            "C:\\Users\\me\\OneDrive\\work\\app".to_string(),
            "/home/me/personal/app".to_string(),
            "/home/me/praxis-main".to_string(),
        ]);
        assert_eq!(names[0], "work/app");
        assert_eq!(names[1], "personal/app");
        // 충돌하지 않은 항목은 짧은 이름 유지.
        assert_eq!(names[2], "praxis-main");
    }

    #[test]
    fn project_names_go_deeper_until_unique() {
        // 마지막 두 세그먼트까지 같아 세 번째까지 가야 갈린다.
        let names = display_names(&["/a/one/src/app".to_string(), "/b/two/src/app".to_string()]);
        assert_eq!(names[0], "one/src/app");
        assert_eq!(names[1], "two/src/app");
    }

    #[test]
    fn weekday_hours_bucket() {
        // 2026-06-30은 화요일 → weekday 인덱스 2. 08시.
        let content = line("assistant", "2026-06-30T08:00:00.000Z", "s1", "m", 1, 1);
        let mut acc = Acc::default();
        feed(&mut acc, &content, 0, 0);
        let ins = finalize(acc, None, days_from_civil(2026, 6, 30));
        assert_eq!(ins.weekday_hours.len(), 168);
        assert_eq!(ins.weekday_hours[2 * 24 + 8], 1);
        assert_eq!(ins.weekday_hours.iter().sum::<u64>(), 1);
    }

    #[test]
    fn weekday_of_known_dates() {
        assert_eq!(weekday(days_from_civil(2026, 6, 28)), 0); // 일
        assert_eq!(weekday(days_from_civil(2026, 6, 30)), 2); // 화
        assert_eq!(weekday(days_from_civil(1970, 1, 1)), 4); // 목
    }

    #[test]
    fn prev_window_separates_from_current() {
        // now = 2026-06-30 09:00 UTC, range 7d → 현재 [6/23, 6/30], 직전 [6/16, 6/23].
        let now = days_from_civil(2026, 6, 30) * 86400 + 9 * 3600;
        let content = [
            line("assistant", "2026-06-29T08:00:00.000Z", "s1", "m", 100, 0), // 현재
            line("assistant", "2026-06-20T08:00:00.000Z", "s2", "m", 70, 0),  // 직전
            line("assistant", "2026-06-01T08:00:00.000Z", "s3", "m", 999, 0), // 둘 다 아님
        ]
        .join("\n");
        let cut = cutoff("7d", now);
        let prev_cut = cut - 7 * 86400;
        let mut acc = Acc::default();
        let mut prev = Some(PrevAcc::default());
        apply_records(
            &mut acc,
            &mut prev,
            &parse_records(&content),
            cut,
            prev_cut,
            0,
        );
        let ins = finalize(acc, prev, days_from_civil(2026, 6, 30));
        assert_eq!(ins.messages, 1);
        assert_eq!(ins.total_tokens, 100);
        let p = ins.prev.expect("7d는 직전 구간이 있어야 한다");
        assert_eq!(p.messages, 1);
        assert_eq!(p.total_tokens, 70);
        assert_eq!(p.sessions, 1);
        assert_eq!(p.active_days, 1);
    }

    #[test]
    fn cache_tokens_roll_up() {
        let content = r#"{"type":"assistant","timestamp":"2026-06-30T08:00:00.000Z","sessionId":"s1","message":{"model":"claude-opus-4-8","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":700,"cache_creation_input_tokens":30}}}"#.to_string();
        let mut acc = Acc::default();
        feed(&mut acc, &content, 0, 0);
        let ins = finalize(acc, None, days_from_civil(2026, 6, 30));
        assert_eq!(ins.cache_read_tokens, 700);
        assert_eq!(ins.cache_creation_tokens, 30);
        assert_eq!(ins.total_tokens, 10 + 5 + 700 + 30);
    }

    #[test]
    fn normalize_repo_root_folds_worktree_path() {
        let never = |_: &Path| false;
        assert_eq!(
            normalize_repo_root("/home/me/app/.praxis/worktrees/wt-1/src", &never),
            "/home/me/app"
        );
    }

    #[test]
    fn normalize_repo_root_climbs_to_repo_ancestor() {
        let is_root = |p: &Path| p == Path::new("/home/me/app");
        assert_eq!(
            normalize_repo_root("/home/me/app/src/insights", &is_root),
            "/home/me/app"
        );
    }

    #[test]
    fn normalize_repo_root_folds_then_climbs() {
        let is_root = |p: &Path| p == Path::new("/home/me/mono");
        assert_eq!(
            normalize_repo_root("/home/me/mono/pkg/.praxis/worktrees/wt-1/src", &is_root),
            "/home/me/mono"
        );
    }

    #[test]
    fn normalize_repo_root_keeps_unmatched_path() {
        let never = |_: &Path| false;
        assert_eq!(
            normalize_repo_root("/home/me/plain", &never),
            "/home/me/plain"
        );
    }

    #[test]
    fn cache_reuses_parsed_records_until_file_changes() {
        let _g = guard();
        let root = tmp_projects("cache");
        let now = days_from_civil(2026, 6, 30) * 86400 + 9 * 3600;
        write_jsonl(
            &root,
            "a.jsonl",
            &line("assistant", "2026-06-30T08:00:00.000Z", "s1", "m", 10, 5),
        );
        write_jsonl(
            &root,
            "b.jsonl",
            &line("assistant", "2026-06-30T09:00:00.000Z", "s2", "m", 1, 1),
        );

        let first = compute_in(&root, "all", 0, now);
        let after_first = file_read_count();
        let second = compute_in(&root, "all", 0, now);
        assert_eq!(
            file_read_count(),
            after_first,
            "캐시 히트는 파일을 열지 않는다"
        );
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        );

        // 줄 추가 — mtime이 같아도 크기가 달라져 그 파일만 다시 읽힌다.
        write_jsonl(
            &root,
            "a.jsonl",
            &[
                line("assistant", "2026-06-30T08:00:00.000Z", "s1", "m", 10, 5),
                line("assistant", "2026-06-30T10:00:00.000Z", "s3", "m", 100, 0),
            ]
            .join("\n"),
        );
        let third = compute_in(&root, "all", 0, now);
        assert_eq!(
            file_read_count(),
            after_first + 1,
            "바뀐 파일 하나만 다시 읽는다"
        );
        assert_eq!(third.messages, first.messages + 1);
        assert_eq!(third.total_tokens, first.total_tokens + 100);
    }

    #[test]
    fn dropped_file_leaves_the_result() {
        let _g = guard();
        let root = tmp_projects("dropped");
        let now = days_from_civil(2026, 6, 30) * 86400 + 9 * 3600;
        write_jsonl(
            &root,
            "a.jsonl",
            &line("assistant", "2026-06-30T08:00:00.000Z", "s1", "m", 10, 5),
        );
        write_jsonl(
            &root,
            "b.jsonl",
            &line("assistant", "2026-06-30T09:00:00.000Z", "s2", "m", 7, 0),
        );
        let before = compute_in(&root, "all", 0, now);
        assert_eq!(before.messages, 2);

        std::fs::remove_file(root.join("proj").join("b.jsonl")).unwrap();
        let after = compute_in(&root, "all", 0, now);
        assert_eq!(after.messages, 1);
        assert_eq!(after.sessions, 1);
        assert_eq!(after.total_tokens, 15);
    }

    #[test]
    fn model_stat_carries_provider() {
        let _g = guard();
        let root = tmp_projects("provider");
        let now = days_from_civil(2026, 6, 30) * 86400 + 9 * 3600;
        write_jsonl(
            &root,
            "a.jsonl",
            &line(
                "assistant",
                "2026-06-30T08:00:00.000Z",
                "s1",
                "claude-opus-4-8",
                1,
                1,
            ),
        );
        let ins = compute_in(&root, "all", 0, now);
        assert_eq!(ins.models[0].provider, "claude");
    }
}
