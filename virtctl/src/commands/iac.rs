use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::{NetworkCreateArgs, VmCreateArgs};
use crate::commands::network as network_cmd;
use crate::commands::vm as vm_cmd;
use crate::config::{self, IacDocument, NetworkSpec, VmSpec};
use crate::docker;
use crate::ui;
use crate::util;

const PLAN_CREATE: &str = "CREATE";
const PLAN_DESTROY: &str = "DESTROY";
const PLAN_NOOP: &str = "NOOP";
const PLAN_UPDATE: &str = "UPDATE";
const PLAN_RECREATE: &str = "RECREATE";

pub fn validate(path: &Path) -> Result<()> {
    let _ = config::load_document(path)?;
    ui::ok(format!(
        "config '{}' is valid",
        ui::cyan(&path.display().to_string())
    ));
    Ok(())
}

pub fn state(path: &Path) -> Result<()> {
    let source = canonical_source(path)?;
    let networks = list_source_networks(&source)?;
    let vms = list_source_vms(&source)?;
    println!("source: {source}");
    println!();
    println!("networks:");
    if networks.is_empty() {
        println!("  (none)");
    } else {
        let rows: Vec<Vec<String>> = networks
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.subnet.clone(),
                    r.gateway.clone(),
                    r.dns.join(","),
                ]
            })
            .collect();
        util::print_table(&["NAME", "SUBNET", "GATEWAY", "DNS"], &rows);
    }
    println!();
    println!("virtual-machines:");
    if vms.is_empty() {
        println!("  (none)");
    } else {
        let rows: Vec<Vec<String>> = vms
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    r.status.clone(),
                    r.ipv4.clone(),
                    r.network.clone(),
                    util::format_cpu(r.cpus),
                    util::format_bytes(r.memory_bytes),
                ]
            })
            .collect();
        util::print_table(&["NAME", "STATE", "IP", "NETWORK", "CPU", "RAM"], &rows);
    }
    Ok(())
}

pub fn plan(path: &Path) -> Result<()> {
    let source = canonical_source(path)?;
    let doc = config::load_document(path)?;
    let plan = compute_plan(&doc, &source)?;
    print_plan(&plan);
    Ok(())
}

pub fn apply(path: &Path, yes: bool) -> Result<()> {
    let source = canonical_source(path)?;
    let doc = config::load_document(path)?;
    let plan = compute_plan(&doc, &source)?;
    print_plan(&plan);
    if plan.is_no_change() {
        ui::info("nothing to do; state already matches desired.");
        return Ok(());
    }
    if !yes && !util::confirm("apply the plan above?")? {
        ui::warn("aborted");
        return Ok(());
    }
    execute_apply(&plan, &source)?;
    ui::ok("apply complete");
    Ok(())
}

