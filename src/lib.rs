use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

pub fn statusline(_show_pr_status: bool) -> String {
    let input = read_input().unwrap_or_default();
    let api_status = fetch_api_status();

    let current_dir = input
        .get("workspace")
        .and_then(|w| w.get("current_dir"))
        .and_then(|d| d.as_str());

    let model = input
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(|d| d.as_str());

    let output_style = input
        .get("output_style")
        .and_then(|o| o.get("name"))
        .and_then(|n| n.as_str());

    let model_display = if let Some(model) = model {
        let style_suffix = match output_style {
            Some(style) => format!(" \x1b[90m({})\x1b[0m", style),
            None => String::new(),
        };
        format!("\x1b[38;5;208m{}{}", model, style_suffix)
    } else {
        String::new()
    };

    let current_dir = match current_dir {
        Some(dir) => dir,
        None => return format!("\x1b[31m\u{f071} missing workspace.current_dir\x1b[0m"),
    };

    let branch = if is_git_repo(current_dir) {
        get_git_branch(current_dir)
    } else {
        String::new()
    };

    let display_dir = format!("{} ", fish_shorten_path(current_dir));

    let lines_changed = if let Some(cost_obj) = input.get("cost") {
        let lines_added = cost_obj
            .get("total_lines_added")
            .and_then(|l| l.as_u64())
            .unwrap_or(0);
        let lines_removed = cost_obj
            .get("total_lines_removed")
            .and_then(|l| l.as_u64())
            .unwrap_or(0);

        if lines_added > 0 || lines_removed > 0 {
            format!(
                "(\x1b[32m+{}\x1b[0m \x1b[31m-{}\x1b[0m)",
                lines_added, lines_removed
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let as_u = |v: &serde_json::Value| -> Option<u64> {
        v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))
    };

    let mut row1_cells: Vec<String> = Vec::new();
    if let Some(ctx) = input.get("context_window") {
        let window_size = ctx
            .get("context_window_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(200000);
        let used = if let Some(current) = ctx.get("current_usage") {
            let it = current
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_creation = current
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = current
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            it + cache_creation + cache_read
        } else {
            0
        };
        let pct = if window_size > 0 {
            ((used as f64 * 100.0) / window_size as f64).min(100.0)
        } else {
            0.0
        };
        row1_cells.push(format_bar_cell(
            "\x1b[38;5;13m\u{f49b}\x1b[0m",
            pct,
            None,
        ));
    }

    if let Some(rate_limits) = input.get("rate_limits") {
        if let Some(five_hour) = rate_limits.get("five_hour") {
            if let Some(pct) = five_hour.get("used_percentage").and_then(|v| v.as_f64()) {
                let resets_at = five_hour.get("resets_at").and_then(|v| v.as_i64());
                row1_cells.push(format_bar_cell(
                    "\x1b[38;5;14m\u{f017}\x1b[0m",
                    pct,
                    resets_at,
                ));
            }
        }
        if let Some(seven_day) = rate_limits.get("seven_day") {
            if let Some(pct) = seven_day.get("used_percentage").and_then(|v| v.as_f64()) {
                let resets_at = seven_day.get("resets_at").and_then(|v| v.as_i64());
                row1_cells.push(format_bar_cell(
                    "\x1b[38;5;14m\u{f073}\x1b[0m",
                    pct,
                    resets_at,
                ));
            }
        }
    }

    let extra_usage_suffix = api_status.as_ref().and_then(|s| {
        let enabled = s.get("extra_usage_enabled").and_then(|v| v.as_bool())?;
        if !enabled {
            return None;
        }
        let used = s.get("extra_usage_used").and_then(&as_u)?;
        let limit = s.get("extra_usage_monthly_limit").and_then(&as_u)?;
        let pct = if limit > 0 {
            (used as f64 / limit as f64) * 100.0
        } else {
            0.0
        };
        let value_color = if pct >= 90.0 {
            "\x1b[31m"
        } else if pct >= 70.0 {
            "\x1b[38;5;208m"
        } else if pct >= 50.0 {
            "\x1b[33m"
        } else {
            "\x1b[32m"
        };
        Some(format!(
            " \x1b[90m• \x1b[38;5;208m\u{f155}\u{f155}\x1b[0m {}{}\x1b[90m/{}\x1b[0m",
            value_color,
            format_credits_compact(used),
            format_credits_compact(limit),
        ))
    });

    let cost_suffix = if let Some(extra) = extra_usage_suffix {
        extra
    } else if let Some(cost_obj) = input.get("cost") {
        if let Some(total_cost) = cost_obj.get("total_cost_usd").and_then(|c| c.as_f64()) {
            let cost_color = if total_cost < 5.0 {
                "\x1b[32m"
            } else if total_cost < 20.0 {
                "\x1b[33m"
            } else {
                "\x1b[31m"
            };
            format!(
                " \x1b[90m• \x1b[33m\u{f155}\x1b[0m {}{}\x1b[0m",
                cost_color,
                format_cost(total_cost)
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let model_str = if model_display.is_empty() {
        cost_suffix.clone()
    } else {
        format!(" \x1b[90m• \x1b[0m{}{}", model_display, cost_suffix)
    };

    if let Some(s) = api_status.as_ref() {
        let combined = s.get("combined_saved").and_then(&as_u).unwrap_or(0);
        let this_week = s.get("this_week_saved").and_then(&as_u).unwrap_or(0);
        let last_week = s.get("last_week_saved").and_then(&as_u).unwrap_or(0);
        let daily_avg = s.get("burn_rate_daily").and_then(&as_u).unwrap_or(0);
        if combined > 0 || this_week > 0 || last_week > 0 || daily_avg > 0 {
            row1_cells.push(format_savings_cell(
                "\x1b[38;5;10m\u{f0c7}\x1b[0m",
                combined,
            ));
            row1_cells.push(format_savings_cell("\x1b[90mwk\x1b[0m", this_week));
            if last_week > 0 {
                row1_cells.push(format_savings_cell("\x1b[90mprev\x1b[0m", last_week));
            }
            row1_cells.push(format_savings_cell("\x1b[90md/avg\x1b[0m", daily_avg));
        }
    }

    let separator = " \x1b[90m\u{2022}\x1b[0m ";

    let second_line = if row1_cells.is_empty() {
        String::new()
    } else {
        format!("\n{}", row1_cells.join(separator))
    };

    let third_line = String::new();

    if !branch.is_empty() {
        if display_dir.is_empty() {
            format!(
                "\x1b[38;5;12m\u{f02a2} \x1b[32m{}{}\x1b[0m{}{}{}",
                branch, lines_changed, model_str, second_line, third_line
            )
        } else {
            format!(
                "\x1b[36m{}\x1b[0m \x1b[38;5;12m\u{f02a2} \x1b[32m{}{}\x1b[0m{}{}{}",
                display_dir.trim_end(),
                branch,
                lines_changed,
                model_str,
                second_line,
                third_line
            )
        }
    } else {
        format!(
            "\x1b[36m{}\x1b[0m{}{}{}",
            display_dir.trim_end(),
            model_str,
            second_line,
            third_line
        )
    }
}

pub fn read_input() -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(serde_json::from_str(&buffer)?)
}


pub fn get_git_branch(working_dir: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(working_dir)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => String::new(),
    }
}

pub fn is_git_repo(dir: &str) -> bool {
    let output = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(dir)
        .output();

    matches!(output, Ok(output) if output.status.success() &&
             String::from_utf8_lossy(&output.stdout).trim() == "true")
}

pub fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

pub fn get_session_duration(transcript_path: Option<&str>) -> Option<String> {
    let transcript_path = transcript_path?;
    if !Path::new(transcript_path).exists() {
        return None;
    }

    let data = fs::read_to_string(transcript_path).ok()?;
    let lines: Vec<&str> = data.lines().filter(|l| !l.trim().is_empty()).collect();

    if lines.len() < 2 {
        return None;
    }

    let mut first_ts = None;
    let mut last_ts = None;

    for line in &lines {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(timestamp) = json.get("timestamp") {
                first_ts = Some(parse_timestamp(timestamp)?);
                break;
            }
        }
    }

    for line in lines.iter().rev() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(timestamp) = json.get("timestamp") {
                last_ts = Some(parse_timestamp(timestamp)?);
                break;
            }
        }
    }

    if let (Some(first), Some(last)) = (first_ts, last_ts) {
        let duration_ms = last - first;
        let hours = duration_ms / (1000 * 60 * 60);
        let minutes = (duration_ms % (1000 * 60 * 60)) / (1000 * 60);

        if hours > 0 {
            Some(format!("{}h{}m", hours, minutes))
        } else if minutes > 0 {
            Some(format!("{}m", minutes))
        } else {
            Some("<1m".to_string())
        }
    } else {
        None
    }
}

