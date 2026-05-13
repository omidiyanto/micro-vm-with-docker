# VIRTCTL - MANAGE MICRO VMS WITH DOCKER

A Docker-backed Micro VM management CLI. Compiles to a single ~1.3 MB binary with strict validation, idempotent operations, and a terraform-style declarative IaC workflow.

---

> ## ⚠️ WARNING — EPHEMERAL MOCK VMs ONLY
>
> `virtctl` manages **ephemeral, non-persistent, mock micro-VMs** implemented as Docker containers running `systemd`. It is intended for **learning, local testing, and experimentation only**.
>
> - Container restart or host reboot **WILL LOSE all in-VM state** (filesystem, processes, user data).
> - "Snapshots" are local docker images (`docker commit`), **not real disk snapshots**. They are not crash-consistent.
> - "Extra disks" are loopback-mounted files inside the container; they vanish with the container.
> - This is **NOT a hypervisor** and **NOT a production VM platform**. There is no isolation guarantee comparable to KVM/Xen/VMware.
>
> **DO NOT use `virtctl` for production workloads, customer data, or anything you care about.**

---

## Getting Started

Install the latest `virtctl` binary in one line (Linux x86_64):

```bash
curl -s https://raw.githubusercontent.com/omidiyanto/micro-vm-with-docker/refs/heads/main/install.sh | sh
```

The installer downloads the latest published binary into `/usr/local/bin/virtctl`, marks it executable, and prints the installed version. It uses `sudo` internally, so you may be prompted for your password.

Verify the install:

```bash
virtctl --version
virtctl --help
```

## Build from source

```bash
cargo build --release
# binary at target/release/virtctl
```

## Imperative usage

```bash
# host dependency check (warns about missing tools)
virtctl check

# networks
virtctl network create --name net01 --subnet 172.25.0.0/24 --gateway 172.25.0.1
virtctl network list
virtctl network show net01
virtctl network modify --name net01 --dns 8.8.8.8,1.1.1.1
virtctl network destroy --name net01 -y

# vms
virtctl vm create --name vm01 --network net01 --ipv4 172.25.0.10 --cpu 0.5 --ram 512M
virtctl vm create --name vm02 --network net01 --ipv4 DHCP --cpu 0.7 --ram 1G
virtctl vm list
virtctl vm show vm01
virtctl vm modify --name vm01 --cpu 1 --ram 1G
virtctl vm console --name vm01
virtctl vm state start   --name vm01
virtctl vm state stop    --name vm01
virtctl vm state restart --name vm01
virtctl vm destroy --name vm01 -y

# snapshots (docker commit based; NOT a real disk snapshot)
virtctl vm snapshot backup  --vm-name vm01 --name pre-upgrade
virtctl vm snapshot list    --vm-name vm01
virtctl vm snapshot restore pre-upgrade --vm-name vm01
virtctl vm snapshot destroy pre-upgrade --vm-name vm01 -y

# mock extra disks (lost when container is removed)
virtctl vm extra-disk --name vm01 --action attach --size 1G
virtctl vm extra-disk --name vm01 --action list
virtctl vm extra-disk --name vm01 --action remove --disk-name loop0
```

## Declarative IaC (terraform-like)

```bash
virtctl iac -f examples/infra.yaml validate-config
virtctl iac -f examples/infra.yaml plan
virtctl iac -f examples/infra.yaml apply
virtctl iac -f examples/infra.yaml state
virtctl iac -f examples/infra.yaml destroy
```

State is tracked via Docker labels (`managed-by=virtctl`, `virtctl.iac-source=<abs-path>`), so the tool stays stateless on the host filesystem and remains idempotent across runs.

## Help

Every level supports `--help` with detailed descriptions:

```bash
virtctl --help
virtctl network --help
virtctl vm snapshot --help
virtctl iac --help
virtctl vm create --help
```
