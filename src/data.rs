use serde::Deserialize;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppData {
    /// input+output tokens across all projects in the rolling 5-hour window
    pub tokens_session: u64,
    /// input+output tokens across all sessions in the last 7 days
    pub tokens_this_week: u64,
    /// seconds until the rate-limit window resets (oldest msg in window + 5h − now)
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

// ── Single-pass scan of all project JSONL files ───────────────────────────────

struct ScanResult {
    /// tokens in rolling 5-hour window (by entry timestamp)
    window_tokens: u64,
    /// oldest entry timestamp within the 5-hour window (drives reset countdown)
    window_oldest_ts: Option<u64>,
    /// (first_message_ts_ms, total_tokens) per file for 7-day weekly filter
    sessions: Vec<(u64, u64)>,
    /// daily token totals: index 0 = 6 days ago, index 6 = today (UTC days)
    daily: [u64; 7],
}

fn scan_all_project_jsonl(now: u64, window_cutoff: u64) -> ScanResult {
    let now_day = now / 86_400_000;
    let mut window_tokens = 0u64;
    let mut window_oldest_ts: Option<u64> = None;
    let mut daily = [0u64; 7];
    let mut sessions = Vec::new();

    let projects_dir = home_dir().join(".claude/projects");
    let Ok(project_entries) = std::fs::read_dir(&projects_dir) else {
        return ScanResult { window_tokens, window_oldest_ts, sessions, daily };
    };

    for proj in project_entries.flatten() {
        if !proj.path().is_dir() { continue }
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

                let ts = entry.timestamp.as_ref().and_then(parse_ts);
                let tokens = entry.message
                    .as_ref()
                    .and_then(|m| m.usage.as_ref())
                    .map(|u| u.input_tokens + u.output_tokens)
                    .unwrap_or(0);

                if let Some(t) = ts {
                    if first_ts.map(|f| t < f).unwrap_or(true) {
                        first_ts = Some(t);
                    }

                    if tokens > 0 {
                        // 5-hour rolling window
                        if t >= window_cutoff {
                            window_tokens += tokens;
                            if window_oldest_ts.map(|o| t < o).unwrap_or(true) {
                                window_oldest_ts = Some(t);
                            }
                        }

                        // Daily sparkline
                        let entry_day = t / 86_400_000;
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

    ScanResult { window_tokens, window_oldest_ts, sessions, daily }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn compute_app_data() -> AppData {
    let now = now_ms();
    const WEEK_MS: u64 = 7 * 86_400_000;
    const WINDOW_MS: u64 = 5 * 3_600_000;

    let window_cutoff = now.saturating_sub(WINDOW_MS);
    let scan = scan_all_project_jsonl(now, window_cutoff);

    let tokens_session = scan.window_tokens;

    let remaining_secs = scan.window_oldest_ts
        .map(|oldest| ((oldest + WINDOW_MS).saturating_sub(now) / 1000).min(5 * 3600))
        .unwrap_or(5 * 3600);

    let cutoff_week = now.saturating_sub(WEEK_MS);
    let tokens_this_week = scan.sessions.iter()
        .filter(|(first_ts, _)| *first_ts >= cutoff_week)
        .map(|(_, tokens)| tokens)
        .sum();

    AppData { tokens_session, tokens_this_week, remaining_secs, daily_tokens: scan.daily }
}