pub fn parse_timestamp(timestamp: &serde_json::Value) -> Option<i64> {
    if let Some(ts_str) = timestamp.as_str() {
        chrono::DateTime::parse_from_rfc3339(ts_str)
            .map(|dt| dt.timestamp_millis())
            .ok()
    } else {
        timestamp.as_i64()
    }
}

pub fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("{:.3}", cost)
    } else {
        format!("{:.2}", cost)
    }
}

pub fn format_tokens(tokens: u64) -> String {
    let k = tokens as f64 / 1000.0;
    if k >= 100.0 {
        format!("{}k", k.round() as u64)
    } else if k >= 10.0 {
        format!("{:.0}k", k)
    } else {
        format!("{:.1}k", k)
    }
}

pub fn format_count_compact(n: u64) -> String {
    let t = n as f64;
    if t >= 1_000_000_000.0 {
        format!("{:.1}B", t / 1_000_000_000.0)
    } else if t >= 1_000_000.0 {
        format!("{:.1}M", t / 1_000_000.0)
    } else if t >= 1_000.0 {
        format!("{:.1}k", t / 1_000.0)
    } else {
        format!("{}", n)
    }
}

pub fn format_credits_compact(n: u64) -> String {
    if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

pub fn display_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        width += 1;
    }
    width
}

