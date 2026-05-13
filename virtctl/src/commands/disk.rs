use anyhow::{Context, Result, bail};

use crate::commands::vm as vm_cmd;
use crate::docker;
use crate::error::ValidationError;
use crate::ui;
use crate::util;

pub fn attach(name: &str, size: Option<&str>) -> Result<()> {
    let size_str = size.context("--size is required for action=attach")?;
    let size_bytes = util::parse_size(size_str)?;
    vm_cmd::load_managed_vm(name)?;
    ensure_running(name)?;
    vm_cmd::wait_for_exec_ready(name)?;
    let host_avail = util::available_bytes_on_host("/")?;
    if size_bytes > host_avail {
        return Err(ValidationError::InsufficientDiskSpace {
            requested_bytes: size_bytes,
            available_bytes: host_avail,
        }
        .into());
    }
    let index = next_free_index(name)?;
    let file_path = format!("/extra-disk{index}.img");
    create_disk_file(name, &file_path, size_str)?;
    let loop_dev = losetup_attach(name, &file_path)?;
    ui::ok(format!(
        "attached {} ({}) to VM '{}'",
        ui::cyan(loop_dev.trim_start_matches("/dev/")),
        size_str,
        ui::cyan(name)
    ));
    Ok(())
}

pub fn remove(name: &str, disk_name: Option<&str>) -> Result<()> {
    let raw = disk_name.context("--disk-name is required for action=remove")?;
    vm_cmd::load_managed_vm(name)?;
    ensure_running(name)?;
    let loop_dev = if raw.starts_with("/dev/") {
        raw.to_string()
    } else {
        format!("/dev/{raw}")
    };
    let back_file = back_file_for(name, &loop_dev)?;
    if !is_managed_disk_backing(&back_file) {
        bail!("'{loop_dev}' is not a virtctl-managed mock disk (backing: {back_file})");
    }
    let output = docker::container_exec_capture(name, &["losetup", "-d", &loop_dev])?;
    if !output.status.success() {
        bail!(
            "losetup -d {loop_dev} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let rm_output = docker::container_exec_capture(name, &["rm", "-f", &back_file])?;
    if !rm_output.status.success() {
        bail!(
            "failed to remove backing file '{back_file}': {}",
            String::from_utf8_lossy(&rm_output.stderr).trim()
        );
    }
    ui::ok(format!(
        "removed {} (was {})",
        ui::cyan(loop_dev.trim_start_matches("/dev/")),
        back_file
    ));
    Ok(())
}

pub fn list(name: &str) -> Result<()> {
    vm_cmd::load_managed_vm(name)?;
    ensure_running(name)?;
    let disks = list_managed_disks(name)?;
    let rows: Vec<Vec<String>> = disks
        .into_iter()
        .map(|(dev, size, back)| vec![dev.trim_start_matches("/dev/").to_string(), size, back])
        .collect();
    util::print_table(&["DEVICE", "SIZE", "BACKING"], &rows);
    Ok(())
}

pub fn create_disk_file(name: &str, path: &str, size: &str) -> Result<()> {
    let arg = format!("--size={size}");
    let output = docker::container_exec_capture(name, &["truncate", &arg, path])?;
    if !output.status.success() {
        bail!(
            "truncate {} failed: {}",
            path,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub fn losetup_attach(name: &str, path: &str) -> Result<String> {
    let output = docker::container_exec_capture(name, &["losetup", "-fP", "--show", path])?;
    if !output.status.success() {
        bail!(
            "losetup -fP failed for {}: {}",
            path,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn next_free_index(name: &str) -> Result<u32> {
    for index in 1..1024 {
        let path = format!("/extra-disk{index}.img");
        let probe = docker::container_exec_capture(name, &["test", "-e", &path])?;
        if !probe.status.success() {
            return Ok(index);
        }
    }
    bail!("could not find a free extra-disk index slot")
}

fn back_file_for(name: &str, loop_dev: &str) -> Result<String> {
    let output = docker::container_exec_capture(name, &["losetup", "-nO", "BACK-FILE", loop_dev])?;
    if !output.status.success() {
        bail!(
            "losetup query for {loop_dev} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn list_managed_disks(name: &str) -> Result<Vec<(String, String, String)>> {
    let output =
        docker::container_exec_capture(name, &["losetup", "-nO", "NAME,BACK-FILE", "--list"])?;
    if !output.status.success() {
        bail!(
            "losetup --list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let dev = parts[0].to_string();
        let back = parts[1..].join(" ");
        if !is_managed_disk_backing(&back) {
            continue;
        }
        let size = loop_device_size(name, &dev).unwrap_or_else(|_| "?".into());
        result.push((dev, size, back));
    }
    Ok(result)
}

fn loop_device_size(vm: &str, dev: &str) -> Result<String> {
    let output = docker::container_exec_capture(vm, &["lsblk", "-nbdo", "SIZE", dev])?;
    if !output.status.success() {
        bail!(
            "lsblk {dev} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let bytes: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .with_context(|| format!("invalid size returned for '{dev}'"))?;
    Ok(util::format_bytes(bytes))
}

fn is_managed_disk_backing(path: &str) -> bool {
    if !path.starts_with("/extra-disk") {
        return false;
    }
    std::path::Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("img"))
}

fn ensure_running(name: &str) -> Result<()> {
    if !docker::container_running(name)? {
        bail!("VM '{name}' is not running; start it first");
    }
    Ok(())
}