pub fn destroy(path: &Path, yes: bool) -> Result<()> {
    let source = canonical_source(path)?;
    let networks = list_source_networks(&source)?;
    let vms = list_source_vms(&source)?;
    if networks.is_empty() && vms.is_empty() {
        ui::info(format!(
            "nothing to destroy; no resources managed by '{}'",
            ui::cyan(source.as_str())
        ));
        return Ok(());
    }
    ui::header("The following resources will be destroyed:");
    for v in &vms {
        println!("  {} vm/{}", ui::red("-"), ui::cyan(&v.name));
    }
    for n in &networks {
        println!("  {} network/{}", ui::red("-"), ui::cyan(&n.name));
    }
    if !yes && !util::confirm("destroy all resources?")? {
        ui::warn("aborted");
        return Ok(());
    }
    for v in &vms {
        docker::container_remove(&v.name, true)?;
        ui::ok(format!("destroyed vm/{}", ui::cyan(&v.name)));
    }
    for n in &networks {
        let attached = docker::network_attached_vms(&n.name)?;
        if !attached.is_empty() {
            bail!(
                "network '{}' still has attached containers [{}]; refusing to destroy",
                n.name,
                attached.join(",")
            );
        }
        docker::network_remove(&n.name)?;
        ui::ok(format!("destroyed network/{}", ui::cyan(&n.name)));
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct Plan {
    pub network_create: Vec<NetworkSpec>,
    pub network_update: Vec<NetworkUpdate>,
    pub network_destroy: Vec<String>,
    pub network_noop: Vec<String>,
    pub vm_create: Vec<VmSpec>,
    pub vm_update: Vec<VmUpdate>,
    pub vm_recreate: Vec<VmSpec>,
    pub vm_destroy: Vec<String>,
    pub vm_noop: Vec<String>,
}

impl Plan {
    pub const fn is_no_change(&self) -> bool {
        self.network_create.is_empty()
            && self.network_update.is_empty()
            && self.network_destroy.is_empty()
            && self.vm_create.is_empty()
            && self.vm_update.is_empty()
            && self.vm_recreate.is_empty()
            && self.vm_destroy.is_empty()
    }
}

#[derive(Debug)]
pub struct NetworkUpdate {
    pub spec: NetworkSpec,
    pub reasons: Vec<String>,
}

#[derive(Debug)]
pub struct VmUpdate {
    pub spec: VmSpec,
    pub new_cpus: f64,
    pub new_memory_bytes: u64,
    pub reasons: Vec<String>,
}

pub fn compute_plan(doc: &IacDocument, source: &str) -> Result<Plan> {
    let mut plan = Plan::default();
    let current_networks = list_source_networks(source)?;
    let current_vms = list_source_vms(source)?;
    let current_net_by_name: HashMap<&str, &docker::NetworkRecord> = current_networks
        .iter()
        .map(|n| (n.name.as_str(), n))
        .collect();
    let current_vm_by_name: HashMap<&str, &docker::VmRecord> =
        current_vms.iter().map(|v| (v.name.as_str(), v)).collect();
    let desired_net_names: std::collections::HashSet<&str> =
        doc.network.iter().map(|n| n.name.as_str()).collect();
    let desired_vm_names: std::collections::HashSet<&str> = doc
        .virtual_machines
        .iter()
        .map(|v| v.name.as_str())
        .collect();

    for net in &doc.network {
        ensure_not_foreign_network(&net.name, source)?;
        match current_net_by_name.get(net.name.as_str()) {
            None => plan.network_create.push(net.clone()),
            Some(current) => {
                let reasons = network_diff_reasons(current, net);
                if reasons.is_empty() {
                    plan.network_noop.push(net.name.clone());
                } else {
                    plan.network_update.push(NetworkUpdate {
                        spec: net.clone(),
                        reasons,
                    });
                }
            }
        }
    }
    for current in &current_networks {
        if !desired_net_names.contains(current.name.as_str()) {
            plan.network_destroy.push(current.name.clone());
        }
    }
    for vm in &doc.virtual_machines {
        ensure_not_foreign_vm(&vm.name, source)?;
        match current_vm_by_name.get(vm.name.as_str()) {
            None => plan.vm_create.push(vm.clone()),
            Some(current) => {
                let new_mem = config::ram_to_bytes(&vm.ram)?;
                let mut recreate_reasons = Vec::new();
                if current.image != vm.image {
                    recreate_reasons.push(format!("image: {} -> {}", current.image, vm.image));
                }
                if current.network != vm.network {
                    recreate_reasons
                        .push(format!("network: {} -> {}", current.network, vm.network));
                }
                if !vm.ipv4.eq_ignore_ascii_case("DHCP") && current.ipv4 != vm.ipv4 {
                    recreate_reasons.push(format!("ipv4: {} -> {}", current.ipv4, vm.ipv4));
                }
                if current.extra_disks_spec != vm.extra_disk {
                    recreate_reasons.push(format!(
                        "extra-disk: {:?} -> {:?}",
                        current.extra_disks_spec, vm.extra_disk
                    ));
                }
                if !recreate_reasons.is_empty() {
                    plan.vm_recreate.push(vm.clone());
                    continue;
                }
                let mut update_reasons = Vec::new();
                if (current.cpus - vm.cpu).abs() > f64::EPSILON {
                    update_reasons.push(format!("cpu: {} -> {}", current.cpus, vm.cpu));
                }
                if current.memory_bytes != new_mem {
                    update_reasons.push(format!(
                        "ram: {} -> {}",
                        util::format_bytes(current.memory_bytes),
                        util::format_bytes(new_mem)
                    ));
                }
                if update_reasons.is_empty() {
                    plan.vm_noop.push(vm.name.clone());
                } else {
                    plan.vm_update.push(VmUpdate {
                        spec: vm.clone(),
                        new_cpus: vm.cpu,
                        new_memory_bytes: new_mem,
                        reasons: update_reasons,
                    });
                }
            }
        }
    }
    for current in &current_vms {
        if !desired_vm_names.contains(current.name.as_str()) {
            plan.vm_destroy.push(current.name.clone());
        }
    }
    Ok(plan)
}

fn network_diff_reasons(current: &docker::NetworkRecord, desired: &NetworkSpec) -> Vec<String> {
    let mut reasons = Vec::new();
    if current.subnet != desired.subnet {
        reasons.push(format!("subnet: {} -> {}", current.subnet, desired.subnet));
    }
    if current.gateway != desired.gateway {
        reasons.push(format!(
            "gateway: {} -> {}",
            current.gateway, desired.gateway
        ));
    }
    if current.dns != desired.dns {
        reasons.push(format!(
            "dns: {} -> {}",
            current.dns.join(","),
            desired.dns.join(",")
        ));
    }
    reasons
}

fn ensure_not_foreign_network(name: &str, source: &str) -> Result<()> {
    if !docker::network_exists(name)? {
        return Ok(());
    }
    let value = docker::network_inspect(name)?;
    let labels = value
        .get("Labels")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    let managed = labels
        .get(docker::MANAGED_LABEL_KEY)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if managed != docker::MANAGED_LABEL_VALUE {
        bail!(
            "network '{name}' already exists and is not managed by virtctl; refusing to take over"
        );
    }
    let owner = labels
        .get(docker::LABEL_IAC_SOURCE)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !owner.is_empty() && owner != source {
        bail!("network '{name}' belongs to another iac source '{owner}'; refusing to take over");
    }
    Ok(())
}

fn ensure_not_foreign_vm(name: &str, source: &str) -> Result<()> {
    if !docker::container_exists(name)? {
        return Ok(());
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
    if managed != docker::MANAGED_LABEL_VALUE {
        bail!(
            "container '{name}' already exists and is not managed by virtctl; refusing to take over"
        );
    }
    let owner = labels
        .get(docker::LABEL_IAC_SOURCE)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !owner.is_empty() && owner != source {
        bail!("VM '{name}' belongs to another iac source '{owner}'; refusing to take over");
    }
    Ok(())
}

fn print_plan(plan: &Plan) {
    ui::header("Plan:");
    println!(
        "  networks: {} to create, {} to update, {} to destroy, {} unchanged",
        ui::green(&plan.network_create.len().to_string()),
        ui::yellow(&plan.network_update.len().to_string()),
        ui::red(&plan.network_destroy.len().to_string()),
        ui::dim(&plan.network_noop.len().to_string()),
    );
    println!(
        "  vms     : {} to create, {} to update, {} to recreate, {} to destroy, {} unchanged",
        ui::green(&plan.vm_create.len().to_string()),
        ui::yellow(&plan.vm_update.len().to_string()),
        ui::magenta(&plan.vm_recreate.len().to_string()),
        ui::red(&plan.vm_destroy.len().to_string()),
        ui::dim(&plan.vm_noop.len().to_string()),
    );
    println!();
    let create_label = ui::green(PLAN_CREATE);
    let update_label = ui::yellow(PLAN_UPDATE);
    let destroy_label = ui::red(PLAN_DESTROY);
    let recreate_label = ui::magenta(PLAN_RECREATE);
    let noop_label = ui::dim(PLAN_NOOP);
    for net in &plan.network_create {
        println!("  {create_label}    network/{}", ui::cyan(&net.name));
    }
    for update in &plan.network_update {
        println!(
            "  {update_label}    network/{} ({})",
            ui::cyan(&update.spec.name),
            ui::dim(&update.reasons.join(", "))
        );
    }
    for name in &plan.network_destroy {
        println!("  {destroy_label}   network/{}", ui::cyan(name));
    }
    for name in &plan.network_noop {
        println!("  {noop_label}      network/{}", ui::cyan(name));
    }
    for vm in &plan.vm_create {
        println!("  {create_label}    vm/{}", ui::cyan(&vm.name));
    }
    for update in &plan.vm_update {
        println!(
            "  {update_label}    vm/{} ({})",
            ui::cyan(&update.spec.name),
            ui::dim(&update.reasons.join(", "))
        );
    }
    for vm in &plan.vm_recreate {
        println!("  {recreate_label}  vm/{}", ui::cyan(&vm.name));
    }
    for name in &plan.vm_destroy {
        println!("  {destroy_label}   vm/{}", ui::cyan(name));
    }
    for name in &plan.vm_noop {
        println!("  {noop_label}      vm/{}", ui::cyan(name));
    }
    println!();
}

fn execute_apply(plan: &Plan, source: &str) -> Result<()> {
    for name in &plan.vm_destroy {
        docker::container_remove(name, true)?;
        ui::ok(format!("destroyed vm/{}", ui::cyan(name)));
    }
    for name in &plan.network_destroy {
        let attached = docker::network_attached_vms(name)?;
        if !attached.is_empty() {
            bail!(
                "network '{name}' still has attached containers [{}]",
                attached.join(",")
            );
        }
        docker::network_remove(name)?;
        ui::ok(format!("destroyed network/{}", ui::cyan(name)));
    }
    for net in &plan.network_create {
        let args = NetworkCreateArgs {
            name: net.name.clone(),
            subnet: net.subnet.clone(),
            gateway: net.gateway.clone(),
            dns: net.dns.clone(),
        };
        network_cmd::create(&args, Some(source))?;
    }
    for update in &plan.network_update {
        let attached = docker::network_attached_vms(&update.spec.name)?;
        if !attached.is_empty() {
            bail!(
                "network '{}' has attached containers [{}]; cannot update",
                update.spec.name,
                attached.join(",")
            );
        }
        docker::network_remove(&update.spec.name)?;
        let args = NetworkCreateArgs {
            name: update.spec.name.clone(),
            subnet: update.spec.subnet.clone(),
            gateway: update.spec.gateway.clone(),
            dns: update.spec.dns.clone(),
        };
        network_cmd::create(&args, Some(source))?;
    }
    for vm in &plan.vm_recreate {
        docker::container_remove(&vm.name, true)?;
        create_vm_from_spec(vm, source)?;
    }
    for vm in &plan.vm_create {
        create_vm_from_spec(vm, source)?;
    }
    for update in &plan.vm_update {
        docker::container_update_resources(
            &update.spec.name,
            update.new_cpus,
            update.new_memory_bytes,
        )?;
        ui::ok(format!(
            "updated vm/{} (cpu={} ram={})",
            ui::cyan(&update.spec.name),
            util::format_cpu(update.new_cpus),
            util::format_bytes(update.new_memory_bytes)
        ));
    }
    Ok(())
}

fn create_vm_from_spec(vm: &VmSpec, source: &str) -> Result<()> {
    let ram_bytes = config::ram_to_bytes(&vm.ram)?;
    let ram_str = util::format_bytes(ram_bytes);
    let args = VmCreateArgs {
        name: vm.name.clone(),
        network: vm.network.clone(),
        ipv4: vm.ipv4.clone(),
        cpu: vm.cpu,
        ram: ram_str,
        image: vm.image.clone(),
        extra_disk: vm.extra_disk.clone(),
    };
    vm_cmd::create(&args, Some(source))
}

fn list_source_networks(source: &str) -> Result<Vec<docker::NetworkRecord>> {
    Ok(docker::list_managed_networks()?
        .into_iter()
        .filter(|n| n.iac_source.as_deref() == Some(source))
        .collect())
}

fn list_source_vms(source: &str) -> Result<Vec<docker::VmRecord>> {
    Ok(docker::list_managed_vms()?
        .into_iter()
        .filter(|v| v.iac_source.as_deref() == Some(source))
        .collect())
}

pub fn canonical_source(path: &Path) -> Result<String> {
    let abs: PathBuf = std::fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize '{}'", path.display()))?;
    Ok(abs.to_string_lossy().into_owned())
}
