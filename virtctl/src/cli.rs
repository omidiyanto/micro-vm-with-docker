use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

const LONG_ABOUT: &str = "\
virtctl - a Docker-backed micro VM management CLI.

Manage micro virtual machines (Docker containers running systemd) and their\n\
networks with strict validation, idempotent operations, and a declarative\n\
IaC workflow comparable to terraform plan/apply/destroy.\n\
\n\
!! WARNING: virtctl manages EPHEMERAL, NON-PERSISTENT, MOCK micro-VMs.\n\
   They are intended for learning, local testing, and experimentation only.\n\
   Container restart / host reboot WILL LOSE all in-VM state. Snapshots are\n\
   local docker images, not real disk snapshots. DO NOT use this tool for\n\
   production workloads. !!\n\
\n\
Examples:\n  \
virtctl check\n  \
virtctl network create --name net01 --subnet 172.25.0.0/24 --gateway 172.25.0.1\n  \
virtctl vm create --name vm01 --network net01 --ipv4 172.25.0.10 --cpu 0.5 --ram 512M\n  \
virtctl vm list\n  \
virtctl vm snapshot backup --vm-name vm01\n  \
virtctl iac -f infra.yaml plan\n  \
virtctl iac -f infra.yaml apply\n";

#[derive(Debug, Parser)]
#[command(
    name = "virtctl",
    version,
    about = "Docker-backed micro VM management CLI",
    long_about = LONG_ABOUT,
    propagate_version = true,
    arg_required_else_help = true,
    disable_help_subcommand = true,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(
        about = "Check host dependencies (docker, daemon, internet)",
        long_about = "Verify host readiness for virtctl: docker binary, docker daemon, and internet connectivity. Will attempt to install or start docker if missing."
    )]
    Check,

    #[command(
        about = "Manage virtual networks",
        long_about = "Create, list, inspect, modify, and destroy virtctl-managed Docker networks used by VMs."
    )]
    Network {
        #[command(subcommand)]
        action: NetworkAction,
    },

    #[command(
        about = "Manage virtual machines",
        long_about = "Create, list, modify, destroy, and operate Docker-backed mock virtual machines."
    )]
    Vm {
        #[command(subcommand)]
        action: VmAction,
    },

    #[command(
        about = "Declarative Infrastructure-as-Code from a YAML config",
        long_about = "Apply a desired-state YAML describing networks and VMs. Supports plan, apply, destroy, state, and validate-config subcommands.\n\nWARNING: virtctl manages EPHEMERAL mock VMs; do NOT use for production workloads."
    )]
    Iac(IacArgs),
}

#[derive(Debug, Subcommand)]
pub enum NetworkAction {
    #[command(about = "Create a new virtual network")]
    Create(NetworkCreateArgs),
    #[command(about = "List all virtctl-managed networks")]
    List,
    #[command(about = "Show detailed information about a network")]
    Show {
        #[arg(help = "Network name")]
        name: String,
    },
    #[command(
        about = "Modify a network (recreates it; requires no attached VMs)",
        long_about = "Docker does not allow in-place changes of subnet or gateway. virtctl will remove and recreate the network with the new parameters. The operation is rejected if any VM is currently attached."
    )]
    Modify(NetworkModifyArgs),
    #[command(
        about = "Destroy a network (requires no attached VMs)",
        long_about = "Permanently removes the network. The operation is rejected if any VM (managed or unmanaged) is still attached."
    )]
    Destroy {
        #[arg(long, help = "Network name")]
        name: String,
        #[arg(short = 'y', long, help = "Do not prompt for confirmation")]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct NetworkCreateArgs {
    #[arg(long, help = "Unique network name (a-z, 0-9, '-', '_')")]
    pub name: String,
    #[arg(long, help = "IPv4 subnet in CIDR notation, e.g. 172.25.0.0/24")]
    pub subnet: String,
    #[arg(long, help = "Gateway IPv4 address inside the subnet")]
    pub gateway: String,
    #[arg(
        long,
        value_delimiter = ',',
        default_values_t = vec!["8.8.8.8".to_string(), "1.1.1.1".to_string()],
        help = "DNS servers (comma-separated)"
    )]
    pub dns: Vec<String>,
}

#[derive(Debug, Args)]
pub struct NetworkModifyArgs {
    #[arg(long, help = "Existing network name")]
    pub name: String,
    #[arg(long, help = "New subnet (optional)")]
    pub subnet: Option<String>,
    #[arg(long, help = "New gateway (optional)")]
    pub gateway: Option<String>,
    #[arg(long, value_delimiter = ',', help = "New DNS list (optional)")]
    pub dns: Option<Vec<String>>,
}

