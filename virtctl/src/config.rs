use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IacDocument {
    #[serde(default)]
    pub network: Vec<NetworkSpec>,
    #[serde(default, rename = "virtual-machines")]
    pub virtual_machines: Vec<VmSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSpec {
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    #[serde(default = "default_dns")]
    pub dns: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VmSpec {
    pub name: String,
    pub image: String,
    pub cpu: f64,
    pub ram: serde_yaml::Value,
    pub network: String,
    pub ipv4: String,
    #[serde(default, rename = "extra-disk")]
    pub extra_disk: Vec<String>,
}

fn default_dns() -> Vec<String> {
    vec!["8.8.8.8".into(), "1.1.1.1".into()]
}

pub fn load_document(path: &Path) -> Result<IacDocument> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file '{}'", path.display()))?;
    let document: IacDocument = serde_yaml::from_str(&content)
        .with_context(|| format!("failed to parse YAML config '{}'", path.display()))?;
    validate_document(&document)?;
    Ok(document)
}

pub fn validate_document(doc: &IacDocument) -> Result<()> {
    let mut seen_networks: HashSet<&str> = HashSet::new();
    for net in &doc.network {
        crate::error::validate_name(&net.name)
            .with_context(|| format!("invalid network name '{}'", net.name))?;
        if !seen_networks.insert(net.name.as_str()) {
            bail!("duplicate network name '{}' in config", net.name);
        }
        let subnet: ipnet::Ipv4Net = net.subnet.parse().with_context(|| {
            format!("invalid subnet '{}' on network '{}'", net.subnet, net.name)
        })?;
        let gateway: std::net::Ipv4Addr = net.gateway.parse().with_context(|| {
            format!(
                "invalid gateway '{}' on network '{}'",
                net.gateway, net.name
            )
        })?;
        if !subnet.contains(&gateway) {
            bail!(
                "gateway '{}' is not within subnet '{}' on network '{}'",
                net.gateway,
                net.subnet,
                net.name
            );
        }
        for dns in &net.dns {
            let _: std::net::IpAddr = dns.parse().with_context(|| {
                format!("invalid DNS server '{}' on network '{}'", dns, net.name)
            })?;
        }
    }
    let mut seen_vms: HashSet<&str> = HashSet::new();
    let mut used_static_ips: HashSet<String> = HashSet::new();
    for vm in &doc.virtual_machines {
        crate::error::validate_name(&vm.name)
            .with_context(|| format!("invalid VM name '{}'", vm.name))?;
        if !seen_vms.insert(vm.name.as_str()) {
            bail!("duplicate VM name '{}' in config", vm.name);
        }
        if vm.cpu <= 0.0 || vm.cpu > 1024.0 || !vm.cpu.is_finite() {
            bail!("VM '{}' has invalid cpu '{}'", vm.name, vm.cpu);
        }
        let _ =
            ram_to_bytes(&vm.ram).with_context(|| format!("VM '{}' has invalid ram", vm.name))?;
        if !seen_networks.contains(vm.network.as_str()) {
            bail!(
                "VM '{}' references unknown network '{}'",
                vm.name,
                vm.network
            );
        }
        if vm.ipv4.eq_ignore_ascii_case("DHCP") {
            // dynamic allocation deferred to apply phase
        } else {
            let addr: std::net::Ipv4Addr = vm
                .ipv4
                .parse()
                .with_context(|| format!("VM '{}' has invalid ipv4 '{}'", vm.name, vm.ipv4))?;
            let net_spec = doc
                .network
                .iter()
                .find(|n| n.name == vm.network)
                .expect("network presence already validated");
            let subnet: ipnet::Ipv4Net = net_spec
                .subnet
                .parse()
                .context("subnet parse error after validation should not occur")?;
            if !subnet.contains(&addr) {
                bail!(
                    "VM '{}' ipv4 '{}' is not within network '{}' subnet '{}'",
                    vm.name,
                    vm.ipv4,
                    vm.network,
                    net_spec.subnet
                );
            }
            let key = format!("{}|{}", vm.network, vm.ipv4);
            if !used_static_ips.insert(key) {
                bail!(
                    "duplicate static IP '{}' on network '{}' (VM '{}')",
                    vm.ipv4,
                    vm.network,
                    vm.name
                );
            }
        }
        for disk in &vm.extra_disk {
            let _ = crate::util::parse_size(disk)
                .with_context(|| format!("VM '{}' has invalid extra-disk '{}'", vm.name, disk))?;
        }
    }
    Ok(())
}

pub fn ram_to_bytes(value: &serde_yaml::Value) -> Result<u64> {
    match value {
        serde_yaml::Value::Number(num) => {
            if let Some(i) = num.as_u64() {
                Ok(i.saturating_mul(1024 * 1024))
            } else if let Some(f) = num.as_f64() {
                if !f.is_finite() || f <= 0.0 {
                    bail!("ram must be a positive number");
                }
                Ok((f * 1024.0 * 1024.0).round() as u64)
            } else {
                bail!("ram number is not representable")
            }
        }
        serde_yaml::Value::String(s) => crate::util::parse_memory(s).map_err(Into::into),
        _ => bail!("ram must be a number (MB) or string like '512M' / '1G'"),
    }
}
