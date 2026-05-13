mod cli;
mod commands;
mod config;
mod docker;
mod error;
mod ui;
mod util;

use clap::Parser;
use std::process::ExitCode;

use crate::cli::{
    Cli, Command, DiskAction, IacAction, NetworkAction, SnapshotAction, VmAction, VmStateAction,
};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            ui::eprintln_error(format!("{err:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Check => commands::check::run(),
        Command::Network { action } => match action {
            NetworkAction::Create(args) => commands::network::create(&args, None),
            NetworkAction::List => commands::network::list(),
            NetworkAction::Show { name } => commands::network::show(&name),
            NetworkAction::Modify(args) => commands::network::modify(&args),
            NetworkAction::Destroy { name, yes } => commands::network::destroy(&name, yes),
        },
        Command::Vm { action } => match action {
            VmAction::Create(args) => commands::vm::create(&args, None),
            VmAction::List => commands::vm::list(),
            VmAction::Show { name } => commands::vm::show(&name),
            VmAction::Modify(args) => commands::vm::modify(&args),
            VmAction::Destroy { name, yes } => commands::vm::destroy(&name, yes),
            VmAction::Console { name } => commands::vm::console(&name),
            VmAction::State { action } => match action {
                VmStateAction::Start { name } => commands::vm::start(&name),
                VmStateAction::Stop { name } => commands::vm::stop(&name),
                VmStateAction::Restart { name } => commands::vm::restart(&name),
            },
            VmAction::Snapshot { action } => match action {
                SnapshotAction::Backup { vm_name, name } => {
                    commands::snapshot::backup(&vm_name, name.as_deref())
                }
                SnapshotAction::List { vm_name } => commands::snapshot::list(&vm_name),
                SnapshotAction::Restore { snapshot, vm_name } => {
                    commands::snapshot::restore(&vm_name, &snapshot)
                }
                SnapshotAction::Destroy {
                    snapshot,
                    vm_name,
                    yes,
                } => commands::snapshot::destroy(&vm_name, &snapshot, yes),
            },
            VmAction::ExtraDisk(args) => match args.action {
                DiskAction::Attach => commands::disk::attach(&args.name, args.size.as_deref()),
                DiskAction::Remove => commands::disk::remove(&args.name, args.disk_name.as_deref()),
                DiskAction::List => commands::disk::list(&args.name),
            },
        },
        Command::Iac(args) => match args.action {
            IacAction::Plan => commands::iac::plan(&args.file),
            IacAction::Apply => commands::iac::apply(&args.file, args.yes),
            IacAction::Destroy => commands::iac::destroy(&args.file, args.yes),
            IacAction::State => commands::iac::state(&args.file),
            IacAction::ValidateConfig => commands::iac::validate(&args.file),
        },
    }
}