#[derive(Debug, Subcommand)]
pub enum VmAction {
    #[command(about = "Create and start a new VM")]
    Create(VmCreateArgs),
    #[command(about = "List all virtctl-managed VMs")]
    List,
    #[command(about = "Show detailed information about a VM")]
    Show {
        #[arg(help = "VM name")]
        name: String,
    },
    #[command(
        about = "Modify a VM (CPU and RAM live, others recreate)",
        long_about = "Live update of CPU/RAM via docker update. IP or network changes will recreate the container. Extra disks are reconciled by replay."
    )]
    Modify(VmModifyArgs),
    #[command(about = "Destroy a VM (data is non-persistent and lost)")]
    Destroy {
        #[arg(long, help = "VM name")]
        name: String,
        #[arg(short = 'y', long, help = "Do not prompt for confirmation")]
        yes: bool,
    },
    #[command(about = "Attach to the VM console (login prompt)")]
    Console {
        #[arg(long, help = "VM name")]
        name: String,
    },
    #[command(about = "Manage VM lifecycle (start/stop/restart)")]
    State {
        #[command(subcommand)]
        action: VmStateAction,
    },
    #[command(about = "Manage VM snapshots (docker commit based)")]
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },
    #[command(about = "Manage extra (mock) disks attached to a VM")]
    ExtraDisk(ExtraDiskArgs),
}

#[derive(Debug, Args)]
pub struct VmCreateArgs {
    #[arg(long, help = "Unique VM name")]
    pub name: String,
    #[arg(long, help = "Target network name")]
    pub network: String,
    #[arg(
        long,
        default_value = "DHCP",
        help = "IPv4 address or 'DHCP' to auto-allocate"
    )]
    pub ipv4: String,
    #[arg(long, default_value_t = 0.5, help = "Number of CPUs (float)")]
    pub cpu: f64,
    #[arg(long, default_value = "512M", help = "Memory size, e.g. 512M or 1G")]
    pub ram: String,
    #[arg(
        long,
        default_value = "ghcr.io/omidiyanto/micro-vm-with-docker/ubuntu:noble",
        help = "Base image to boot"
    )]
    pub image: String,
    #[arg(
        long = "extra-disk",
        value_delimiter = ',',
        help = "Extra mock disks (comma-separated sizes, e.g. 1G,512M)"
    )]
    pub extra_disk: Vec<String>,
}

#[derive(Debug, Args)]
pub struct VmModifyArgs {
    #[arg(long, help = "Existing VM name")]
    pub name: String,
    #[arg(long, help = "New CPU value (optional)")]
    pub cpu: Option<f64>,
    #[arg(long, help = "New RAM value, e.g. 1G (optional)")]
    pub ram: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum VmStateAction {
    #[command(about = "Start a VM")]
    Start {
        #[arg(long, help = "VM name")]
        name: String,
    },
    #[command(about = "Stop a VM gracefully")]
    Stop {
        #[arg(long, help = "VM name")]
        name: String,
    },
    #[command(about = "Restart a VM")]
    Restart {
        #[arg(long, help = "VM name")]
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum SnapshotAction {
    #[command(about = "Create a snapshot (docker commit) of a running VM")]
    Backup {
        #[arg(long = "vm-name", help = "VM name to snapshot")]
        vm_name: String,
        #[arg(long, help = "Optional snapshot name (default: timestamp tag)")]
        name: Option<String>,
    },
    #[command(about = "List snapshots for a VM")]
    List {
        #[arg(long = "vm-name", help = "VM name")]
        vm_name: String,
    },
    #[command(
        about = "Restore a VM from a snapshot",
        long_about = "Recreates the VM container from the snapshot image, preserving the network configuration, IP, CPU, and RAM of the live VM."
    )]
    Restore {
        #[arg(help = "Snapshot identifier (image:tag) as shown by 'snapshot list'")]
        snapshot: String,
        #[arg(long = "vm-name", help = "VM name to restore")]
        vm_name: String,
    },
    #[command(about = "Delete a snapshot")]
    Destroy {
        #[arg(help = "Snapshot identifier (image:tag)")]
        snapshot: String,
        #[arg(long = "vm-name", help = "VM name owning the snapshot")]
        vm_name: String,
        #[arg(short = 'y', long, help = "Do not prompt for confirmation")]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct ExtraDiskArgs {
    #[arg(long, help = "VM name")]
    pub name: String,
    #[arg(long, value_enum, help = "Action to perform")]
    pub action: DiskAction,
    #[arg(long, help = "Size of the disk to attach (e.g. 1G)")]
    pub size: Option<String>,
    #[arg(long = "disk-name", help = "Loop device name to remove (e.g. loop0)")]
    pub disk_name: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DiskAction {
    Attach,
    Remove,
    List,
}

#[derive(Debug, Args)]
pub struct IacArgs {
    #[arg(short = 'f', long = "file", help = "Path to YAML config")]
    pub file: PathBuf,
    #[arg(
        short = 'y',
        long,
        global = true,
        help = "Do not prompt for confirmation (apply/destroy)"
    )]
    pub yes: bool,
    #[command(subcommand)]
    pub action: IacAction,
}

#[derive(Debug, Subcommand)]
pub enum IacAction {
    #[command(about = "Show the diff between desired state and current state")]
    Plan,
    #[command(about = "Apply the desired state (idempotent)")]
    Apply,
    #[command(about = "Destroy all resources managed by this config file")]
    Destroy,
    #[command(about = "Show current state of resources tracked by this config file")]
    State,
    #[command(about = "Validate the YAML config without contacting Docker")]
    ValidateConfig,
}
