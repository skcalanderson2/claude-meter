use serde::Deserialize;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Session metadata (~/.claude/sessions/*.json) ─────────────────────────────

#[derive(Debug, Deserialize)]
struct SessionMeta {
    #[serde(rename = "sessionId")]
    session_id: String,
    cwd: String,
    #[serde(rename = "updatedAt", default)]
    updated_at: u64,
}

// ── Per-message usage in project JSONL files ─────────────────────────────────

#[derive(Deserialize, Default)]
struct TokenUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[derive(Deserialize)]
struct JournalMessage {
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct JournalEntry {
    timestamp: Option<Value>,
    message: Option<JournalMessage>,
}

// ── History (for 5-hour window countdown) ────────────────────────────────────

#[derive(Deserialize)]
struct HistoryEntry {
    timestamp: u64,
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppData {
    /// input+output tokens in the most recently active Claude Code session
    pub tokens_session: u64,
    /// input+output tokens across all sessions in the last 7 days
    pub tokens_this_week: u64,
    /// seconds until the 5-hour rate-limit window resets
    pub remaining_secs: u64,
    /// per-day token totals: index 0 = 6 days ago, index 6 = today
    pub daily_tokens: [u64; 7],
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Parse a JSONL timestamp (integer ms or ISO-8601 string) → ms since epoch.
fn parse_ts(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => {
            let s = s.trim_end_matches('Z');
            let (date, time) = s.split_once('T')?;
            let mut dp = date.splitn(3, '-');
            let yr: i64 = dp.next()?.parse().ok()?;
            let mo: i64 = dp.next()?.parse().ok()?;
            let dy: i64 = dp.next()?.parse().ok()?;
            let time_no_frac = time.split('.').next().unwrap_or(time);
            let mut tp = time_no_frac.splitn(3, ':');
            let hr: i64 = tp.next()?.parse().ok()?;
            let mn: i64 = tp.next()?.parse().ok()?;
            let sc: i64 = tp.next().unwrap_or("0").parse().ok()?;
            let days = days_since_epoch(yr, mo, dy)?;
            let secs = days * 86400 + hr * 3600 + mn * 60 + sc;
            Some((secs as u64) * 1000)
        }
        _ => None,
    }
}

fn days_since_epoch(y: i64, m: i64, d: i64) -> Option<i64> {
    let m2 = if m <= 2 { m + 9 } else { m - 3 };
    let y2 = if m <= 2 { y - 1 } else { y };
    let c = y2 / 100;
    let yc = y2 % 100;
    let days = (146097 * c) / 4 + (1461 * yc) / 4 + (153 * m2 + 2) / 5 + d - 719469;
    Some(days)
}

fn load_session_metas() -> Vec<SessionMeta> {
    let dir = home_dir().join(".claude/sessions");
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    entries
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<SessionMeta>(&s).ok())
        .collect()
}

fn sum_session_tokens(cwd: &str, session_id: &str) -> u64 {
    let cwd_encoded = cwd.replace('/', "-").replace('_', "-");
    let path = home_dir()
        .join(".claude/projects")
        .join(&cwd_encoded)
        .join(format!("{}.jsonl", session_id));

    let Ok(file) = File::open(&path) else { return 0 };
    let reader = BufReader::new(file);
    let mut total = 0u64;
    for line in reader.lines().map_while(Result::ok) {
        if line.trim().is_empty() { continue }
        let Ok(entry) = serde_json::from_str::<JournalEntry>(&line) else { continue };
        if let Some(usage) = entry.message.and_then(|m| m.usage) {
            total += usage.input_tokens + usage.output_tokens;
        }
    }
    total
}

struct ScanResult {
    /// (first_message_ts_ms, total_tokens) per JSONL file, for weekly filtering
    sessions: Vec<(u64, u64)>,
    /// daily token totals: index 0 = 6 days ago, index 6 = today (UTC days)
    daily: [u64; 7],
}

fn scan_all_project_jsonl() -> ScanResult {
    let now_day = now_ms() / 86_400_000;
    let mut daily = [0u64; 7];
    let mut sessions = Vec::new();

    let projects_dir = home_dir().join(".claude/projects");
    let Ok(project_entries) = std::fs::read_dir(&projects_dir) else {
        return ScanResult { sessions, daily };
    };

    for proj in project_entries.flatten() {
        let Ok(files) = std::fs::read_dir(proj.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().map(|x| x != "jsonl").unwrap_or(true) { continue }

            let Ok(file) = File::open(&p) else { continue };
            let mut first_ts: Option<u64> = None;
            let mut file_tokens = 0u64;

            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if line.trim().is_empty() { continue }
                let Ok(entry) = serde_json::from_str::<JournalEntry>(&line) else { continue };

                let ts_ms = entry.timestamp.as_ref().and_then(parse_ts);
                let tokens = entry.message
                    .as_ref()
                    .and_then(|m| m.usage.as_ref())
                    .map(|u| u.input_tokens + u.output_tokens)
                    .unwrap_or(0);

                if let Some(ts) = ts_ms {
                    if first_ts.map(|f| ts < f).unwrap_or(true) {
                        first_ts = Some(ts);
                    }
                    if tokens > 0 {
                        let entry_day = ts / 86_400_000;
                        if entry_day <= now_day {
                            let diff = now_day - entry_day;
                            if diff < 7 {
                                daily[(6 - diff) as usize] += tokens;
                            }
                        }
                    }
                }

                file_tokens += tokens;
            }

            if let Some(ts) = first_ts {
                if file_tokens > 0 {
                    sessions.push((ts, file_tokens));
                }
            }
        }
    }

    ScanResult { sessions, daily }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn load_history_timestamps() -> Vec<u64> {
    let path = home_dir().join(".claude/history.jsonl");
    let Ok(contents) = std::fs::read_to_string(&path) else { return vec![] };
    contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<HistoryEntry>(l).ok())
        .map(|e| e.timestamp)
        .collect()
}

pub fn compute_app_data(history_timestamps: &[u64]) -> AppData {
    let now = now_ms();
    const WEEK_MS: u64 = 7 * 86_400_000;
    const WINDOW_MS: u64 = 5 * 3_600_000;

    let sessions = load_session_metas();

    let current = sessions.iter().max_by_key(|s| s.updated_at);
    let tokens_session = current
        .map(|s| sum_session_tokens(&s.cwd, &s.session_id))
        .unwrap_or(0);

    let scan = scan_all_project_jsonl();

    let cutoff_week = now.saturating_sub(WEEK_MS);
    let tokens_this_week = scan.sessions.iter()
        .filter(|(first_ts, _)| *first_ts >= cutoff_week)
        .map(|(_, tokens)| tokens)
        .sum();

    let cutoff = now.saturating_sub(WINDOW_MS);
    let remaining_secs = history_timestamps
        .iter()
        .filter(|&&ts| ts >= cutoff)
        .min()
        .map(|&min_ts| ((min_ts + WINDOW_MS).saturating_sub(now) / 1000).min(5 * 3600))
        .unwrap_or(5 * 3600);

    AppData { tokens_session, tokens_this_week, remaining_secs, daily_tokens: scan.daily }
}