pub fn pad_cell(s: &str, target: usize) -> String {
    let current = display_width(s);
    if current < target {
        format!("{}{}", s, " ".repeat(target - current))
    } else {
        s.to_string()
    }
}

fn pct_color(pct: f64) -> &'static str {
    if pct >= 90.0 {
        "\x1b[31m"
    } else if pct >= 70.0 {
        "\x1b[38;5;208m"
    } else if pct >= 50.0 {
        "\x1b[33m"
    } else {
        "\x1b[90m"
    }
}

pub fn format_bar_cell(label: &str, pct: f64, resets_at: Option<i64>) -> String {
    let pct = pct.clamp(0.0, 100.0);
    let color = pct_color(pct);

    let bar_width: usize = 5;
    let filled = (pct * bar_width as f64 / 100.0).round() as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar: String = "\u{2588}".repeat(filled) + &"\u{2591}".repeat(empty);

    let remaining_display = match resets_at {
        Some(ts) => {
            let now = chrono::Utc::now().timestamp();
            let secs = (ts - now).max(0);
            format!(" \x1b[90m{}\x1b[0m", format_duration_short(secs))
        }
        None => String::new(),
    };

    format!(
        "{} \x1b[90m{}\x1b[0m {}{}\x1b[90m%\x1b[0m{}",
        label,
        bar,
        color,
        pct.round() as u32,
        remaining_display
    )
}

pub fn format_credits_cell(label: &str, remaining: u64, limit: u64) -> String {
    let used = limit.saturating_sub(remaining);
    let pct = if limit > 0 {
        (used as f64 / limit as f64) * 100.0
    } else {
        0.0
    };
    let value_color = if pct >= 90.0 {
        "\x1b[31m"
    } else if pct >= 70.0 {
        "\x1b[38;5;208m"
    } else if pct >= 50.0 {
        "\x1b[33m"
    } else {
        "\x1b[32m"
    };
    format!(
        "\x1b[90m{:>5}\x1b[0m {}{}\x1b[90m/{}\x1b[0m",
        label,
        value_color,
        format_credits_compact(remaining),
        format_credits_compact(limit),
    )
}

pub fn format_cost_cell(label: &str, cost: f64) -> String {
    let cost_color = if cost < 5.0 {
        "\x1b[32m"
    } else if cost < 20.0 {
        "\x1b[33m"
    } else {
        "\x1b[31m"
    };
    format!(
        "\x1b[90m{:>5}\x1b[0m {}${}\x1b[0m",
        label,
        cost_color,
        format_cost(cost)
    )
}

pub fn format_savings_cell(label: &str, value: u64) -> String {
    format!("{} \x1b[32m{}\x1b[0m", label, format_count_compact(value))
}


pub fn fetch_api_status() -> Option<serde_json::Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(300))
        .build()
        .ok()?;
    let resp = client
        .get("http://localhost:8095/api/status")
        .send()
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<serde_json::Value>().ok()
}

