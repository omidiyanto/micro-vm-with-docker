use anyhow::{Context, Result, bail};
use ipnet::Ipv4Net;
use std::net::Ipv4Addr;

use crate::cli::{NetworkCreateArgs, NetworkModifyArgs};
use crate::docker::{self, NetworkCreateSpec};
use crate::error::{ValidationError, validate_name};
use crate::ui;
use crate::util;

pub fn create(args: &NetworkCreateArgs, iac_source: Option<&str>) -> Result<()> {
    validate_name(&args.name)?;
    let subnet = parse_subnet(&args.subnet)?;
    let gateway = parse_gateway(&args.gateway, subnet)?;
    validate_dns(&args.dns)?;
    if docker::network_exists(&args.name)? {
        bail!("network '{}' already exists", args.name);
    }
    let spec = NetworkCreateSpec {
        name: &args.name,
        subnet: &subnet.to_string(),
        gateway: &gateway.to_string(),
        dns: &args.dns,
        iac_source,
    };
    docker::network_create(&spec)?;
    ui::ok(format!("network '{}' created", ui::cyan(&args.name)));
    Ok(())
}

pub fn list() -> Result<()> {
    let records = docker::list_managed_networks()?;
    let mut rows: Vec<Vec<String>> = records
        .iter()
        .map(|r| {
            vec![
                r.name.clone(),
                r.subnet.clone(),
                r.gateway.clone(),
                r.dns.join(","),
                r.iac_source.clone().unwrap_or_else(|| "-".into()),
            ]
        })
        .collect();
    rows.sort_by(|a, b| a[0].cmp(&b[0]));
    util::print_table(&["NAME", "SUBNET", "GATEWAY", "DNS", "SOURCE"], &rows);
    Ok(())
}

pub fn show(name: &str) -> Result<()> {
    ensure_managed_network(name)?;
    let record = load_managed_record(name)?;
    println!("name      : {}", record.name);
    println!("subnet    : {}", record.subnet);
    println!("gateway   : {}", record.gateway);
    println!("dns       : {}", record.dns.join(","));
    println!(
        "source    : {}",
        record.iac_source.unwrap_or_else(|| "-".into())
    );
    let attached = docker::network_attached_vms(name)?;
    println!(
        "attached  : {}",
        if attached.is_empty() {
            "-".into()
        } else {
            attached.join(",")
        }
    );
    Ok(())
}

pub fn modify(args: &NetworkModifyArgs) -> Result<()> {
    ensure_managed_network(&args.name)?;
    let current = load_managed_record(&args.name)?;
    let new_subnet_str = args
        .subnet
        .clone()
        .unwrap_or_else(|| current.subnet.clone());
    let new_gateway_str = args
        .gateway
        .clone()
        .unwrap_or_else(|| current.gateway.clone());
    let new_dns = args.dns.clone().unwrap_or_else(|| current.dns.clone());
    let subnet = parse_subnet(&new_subnet_str)?;
    let gateway = parse_gateway(&new_gateway_str, subnet)?;
    validate_dns(&new_dns)?;
    let attached = docker::network_attached_vms(&args.name)?;
    if !attached.is_empty() {
        bail!(
            "network '{}' has attached containers: [{}]; detach or destroy them first",
            args.name,
            attached.join(",")
        );
    }
    let source = current.iac_source.clone();
    let no_change = new_subnet_str == current.subnet
        && new_gateway_str == current.gateway
        && new_dns == current.dns;
    if no_change {
        ui::info(format!(
            "no changes to apply for network '{}'",
            ui::cyan(&args.name)
        ));
        return Ok(());
    }
    docker::network_remove(&args.name)?;
    let spec = NetworkCreateSpec {
        name: &args.name,
        subnet: &subnet.to_string(),
        gateway: &gateway.to_string(),
        dns: &new_dns,
        iac_source: source.as_deref(),
    };
    docker::network_create(&spec)?;
    ui::ok(format!("network '{}' modified", ui::cyan(&args.name)));
    Ok(())
}

pub fn destroy(name: &str, yes: bool) -> Result<()> {
    ensure_managed_network(name)?;
    let attached = docker::network_attached_vms(name)?;
    if !attached.is_empty() {
        bail!(
            "network '{}' has attached containers: [{}]; remove them before destroy",
            name,
            attached.join(",")
        );
    }
    if !yes {
        let prompt = format!("about to destroy network '{name}'. continue?");
        if !util::confirm(&prompt)? {
            ui::warn("aborted");
            return Ok(());
        }
    }
    docker::network_remove(name)?;
    ui::ok(format!("network '{}' destroyed", ui::cyan(name)));
    Ok(())
}

pub fn ensure_managed_network(name: &str) -> Result<()> {
    if !docker::network_exists(name)? {
        bail!("network '{name}' does not exist");
    }
    let record = load_managed_record(name)
        .with_context(|| format!("network '{name}' is not managed by virtctl"))?;
    let _ = record;
    Ok(())
}

pub fn load_managed_record(name: &str) -> Result<docker::NetworkRecord> {
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
        bail!("network '{name}' is not managed by virtctl");
    }
    Ok(docker::parse_network_record(name, &value))
}

pub fn parse_subnet(input: &str) -> Result<Ipv4Net> {
    input
        .parse::<Ipv4Net>()
        .map_err(|e| ValidationError::InvalidSubnet(input.to_string(), e.to_string()).into())
}

pub fn parse_gateway(input: &str, subnet: Ipv4Net) -> Result<Ipv4Addr> {
    let addr: Ipv4Addr = input
        .parse()
        .map_err(|_| ValidationError::InvalidSubnet(input.to_string(), "not an IPv4".into()))?;
    if !subnet.contains(&addr) {
        return Err(ValidationError::GatewayNotInSubnet {
            gateway: input.to_string(),
            subnet: subnet.to_string(),
        }
        .into());
    }
    Ok(addr)
}

pub fn validate_dns(dns: &[String]) -> Result<()> {
    for entry in dns {
        let _: std::net::IpAddr = entry
            .parse()
            .with_context(|| format!("invalid DNS server '{entry}'"))?;
    }
    Ok(())
}
