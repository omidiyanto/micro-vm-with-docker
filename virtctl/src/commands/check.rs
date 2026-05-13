use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::docker;
use crate::ui;

struct HostTool {
    binary: &'static str,
    purpose: &'static str,
}

const HOST_TOOLS: &[HostTool] = &[
    HostTool {
        binary: "curl",
        purpose: "fetch docker installer / external resources",
    },
    HostTool {
        binary: "sh",
        purpose: "execute install scripts via pipeline",
    },
    HostTool {
        binary: "systemctl",
        purpose: "start/stop the docker daemon",
    },
    HostTool {
        binary: "ping",
        purpose: "verify outbound internet connectivity",
    },
    HostTool {
        binary: "df",
        purpose: "query host disk space before allocating mock disks",
    },
];

pub fn run() -> Result<()> {
    ui::header("Running host dependency checks");
    let mut all_ok = true;
    for tool in HOST_TOOLS {
        all_ok &= check_host_tool(tool);
    }
    all_ok &= check_docker_binary()?;
    all_ok &= check_docker_daemon();
    all_ok &= check_internet();
    println!();
    if all_ok {
        ui::ok("All checks passed");
        Ok(())
    } else {
        bail!(
            "one or more checks failed; please install the missing tools using your distro's package manager"
        );
    }
}

fn check_host_tool(tool: &HostTool) -> bool {
    if binary_in_path(tool.binary) {
        ui::ok(format!(
            "{} is available ({})",
            ui::cyan(tool.binary),
            ui::dim(tool.purpose)
        ));
        true
    } else {
        ui::fail(format!(
            "'{}' is required for virtctl ({}); please install it via your distro's package manager",
            ui::cyan(tool.binary),
            ui::dim(tool.purpose)
        ));
        false
    }
}

fn check_docker_binary() -> Result<bool> {
    if docker::binary_available() {
        ui::ok("docker binary is installed");
        return Ok(true);
    }
    if !binary_in_path("curl") || !binary_in_path("sh") {
        ui::fail(
            "docker binary is required for virtctl; please install it manually (https://docs.docker.com/engine/install/)",
        );
        return Ok(false);
    }
    ui::warn("docker binary not found, attempting installation via get.docker.com");
    let installer = "curl -fsSL https://get.docker.com | sh";
    let status = Command::new("sh")
        .args(["-c", installer])
        .status()
        .context("failed to spawn shell to install docker")?;
    if !status.success() {
        ui::fail("docker installation failed; please install it manually");
        return Ok(false);
    }
    if docker::binary_available() {
        ui::ok("docker installed successfully");
        Ok(true)
    } else {
        ui::fail("docker still not available after installation");
        Ok(false)
    }
}

fn check_docker_daemon() -> bool {
    if docker::daemon_running() {
        ui::ok("docker daemon is running");
        return true;
    }
    if !binary_in_path("systemctl") {
        ui::fail(
            "docker daemon is not running; please start it manually (your distro may not use systemd)",
        );
        return false;
    }
    ui::warn("docker daemon not running, attempting `systemctl start docker`");
    let status = Command::new("systemctl")
        .args(["start", "docker"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if matches!(&status, Ok(s) if s.success()) && docker::daemon_running() {
        ui::ok("docker daemon started successfully");
        true
    } else {
        ui::fail("docker daemon is not running and could not be started; please start it manually");
        false
    }
}

fn check_internet() -> bool {
    if !binary_in_path("ping") {
        ui::fail("cannot verify connectivity: 'ping' is required for virtctl");
        return false;
    }
    let status = Command::new("ping")
        .args(["-c", "3", "-W", "2", "8.8.8.8"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if matches!(&status, Ok(s) if s.success()) {
        ui::ok("internet connectivity");
        true
    } else {
        ui::fail("no internet connectivity to 8.8.8.8");
        false
    }
}

fn binary_in_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    for dir in path.split(':').filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir).join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return true;
        }
    }
    false
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
