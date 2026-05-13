use anyhow::{Context, Result, bail};
use ipnet::Ipv4Net;

use crate::commands::network as network_cmd;
use crate::commands::vm as vm_cmd;
use crate::docker::{self, VmCreateSpec};
use crate::error::validate_name;
use crate::ui;
use crate::util;

pub fn backup(vm_name: &str, name: Option<&str>) -> Result<()> {
    let record = vm_cmd::load_managed_vm(vm_name)?;
    let tag = match name {
        Some(value) => {
            validate_name(value)?;
            value.to_string()
        }
        None => util::timestamp_tag(),
    };
    let image_tag = snapshot_image_tag(vm_name, &tag);
    if docker::image_exists(&image_tag)? {
        bail!("snapshot '{image_tag}' already exists for VM '{vm_name}'");
    }
    docker::image_commit(&record.name, &image_tag, vm_name)?;
    ui::ok(format!("snapshot created: {}", ui::cyan(&image_tag)));
    Ok(())
}

pub fn list(vm_name: &str) -> Result<()> {
    vm_cmd::load_managed_vm(vm_name)?;
    let entries = docker::list_snapshots_for(vm_name)?;
    let rows: Vec<Vec<String>> = entries
        .into_iter()
        .map(|(name, id, created)| vec![name, id, format_unix_timestamp(created)])
        .collect();
    util::print_table(&["SNAPSHOT", "IMAGE-ID", "CREATED"], &rows);
    Ok(())
}

pub fn restore(vm_name: &str, snapshot: &str) -> Result<()> {
    let record = vm_cmd::load_managed_vm(vm_name)?;
    let snapshot_tag = normalize_snapshot_ref(vm_name, snapshot);
    if !docker::image_exists(&snapshot_tag)? {
        bail!("snapshot '{snapshot_tag}' not found");
    }
    let value = docker::image_inspect(&snapshot_tag)?;
    let labels = value
        .pointer("/Config/Labels")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let owner = labels
        .get(docker::LABEL_SNAPSHOT_VM)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if owner != vm_name {
        bail!("snapshot '{snapshot_tag}' does not belong to VM '{vm_name}' (owner: '{owner}')");
    }
    let net_record = network_cmd::load_managed_record(&record.network)?;
    let subnet: Ipv4Net = net_record.subnet.parse().context("invalid stored subnet")?;
    let cidr = format!("{}/{}", record.ipv4, subnet.prefix_len());
    let dns = net_record.dns.join(",");
    docker::container_remove(vm_name, true)?;
    let spec = VmCreateSpec {
        name: vm_name,
        image: &snapshot_tag,
        network: &record.network,
        ipv4: &record.ipv4,
        ipv4_cidr: &cidr,
        gateway: &net_record.gateway,
        dns: &dns,
        cpus: record.cpus.max(0.1),
        memory_bytes: record.memory_bytes.max(64 * 1024 * 1024),
        extra_disks_spec: &record.extra_disks_spec,
        iac_source: record.iac_source.as_deref(),
    };
    docker::container_run_vm(&spec)?;
    vm_cmd::apply_extra_disks_from_spec(vm_name, &record.extra_disks_spec)?;
    ui::ok(format!(
        "VM '{}' restored from snapshot '{}'",
        ui::cyan(vm_name),
        ui::cyan(&snapshot_tag)
    ));
    Ok(())
}

pub fn destroy(vm_name: &str, snapshot: &str, yes: bool) -> Result<()> {
    let snapshot_tag = normalize_snapshot_ref(vm_name, snapshot);
    if !docker::image_exists(&snapshot_tag)? {
        bail!("snapshot '{snapshot_tag}' not found");
    }
    let value = docker::image_inspect(&snapshot_tag)?;
    let labels = value
        .pointer("/Config/Labels")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let owner = labels
        .get(docker::LABEL_SNAPSHOT_VM)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if owner != vm_name {
        bail!("snapshot '{snapshot_tag}' does not belong to VM '{vm_name}' (owner: '{owner}')");
    }
    if !yes {
        let prompt = format!("destroy snapshot '{snapshot_tag}'?");
        if !util::confirm(&prompt)? {
            ui::warn("aborted");
            return Ok(());
        }
    }
    docker::image_remove(&snapshot_tag)?;
    ui::ok(format!("snapshot '{}' destroyed", ui::cyan(&snapshot_tag)));
    Ok(())
}

pub fn snapshot_image_tag(vm_name: &str, tag: &str) -> String {
    format!("virtctl-snapshot/{vm_name}:{tag}")
}

pub fn normalize_snapshot_ref(vm_name: &str, value: &str) -> String {
    if value.contains(':') || value.contains('/') {
        value.to_string()
    } else {
        snapshot_image_tag(vm_name, value)
    }
}

fn format_unix_timestamp(secs: u64) -> String {
    if secs == 0 {
        return "-".into();
    }
    let days = secs / 86_400;
    let mut year: u64 = 1970;
    let mut remaining_days = days;
    loop {
        let year_days: u64 = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        year += 1;
    }
    let leap = is_leap_year(year);
    let month_days: [u64; 12] = [
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
    let mut month: u64 = 1;
    for md in month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;
    let secs_of_day = secs % 86_400;
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}Z")
}

const fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