pub fn format_duration_short(total_secs: i64) -> String {
    let secs = total_secs.max(0);
    if secs >= 86400 {
        format!("{}d", secs / 86400)
    } else if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

pub fn fish_shorten_path(path: &str) -> String {
    let home = home_dir();
    let path = path.replace(&home, "~");

    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= 1 {
        return path;
    }

    let shortened: Vec<String> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| {
            if i == parts.len() - 1 || part.is_empty() || *part == "~" {
                part.to_string()
            } else if part.starts_with('.') && part.len() > 1 {
                format!(".{}", part.chars().nth(1).unwrap_or_default())
            } else {
                part.chars()
                    .next()
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            }
        })
        .collect();

    shortened.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bar_cell_with_label() {
        let result = format_bar_cell("ctx", 35.0, None);
        assert!(result.contains("ctx"));
        assert!(result.contains("35"));
        assert!(!result.contains("(")); // no parens around remaining
    }

    #[test]
    fn test_format_bar_cell_with_resets_no_parens() {
        let future = chrono::Utc::now().timestamp() + 7200;
        let result = format_bar_cell("5h", 35.0, Some(future));
        assert!(result.contains("5h"));
        assert!(result.contains("2h"));
        assert!(!result.contains("("));
        assert!(!result.contains(")"));
    }

    #[test]
    fn test_format_bar_cell_five_segment_bar() {
        let result = format_bar_cell("ctx", 60.0, None);
        // 60% of 5 blocks = 3 filled
        assert_eq!(result.matches('\u{2588}').count(), 3);
        assert_eq!(result.matches('\u{2591}').count(), 2);
    }

    #[test]
    fn test_format_bar_cell_color_critical() {
        let result = format_bar_cell("7d", 95.0, None);
        assert!(result.contains("\x1b[31m"));
    }

    #[test]
    fn test_format_credits_cell_sub_thousand() {
        let result = format_credits_cell("r/usg", 500, 800);
        assert!(result.contains("r/usg"));
        assert!(result.contains("500"));
        assert!(result.contains("800"));
        assert!(!result.contains("k"));
    }

    #[test]
    fn test_format_credits_cell_thousands() {
        let result = format_credits_cell("r/usg", 6877, 17000);
        assert!(result.contains("r/usg"));
        assert!(result.contains("6.9k"));
        assert!(result.contains("17.0k"));
    }

    #[test]
    fn test_format_credits_cell_remaining_not_used() {
        // used = limit - remaining; 10123 used of 17000 → remaining 6877
        let result = format_credits_cell("r/usg", 6877, 17000);
        assert!(result.contains("6.9k"));
        assert!(!result.contains("10.1k")); // should NOT show used amount
    }

    #[test]
    fn test_format_credits_compact() {
        assert_eq!(format_credits_compact(0), "0");
        assert_eq!(format_credits_compact(999), "999");
        assert_eq!(format_credits_compact(1_000), "1.0k");
        assert_eq!(format_credits_compact(10_123), "10.1k");
        assert_eq!(format_credits_compact(17_000), "17.0k");
    }

    #[test]
    fn test_display_width_strips_ansi() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("\x1b[90mhello\x1b[0m"), 5);
        assert_eq!(display_width("\x1b[38;5;208m  ctx\x1b[0m"), 5);
        assert_eq!(display_width("\x1b[32mfoo\x1b[0m \x1b[33mbar\x1b[0m"), 7);
    }

    #[test]
    fn test_pad_cell_adds_trailing_space() {
        assert_eq!(pad_cell("ab", 5), "ab   ");
        assert_eq!(pad_cell("ab", 2), "ab");
        assert_eq!(pad_cell("ab", 1), "ab"); // no truncation
    }

    #[test]
    fn test_pad_cell_respects_ansi() {
        let input = "\x1b[32mab\x1b[0m";
        let padded = pad_cell(input, 5);
        assert_eq!(display_width(&padded), 5);
        assert!(padded.ends_with("   "));
    }

    #[test]
    fn test_format_cost_cell() {
        let result = format_cost_cell("cost", 7.50);
        assert!(result.contains("cost"));
        assert!(result.contains("$"));
        assert!(result.contains("7.50"));
    }

    #[test]
    fn test_format_savings_cell() {
        let result = format_savings_cell("save", 141_902_121);
        assert!(result.contains("save"));
        assert!(result.contains("141.9M"));
    }

    #[test]
    fn test_format_savings_cell_d_avg_label() {
        let result = format_savings_cell("d/avg", 18_000_000);
        assert!(result.contains("d/avg"));
        assert!(result.contains("18.0M"));
    }

    #[test]
    fn test_format_duration_short() {
        assert_eq!(format_duration_short(432000), "5d");
        assert_eq!(format_duration_short(90000), "1d");
        assert_eq!(format_duration_short(86400), "1d");
        assert_eq!(format_duration_short(7200), "2h");
        assert_eq!(format_duration_short(3660), "1h");
        assert_eq!(format_duration_short(3600), "1h");
        assert_eq!(format_duration_short(1800), "30m");
        assert_eq!(format_duration_short(60), "1m");
        assert_eq!(format_duration_short(59), "59s");
        assert_eq!(format_duration_short(30), "30s");
        assert_eq!(format_duration_short(0), "0s");
        assert_eq!(format_duration_short(-5), "0s");
    }

    #[test]
    fn test_format_count_compact() {
        assert_eq!(format_count_compact(0), "0");
        assert_eq!(format_count_compact(999), "999");
        assert_eq!(format_count_compact(1_000), "1.0k");
        assert_eq!(format_count_compact(10_123), "10.1k");
        assert_eq!(format_count_compact(17_000), "17.0k");
        assert_eq!(format_count_compact(1_000_000), "1.0M");
        assert_eq!(format_count_compact(127_838_046), "127.8M");
        assert_eq!(format_count_compact(141_902_121), "141.9M");
        assert_eq!(format_count_compact(2_500_000_000), "2.5B");
    }

}
