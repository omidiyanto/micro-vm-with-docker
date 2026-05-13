use anyhow::{Context, Result, bail};
use std::fmt::Write as _;
use std::io::{self, BufRead, Write};
use std::process::Command;

use crate::error::ValidationError;

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;
const TIB: u64 = GIB * 1024;

pub fn parse_memory(input: &str) -> Result<u64, ValidationError> {
    parse_size_internal(input)
        .map_err(|reason| ValidationError::InvalidMemory(input.to_string(), reason))
}

pub fn parse_size(input: &str) -> Result<u64, ValidationError> {
    parse_size_internal(input)
        .map_err(|reason| ValidationError::InvalidSize(input.to_string(), reason))
}

const SIZE_UNITS: &[(&str, u64)] = &[
    ("TB", TIB),
    ("GB", GIB),
    ("MB", MIB),
    ("KB", KIB),
    ("T", TIB),
    ("G", GIB),
    ("M", MIB),
    ("K", KIB),
    ("B", 1),
];

fn parse_size_internal(raw: &str) -> Result<u64, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("value is empty".to_string());
    }
    let upper = trimmed.to_ascii_uppercase();
    let (num_part, multiplier) = SIZE_UNITS
        .iter()
        .find_map(|(suffix, mult)| upper.strip_suffix(suffix).map(|rest| (rest, *mult)))
        .unwrap_or((upper.as_str(), MIB));
    let num: u64 = num_part
        .trim()
        .parse()
        .map_err(|_| format!("'{num_part}' is not a non-negative integer"))?;
    if num == 0 {
        return Err("value must be greater than zero".to_string());
    }
    num.checked_mul(multiplier)
        .ok_or_else(|| "size value overflows u64".to_string())
}

pub fn format_bytes(bytes: u64) -> String {
    if bytes >= TIB {
        format_fractional(bytes, TIB, 'T')
    } else if bytes >= GIB {
        format_fractional(bytes, GIB, 'G')
    } else if bytes >= MIB {
        format_fractional(bytes, MIB, 'M')
    } else if bytes >= KIB {
        format_fractional(bytes, KIB, 'K')
    } else {
        format!("{bytes}B")
    }
}

fn format_fractional(bytes: u64, unit: u64, suffix: char) -> String {
    if bytes.is_multiple_of(unit) {
        format!("{}{suffix}", bytes / unit)
    } else {
        let whole = bytes / unit;
        let hundredths = (bytes % unit) * 100 / unit;
        format!("{whole}.{hundredths:02}{suffix}")
    }
}

pub fn parse_cpu(input: &str) -> Result<f64, ValidationError> {
    let trimmed = input.trim();
    let value: f64 = trimmed
        .parse()
        .map_err(|_| ValidationError::InvalidCpu(input.to_string()))?;
    if !value.is_finite() || value <= 0.0 || value > 1024.0 {
        return Err(ValidationError::InvalidCpu(input.to_string()));
    }
    Ok(value)
}

pub fn format_cpu(cpus: f64) -> String {
    if (cpus.fract()).abs() < f64::EPSILON {
        format!("{cpus:.0}")
    } else {
        format!("{cpus:.2}")
    }
}

pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (idx, cell) in row.iter().enumerate() {
            if idx < widths.len() && cell.len() > widths[idx] {
                widths[idx] = cell.len();
            }
        }
    }
    let mut line = String::new();
    for (idx, header) in headers.iter().enumerate() {
        if idx > 0 {
            line.push_str("  ");
        }
        let width = widths[idx];
        let _ = write!(line, "{header:<width$}");
    }
    println!("{}", line.trim_end());
    for row in rows {
        let mut line = String::new();
        for (idx, cell) in row.iter().enumerate() {
            if idx > 0 {
                line.push_str("  ");
            }
            let width = widths.get(idx).copied().unwrap_or(cell.len());
            let _ = write!(line, "{cell:<width$}");
        }
        println!("{}", line.trim_end());
    }
}

pub fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N]: ");
    io::stdout().flush().context("failed to flush stdout")?;
    let mut buf = String::new();
    io::stdin()
        .lock()
        .read_line(&mut buf)
        .context("failed to read confirmation from stdin")?;
    let answer = buf.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

pub fn available_bytes_on_host(path: &str) -> Result<u64> {
    let output = Command::new("df")
        .args(["-B1", "--output=avail", path])
        .output()
        .context("failed to execute df to query available disk space")?;
    if !output.status.success() {
        bail!(
            "df failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let second_line = stdout
        .lines()
        .nth(1)
        .context("unexpected df output: missing data row")?;
    let value: u64 = second_line
        .trim()
        .parse()
        .context("unexpected df output: not a number")?;
    Ok(value)
}

pub fn timestamp_tag() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    format!("snap-{secs}")
}
