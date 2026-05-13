use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("name '{0}' is invalid; allowed: lowercase letters, digits, '-' and '_', 1-63 chars")]
    InvalidName(String),
    #[error("subnet '{0}' is invalid: {1}")]
    InvalidSubnet(String, String),
    #[error("gateway '{gateway}' is not within subnet '{subnet}'")]
    GatewayNotInSubnet { gateway: String, subnet: String },
    #[error("IP '{ip}' is not within subnet '{subnet}'")]
    IpNotInSubnet { ip: String, subnet: String },
    #[error("IP '{0}' is already assigned to another VM")]
    IpAlreadyUsed(String),
    #[error("no free IP address available in subnet '{0}'")]
    NoFreeIp(String),
    #[error("cpu value '{0}' is invalid; must be a positive number")]
    InvalidCpu(String),
    #[error("memory value '{0}' is invalid: {1}")]
    InvalidMemory(String, String),
    #[error("size value '{0}' is invalid: {1}")]
    InvalidSize(String, String),
    #[error("requested {requested_bytes} bytes but only {available_bytes} bytes available on host")]
    InsufficientDiskSpace {
        requested_bytes: u64,
        available_bytes: u64,
    },
}

pub fn validate_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() || name.len() > 63 {
        return Err(ValidationError::InvalidName(name.to_string()));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(ValidationError::InvalidName(name.to_string()));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(ValidationError::InvalidName(name.to_string()));
    }
    Ok(())
}
