use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // Re-run this script whenever the git HEAD changes (new commit or branch switch).
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "dev".to_string());

    let datetime = build_datetime();

    println!("cargo:rustc-env=APP_VERSION={git_hash}@{datetime}");
}

fn build_datetime() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();

    // Break down the Unix timestamp into date/time components.
    let mut remaining = secs;

    let seconds = remaining % 60;
    remaining /= 60;
    let minutes = remaining % 60;
    remaining /= 60;
    let hours = remaining % 24;
    remaining /= 24;

    // Days since epoch (1970-01-01). Compute year/month/day via the
    // proleptic Gregorian calendar algorithm.
    let mut days = remaining as u32;

    // 400-year cycles
    let cycles_400 = days / 146097;
    days %= 146097;

    // 100-year cycles
    let cycles_100 = (days / 36524).min(3);
    days -= cycles_100 * 36524;

    // 4-year cycles
    let cycles_4 = days / 1461;
    days %= 1461;

    // Remaining years
    let remaining_years = (days / 365).min(3);
    days -= remaining_years * 365;

    let year = cycles_400 * 400 + cycles_100 * 100 + cycles_4 * 4 + remaining_years + 1970;

    // Determine month and day-of-month. Days here is 0-based day within the year.
    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let month_days: [u32; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut month = 1u32;
    let mut day = days + 1;
    for &md in &month_days {
        if day <= md {
            break;
        }
        day -= md;
        month += 1;
    }

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}
