use anyhow::{Context, Result, bail};
use ipnet::Ipv4Net;
use std::net::Ipv4Addr;
use std::thread::sleep;
use std::time::Duration;

use crate::cli::{VmCreateArgs, VmModifyArgs};
use crate::commands::network as network_cmd;
use crate::docker::{self, VmCreateSpec};
use crate::error::{ValidationError, validate_name};
use crate::ui;
use crate::util;

pub fn create(args: &VmCreateArgs, iac_source: Option<&str>) -> Result<()> {
    validate_name(&args.name)?;
    let cpus = util::parse_cpu(&args.cpu.to_string())?;
    let memory_bytes = util::parse_memory(&args.ram)?;
    for disk in &args.extra_disk {
        let _ = util::parse_size(disk)?;
    }
    network_cmd::ensure_managed_network(&args.network)?;
    let net_record = network_cmd::load_managed_record(&args.network)?;
    let subnet: Ipv4Net = net_record
        .subnet
        .parse()
        .context("invalid subnet on stored network")?;
    let ipv4 = resolve_ipv4(&args.ipv4, &args.network, subnet, &net_record.gateway)?;
    if docker::container_exists(&args.name)? {
        bail!("VM '{}' already exists", args.name);
    }
    docker::ensure_image(&args.image)?;
    let ipv4_str = ipv4.to_string();
    let cidr = format!("{}/{}", ipv4_str, subnet.prefix_len());
    let dns = net_record.dns.join(",");
    let spec = VmCreateSpec {
        name: &args.name,
        image: &args.image,
        network: &args.network,
        ipv4: &ipv4_str,
        ipv4_cidr: &cidr,
        gateway: &net_record.gateway,
        dns: &dns,
        cpus,
        memory_bytes,
        extra_disks_spec: &args.extra_disk,
        iac_source,
    };
    docker::container_run_vm(&spec)?;
    apply_extra_disks_from_spec(&args.name, &args.extra_disk)?;
    ui::ok(format!(
        "VM '{}' created (ip={} cpu={} ram={})",
        ui::cyan(&args.name),
        ipv4_str,
        util::format_cpu(cpus),
        util::format_bytes(memory_bytes)
    ));
    Ok(())
}

