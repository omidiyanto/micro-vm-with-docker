use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::process::{Command, Output, Stdio};

pub const MANAGED_LABEL_KEY: &str = "managed-by";
pub const MANAGED_LABEL_VALUE: &str = "virtctl";
pub const MANAGED_LABEL_FILTER: &str = "managed-by=virtctl";
pub const LABEL_KIND: &str = "virtctl.kind";
pub const LABEL_NETWORK_DNS: &str = "virtctl.dns";
pub const LABEL_NETWORK_NAME: &str = "virtctl.network";
pub const LABEL_IAC_SOURCE: &str = "virtctl.iac-source";
pub const LABEL_EXTRA_DISKS_SPEC: &str = "virtctl.extra-disks-spec";
pub const LABEL_IMAGE: &str = "virtctl.image";
pub const LABEL_SNAPSHOT_VM: &str = "virtctl.snapshot-of";
pub const LABEL_SNAPSHOT_CREATED_AT: &str = "virtctl.snapshot-created-at";

pub const KIND_NETWORK: &str = "network";
pub const KIND_VM: &str = "vm";
pub const KIND_SNAPSHOT: &str = "snapshot";

#[derive(Debug, Clone)]
pub struct VmCreateSpec<'a> {
    pub name: &'a str,
    pub image: &'a str,
    pub network: &'a str,
    pub ipv4: &'a str,
    pub ipv4_cidr: &'a str,
    pub gateway: &'a str,
    pub dns: &'a str,
    pub cpus: f64,
    pub memory_bytes: u64,
    pub extra_disks_spec: &'a [String],
    pub iac_source: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct NetworkCreateSpec<'a> {
    pub name: &'a str,
    pub subnet: &'a str,
    pub gateway: &'a str,
    pub dns: &'a [String],
    pub iac_source: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct NetworkRecord {
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    pub dns: Vec<String>,
    pub iac_source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VmRecord {
    pub name: String,
    pub image: String,
    pub network: String,
    pub ipv4: String,
    pub cpus: f64,
    pub memory_bytes: u64,
    pub status: String,
    pub extra_disks_spec: Vec<String>,
    pub iac_source: Option<String>,
}

pub fn binary_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn daemon_running() -> bool {
    Command::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn run(args: &[&str]) -> Result<Output> {
    let output = Command::new("docker")
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn `docker {}`", args.join(" ")))?;
    Ok(output)
}

fn run_ok(args: &[&str]) -> Result<String> {
    let output = run(args)?;
    if !output.status.success() {
        bail!(
            "docker {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn pull_image(image: &str) -> Result<()> {
    let spinner = crate::ui::Spinner::start(format!("pulling image '{image}'"));
    let status = Command::new("docker")
        .args(["pull", "--quiet", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let status = match status {
        Ok(s) => s,
        Err(err) => {
            spinner.finish_fail(format!("failed to invoke docker pull '{image}'"));
            return Err(err).with_context(|| format!("failed to invoke docker pull {image}"));
        }
    };
    if !status.success() {
        spinner.finish_fail(format!("docker pull '{image}' failed"));
        bail!("docker pull {image} failed");
    }
    spinner.finish_ok(format!("image '{image}' pulled"));
    Ok(())
}

pub fn image_exists_locally(image: &str) -> Result<bool> {
    let output = run(&["image", "inspect", image])?;
    Ok(output.status.success())
}

pub fn ensure_image(image: &str) -> Result<()> {
    if image_exists_locally(image)? {
        return Ok(());
    }
    pull_image(image)
}

pub fn network_exists(name: &str) -> Result<bool> {
    let output = run(&["network", "inspect", name])?;
    Ok(output.status.success())
}

pub fn network_inspect(name: &str) -> Result<Value> {
    let stdout = run_ok(&["network", "inspect", name])?;
    let arr: Value =
        serde_json::from_str(&stdout).context("failed to parse network inspect json")?;
    arr.as_array()
        .and_then(|a| a.first())
        .cloned()
        .context("docker network inspect returned no entries")
}

pub fn network_create(spec: &NetworkCreateSpec<'_>) -> Result<()> {
    let dns_joined = spec.dns.join(",");
    let managed_label = format!("{MANAGED_LABEL_KEY}={MANAGED_LABEL_VALUE}");
    let kind_label = format!("{LABEL_KIND}={KIND_NETWORK}");
    let dns_label = format!("{LABEL_NETWORK_DNS}={dns_joined}");
    let subnet_arg = format!("--subnet={}", spec.subnet);
    let gateway_arg = format!("--gateway={}", spec.gateway);
    let mut args: Vec<String> = vec![
        "network".into(),
        "create".into(),
        "--driver".into(),
        "bridge".into(),
        subnet_arg,
        gateway_arg,
        "--label".into(),
        managed_label,
        "--label".into(),
        kind_label,
        "--label".into(),
        dns_label,
    ];
    if let Some(source) = spec.iac_source {
        args.push("--label".into());
        args.push(format!("{LABEL_IAC_SOURCE}={source}"));
    }
    args.push(spec.name.to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_ok(&refs)?;
    Ok(())
}

pub fn network_remove(name: &str) -> Result<()> {
    run_ok(&["network", "rm", name])?;
    Ok(())
}

pub fn list_managed_networks() -> Result<Vec<NetworkRecord>> {
    let stdout = run_ok(&[
        "network",
        "ls",
        "--filter",
        &format!("label={MANAGED_LABEL_FILTER}"),
        "--format",
        "{{.Name}}",
    ])?;
    let names: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let mut records = Vec::with_capacity(names.len());
    for name in names {
        let value = network_inspect(&name)?;
        records.push(parse_network_record(&name, &value));
    }
    Ok(records)
}

pub fn parse_network_record(name: &str, value: &Value) -> NetworkRecord {
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let dns = labels
        .get(LABEL_NETWORK_DNS)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let iac_source = labels
        .get(LABEL_IAC_SOURCE)
        .and_then(Value::as_str)
        .map(String::from);
    let ipam = value
        .pointer("/IPAM/Config/0")
        .cloned()
        .unwrap_or(Value::Null);
    let subnet = ipam
        .get("Subnet")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let gateway = ipam
        .get("Gateway")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    NetworkRecord {
        name: name.to_string(),
        subnet,
        gateway,
        dns,
        iac_source,
    }
}

pub fn container_exists(name: &str) -> Result<bool> {
    let output = run(&["container", "inspect", name])?;
    Ok(output.status.success())
}

pub fn container_running(name: &str) -> Result<bool> {
    let stdout = run_ok(&[
        "container",
        "inspect",
        "--format",
        "{{.State.Running}}",
        name,
    ])?;
    Ok(stdout.trim() == "true")
}

pub fn container_inspect(name: &str) -> Result<Value> {
    let stdout = run_ok(&["container", "inspect", name])?;
    let arr: Value =
        serde_json::from_str(&stdout).context("failed to parse container inspect json")?;
    arr.as_array()
        .and_then(|a| a.first())
        .cloned()
        .context("docker container inspect returned no entries")
}

pub fn list_managed_vms() -> Result<Vec<VmRecord>> {
    let stdout = run_ok(&[
        "ps",
        "-a",
        "--filter",
        &format!("label={MANAGED_LABEL_FILTER}"),
        "--filter",
        &format!("label={LABEL_KIND}={KIND_VM}"),
        "--format",
        "{{.Names}}",
    ])?;
    let names: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let mut records = Vec::with_capacity(names.len());
    for name in names {
        let value = container_inspect(&name)?;
        records.push(parse_vm_record(&name, &value));
    }
    Ok(records)
}

pub fn parse_vm_record(name: &str, value: &Value) -> VmRecord {
    let labels = value
        .pointer("/Config/Labels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let image = labels.get(LABEL_IMAGE).and_then(Value::as_str).map_or_else(
        || {
            value
                .pointer("/Config/Image")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        },
        String::from,
    );
    let network = labels
        .get(LABEL_NETWORK_NAME)
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_default();
    let ipv4 = if network.is_empty() {
        value
            .pointer("/NetworkSettings/Networks")
            .and_then(Value::as_object)
            .and_then(|m| m.values().next())
            .and_then(|n| n.get("IPAddress"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        value
            .pointer(&format!("/NetworkSettings/Networks/{network}/IPAddress"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let nano_cpus = value
        .pointer("/HostConfig/NanoCpus")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let memory_bytes = value
        .pointer("/HostConfig/Memory")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let status = value
        .pointer("/State/Status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let extra_disks_spec = labels
        .get(LABEL_EXTRA_DISKS_SPEC)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let iac_source = labels
        .get(LABEL_IAC_SOURCE)
        .and_then(Value::as_str)
        .map(String::from);
    VmRecord {
        name: name.to_string(),
        image,
        network,
        ipv4,
        cpus: nano_cpus as f64 / 1_000_000_000.0,
        memory_bytes,
        status,
        extra_disks_spec,
        iac_source,
    }
}

pub fn container_run_vm(spec: &VmCreateSpec<'_>) -> Result<()> {
    let cpus_arg = format!("--cpus={}", spec.cpus);
    let memory_arg = format!("--memory={}", spec.memory_bytes);
    let env_cidr = format!("VM_IP_CIDR={}", spec.ipv4_cidr);
    let env_gw = format!("VM_GW={}", spec.gateway);
    let env_dns = format!("VM_DNS={}", spec.dns);
    let managed_label = format!("{MANAGED_LABEL_KEY}={MANAGED_LABEL_VALUE}");
    let kind_label = format!("{LABEL_KIND}={KIND_VM}");
    let image_label = format!("{LABEL_IMAGE}={}", spec.image);
    let net_label = format!("{LABEL_NETWORK_NAME}={}", spec.network);
    let extra_disks_label = format!(
        "{LABEL_EXTRA_DISKS_SPEC}={}",
        spec.extra_disks_spec.join(",")
    );
    let mut args: Vec<String> = vec![
        "run".into(),
        "-d".into(),
        "--name".into(),
        spec.name.into(),
        "--hostname".into(),
        spec.name.into(),
        "--privileged".into(),
        "--network".into(),
        spec.network.into(),
        "--ip".into(),
        spec.ipv4.into(),
        "-e".into(),
        env_cidr,
        "-e".into(),
        env_gw,
        "-e".into(),
        env_dns,
        "--tmpfs".into(),
        "/run".into(),
        "--tmpfs".into(),
        "/run/lock".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        "--stop-signal".into(),
        "RTMIN+3".into(),
        "--restart".into(),
        "unless-stopped".into(),
        cpus_arg,
        memory_arg,
        "--label".into(),
        managed_label,
        "--label".into(),
        kind_label,
        "--label".into(),
        image_label,
        "--label".into(),
        net_label,
        "--label".into(),
        extra_disks_label,
    ];
    if let Some(source) = spec.iac_source {
        args.push("--label".into());
        args.push(format!("{LABEL_IAC_SOURCE}={source}"));
    }
    args.push(spec.image.to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_ok(&refs)?;
    Ok(())
}

pub fn container_remove(name: &str, force: bool) -> Result<()> {
    if force {
        run_ok(&["rm", "-f", name])?;
    } else {
        run_ok(&["rm", name])?;
    }
    Ok(())
}

pub fn container_start(name: &str) -> Result<()> {
    run_ok(&["start", name])?;
    Ok(())
}

pub fn container_stop(name: &str) -> Result<()> {
    run_ok(&["stop", name])?;
    Ok(())
}

pub fn container_restart(name: &str) -> Result<()> {
    run_ok(&["restart", name])?;
    Ok(())
}

pub fn container_update_resources(name: &str, cpus: f64, memory_bytes: u64) -> Result<()> {
    let cpus_arg = format!("--cpus={cpus}");
    let memory_arg = format!("--memory={memory_bytes}");
    run_ok(&["update", &cpus_arg, &memory_arg, name])?;
    Ok(())
}

pub fn container_exec_capture(name: &str, cmd: &[&str]) -> Result<Output> {
    let mut args: Vec<&str> = vec!["exec", name];
    args.extend_from_slice(cmd);
    let output = Command::new("docker")
        .args(&args)
        .output()
        .context("failed to spawn docker exec")?;
    Ok(output)
}

pub fn container_exec_interactive(name: &str, cmd: &[&str]) -> Result<i32> {
    let mut args: Vec<&str> = vec!["exec", "-it", name];
    args.extend_from_slice(cmd);
    let status = Command::new("docker")
        .args(&args)
        .status()
        .context("failed to spawn interactive docker exec")?;
    Ok(status.code().unwrap_or(-1))
}

pub fn image_commit(container: &str, image_tag: &str, vm_name: &str) -> Result<()> {
    let label_managed = format!("{MANAGED_LABEL_KEY}={MANAGED_LABEL_VALUE}");
    let label_kind = format!("{LABEL_KIND}={KIND_SNAPSHOT}");
    let label_vm = format!("{LABEL_SNAPSHOT_VM}={vm_name}");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let label_created = format!("{LABEL_SNAPSHOT_CREATED_AT}={now}");
    run_ok(&[
        "commit",
        "-c",
        &format!("LABEL {label_managed}"),
        "-c",
        &format!("LABEL {label_kind}"),
        "-c",
        &format!("LABEL {label_vm}"),
        "-c",
        &format!("LABEL {label_created}"),
        container,
        image_tag,
    ])?;
    Ok(())
}

pub fn list_snapshots_for(vm_name: &str) -> Result<Vec<(String, String, u64)>> {
    let stdout = run_ok(&[
        "image",
        "ls",
        "--filter",
        &format!("label={MANAGED_LABEL_FILTER}"),
        "--filter",
        &format!("label={LABEL_KIND}={KIND_SNAPSHOT}"),
        "--filter",
        &format!("label={LABEL_SNAPSHOT_VM}={vm_name}"),
        "--format",
        "{{.Repository}}:{{.Tag}}|{{.ID}}|{{.CreatedAt}}",
    ])?;
    let mut entries = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '|');
        let repo_tag = parts.next().unwrap_or("").to_string();
        let id = parts.next().unwrap_or("").to_string();
        let value = image_inspect(&repo_tag).unwrap_or(Value::Null);
        let created = value
            .pointer("/Config/Labels")
            .and_then(Value::as_object)
            .and_then(|m| m.get(LABEL_SNAPSHOT_CREATED_AT))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        entries.push((repo_tag, id, created));
    }
    entries.sort_by_key(|e| e.2);
    Ok(entries)
}

pub fn image_inspect(image: &str) -> Result<Value> {
    let stdout = run_ok(&["image", "inspect", image])?;
    let arr: Value = serde_json::from_str(&stdout).context("failed to parse image inspect json")?;
    arr.as_array()
        .and_then(|a| a.first())
        .cloned()
        .context("docker image inspect returned no entries")
}

pub fn image_exists(image: &str) -> Result<bool> {
    Ok(run(&["image", "inspect", image])?.status.success())
}

pub fn image_remove(image: &str) -> Result<()> {
    run_ok(&["image", "rm", image])?;
    Ok(())
}

pub fn network_attached_vms(network: &str) -> Result<Vec<String>> {
    let stdout = run_ok(&[
        "ps",
        "-a",
        "--filter",
        &format!("network={network}"),
        "--format",
        "{{.Names}}",
    ])?;
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

pub fn list_used_ips_in_network(network: &str) -> Result<Vec<String>> {
    let value = network_inspect(network)?;
    let containers = value
        .get("Containers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut ips = Vec::new();
    for (_id, info) in containers {
        if let Some(ipv4) = info.get("IPv4Address").and_then(Value::as_str)
            && let Some(addr) = ipv4.split('/').next()
            && !addr.is_empty()
        {
            ips.push(addr.to_string());
        }
    }
    if let Some(gw) = value
        .pointer("/IPAM/Config/0/Gateway")
        .and_then(Value::as_str)
    {
        ips.push(gw.to_string());
    }
    Ok(ips)
}