pub fn list() -> Result<()> {
    let records = docker::list_managed_vms()?;
    let mut rows: Vec<Vec<String>> = records
        .iter()
        .map(|r| {
            vec![
                r.name.clone(),
                r.status.clone(),
                if r.ipv4.is_empty() {
                    "-".into()
                } else {
                    r.ipv4.clone()
                },
                if r.network.is_empty() {
                    "-".into()
                } else {
                    r.network.clone()
                },
                util::format_cpu(r.cpus),
                util::format_bytes(r.memory_bytes),
                r.iac_source.clone().unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    rows.sort_by(|a, b| a[0].cmp(&b[0]));
    util::print_table(
        &["NAME", "STATE", "IP", "NETWORK", "CPU", "RAM", "SOURCE"],
        &rows,
    );
    Ok(())
}

pub fn show(name: &str) -> Result<()> {
    let record = load_managed_vm(name)?;
    println!("name     : {}", record.name);
    println!("state    : {}", record.status);
    println!(
        "ip       : {}",
        if record.ipv4.is_empty() {
            "-"
        } else {
            record.ipv4.as_str()
        }
    );
    println!("network  : {}", record.network);
    println!("cpu      : {}", util::format_cpu(record.cpus));
    println!("ram      : {}", util::format_bytes(record.memory_bytes));
    println!("image    : {}", record.image);
    println!(
        "disks    : {}",
        if record.extra_disks_spec.is_empty() {
            "-".into()
        } else {
            record.extra_disks_spec.join(",")
        }
    );
    println!(
        "source   : {}",
        record.iac_source.unwrap_or_else(|| "-".into())
    );
    Ok(())
}

pub fn modify(args: &VmModifyArgs) -> Result<()> {
    let record = load_managed_vm(&args.name)?;
    let new_cpus = match args.cpu {
        Some(value) => util::parse_cpu(&value.to_string())?,
        None => record.cpus,
    };
    let new_mem = match &args.ram {
        Some(value) => util::parse_memory(value)?,
        None => record.memory_bytes,
    };
    if (new_cpus - record.cpus).abs() < f64::EPSILON && new_mem == record.memory_bytes {
        ui::info(format!(
            "no changes to apply for VM '{}'",
            ui::cyan(&args.name)
        ));
        return Ok(());
    }
    docker::container_update_resources(&args.name, new_cpus, new_mem)?;
    ui::ok(format!(
        "VM '{}' updated (cpu={} ram={})",
        ui::cyan(&args.name),
        util::format_cpu(new_cpus),
        util::format_bytes(new_mem)
    ));
    Ok(())
}

pub fn destroy(name: &str, yes: bool) -> Result<()> {
    load_managed_vm(name)?;
    if !yes {
        ui::warn("Mock VMs do NOT have persistent storage.");
        ui::warn(format!(
            "All data inside '{}' will be PERMANENTLY DELETED.",
            ui::cyan(name)
        ));
        let prompt = format!("destroy VM '{name}'?");
        if !util::confirm(&prompt)? {
            ui::warn("aborted");
            return Ok(());
        }
    }
    docker::container_remove(name, true)?;
    ui::ok(format!("VM '{}' destroyed", ui::cyan(name)));
    Ok(())
}

pub fn console(name: &str) -> Result<()> {
    load_managed_vm(name)?;
    if !docker::container_running(name)? {
        bail!("VM '{name}' is not running");
    }
    ui::info(format!(
        "connecting to console of '{}' (press ^D to disconnect)",
        ui::cyan(name)
    ));
    let code = docker::container_exec_interactive(name, &["login"])?;
    if code != 0 {
        bail!("console session exited with status {code}");
    }
    Ok(())
}

pub fn start(name: &str) -> Result<()> {
    load_managed_vm(name)?;
    if docker::container_running(name)? {
        ui::info(format!("VM '{}' is already running", ui::cyan(name)));
        return Ok(());
    }
    docker::container_start(name)?;
    ui::ok(format!("VM '{}' started", ui::cyan(name)));
    Ok(())
}

pub fn stop(name: &str) -> Result<()> {
    load_managed_vm(name)?;
    if !docker::container_running(name)? {
        ui::info(format!("VM '{}' is already stopped", ui::cyan(name)));
        return Ok(());
    }
    docker::container_stop(name)?;
    ui::ok(format!("VM '{}' stopped", ui::cyan(name)));
    Ok(())
}

pub fn restart(name: &str) -> Result<()> {
    load_managed_vm(name)?;
    docker::container_restart(name)?;
    ui::ok(format!("VM '{}' restarted", ui::cyan(name)));
    Ok(())
}

pub fn load_managed_vm(name: &str) -> Result<docker::VmRecord> {
    if !docker::container_exists(name)? {
        bail!("VM '{name}' does not exist");
    }
    let value = docker::container_inspect(name)?;
    let labels = value
        .pointer("/Config/Labels")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let managed = labels
        .get(docker::MANAGED_LABEL_KEY)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let kind = labels
        .get(docker::LABEL_KIND)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if managed != docker::MANAGED_LABEL_VALUE || kind != docker::KIND_VM {
        bail!("container '{name}' is not a virtctl-managed VM");
    }
    Ok(docker::parse_vm_record(name, &value))
}

pub fn resolve_ipv4(
    requested: &str,
    network: &str,
    subnet: Ipv4Net,
    gateway: &str,
) -> Result<Ipv4Addr> {
    if requested.eq_ignore_ascii_case("DHCP") {
        return allocate_free_ip(subnet, network, gateway);
    }
    let addr: Ipv4Addr = requested.parse().map_err(|_| {
        ValidationError::InvalidSubnet(requested.to_string(), "not a valid IPv4 address".into())
    })?;
    if !subnet.contains(&addr) {
        return Err(ValidationError::IpNotInSubnet {
            ip: requested.into(),
            subnet: subnet.to_string(),
        }
        .into());
    }
    let used = docker::list_used_ips_in_network(network)?;
    if used.iter().any(|u| u == &addr.to_string()) {
        return Err(ValidationError::IpAlreadyUsed(requested.into()).into());
    }
    Ok(addr)
}

pub fn allocate_free_ip(subnet: Ipv4Net, network: &str, gateway: &str) -> Result<Ipv4Addr> {
    let used = docker::list_used_ips_in_network(network)?;
    let gw_addr: Ipv4Addr = gateway.parse().context("invalid stored gateway")?;
    for host in subnet.hosts() {
        if host == gw_addr {
            continue;
        }
        if used.iter().any(|u| u == &host.to_string()) {
            continue;
        }
        return Ok(host);
    }
    Err(ValidationError::NoFreeIp(subnet.to_string()).into())
}

pub fn apply_extra_disks_from_spec(name: &str, disks: &[String]) -> Result<()> {
    if disks.is_empty() {
        return Ok(());
    }
    wait_for_exec_ready(name)?;
    let host_avail = util::available_bytes_on_host("/")?;
    let mut planned: u64 = 0;
    for disk in disks {
        planned = planned.saturating_add(util::parse_size(disk)?);
    }
    if planned > host_avail {
        return Err(ValidationError::InsufficientDiskSpace {
            requested_bytes: planned,
            available_bytes: host_avail,
        }
        .into());
    }
    for (idx, size) in disks.iter().enumerate() {
        let file = format!("/extra-disk{}.img", idx + 1);
        crate::commands::disk::create_disk_file(name, &file, size)?;
        let loop_dev = crate::commands::disk::losetup_attach(name, &file)?;
        println!(
            "attached extra disk: {} ({}) -> {}",
            file,
            size,
            loop_dev.trim_start_matches("/dev/")
        );
    }
    Ok(())
}

pub fn wait_for_exec_ready(name: &str) -> Result<()> {
    for _ in 0..50 {
        let output = docker::container_exec_capture(name, &["true"])?;
        if output.status.success() {
            return Ok(());
        }
        sleep(Duration::from_millis(200));
    }
    bail!("timeout waiting for container '{name}' to become exec-ready");
}
