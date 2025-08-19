use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use ipnetwork::{Ipv4Network, Ipv6Network};
use lazy_static::lazy_static;
use log::{error, info, warn};
use regex::Regex;

use crate::network::config::NetworkConfig;

lazy_static! {
    static ref SUBNET_REGEX: Regex =
        Regex::new(r"(\d+\.\d+\.\d+\.\d+)-(\d+)(?:&([a-f\d:]+)-(\d+))?").unwrap();
}

/// Parse subnet key into IPv4 and optional IPv6 networks
pub fn parse_subnet_key(s: &str) -> Option<(Ipv4Network, Option<Ipv6Network>)> {
    if let Some(caps) = SUBNET_REGEX.captures(s) {
        let ipv4: Ipv4Addr = caps[1].parse().ok()?;
        let ipv4_prefix: u8 = caps[2].parse().ok()?;
        let ipv4_net = Ipv4Network::new(ipv4, ipv4_prefix).ok()?;

        let ipv6_net = if let (Some(ipv6_str), Some(prefix_str)) = (caps.get(3), caps.get(4)) {
            let ipv6: Ipv6Addr = ipv6_str.as_str().parse().ok()?;
            let prefix: u8 = prefix_str.as_str().parse().ok()?;
            Some(Ipv6Network::new(ipv6, prefix).ok()?)
        } else {
            None
        };

        Some((ipv4_net, ipv6_net))
    } else {
        None
    }
}

/// Create subnet key from IPv4 and optional IPv6 networks
pub fn make_subnet_key(sn4: &Ipv4Network, sn6: Option<&Ipv6Network>) -> String {
    match sn6 {
        Some(v6) => format!(
            "{}&{}",
            sn4.to_string().replace("/", "-"),
            v6.to_string().replace("/", "-")
        ),
        None => sn4.to_string().replace("/", "-"),
    }
}

/// Write subnet configuration to file (received from rks)
pub fn write_subnet_file<P: AsRef<Path>>(
    path: P,
    config: &NetworkConfig,
    ip_masq: bool,
    mut sn4: Option<Ipv4Network>,
    mut sn6: Option<Ipv6Network>,
    mtu: u32,
) -> Result<()> {
    let path = path.as_ref();
    let (dir, name) = (
        path.parent().context("Missing parent directory")?,
        path.file_name().context("Missing file name")?,
    );
    fs::create_dir_all(dir)?;

    let temp_file = dir.join(format!(".{}", name.to_string_lossy()));
    let mut contents = String::new();

    if config.enable_ipv4
        && let Some(ref mut net) = sn4
    {
        contents += &format!("RKL_NETWORK={}\n", config.network.unwrap());
        contents += &format!("RKL_SUBNET={}/{}\n", net.ip(), net.prefix());
    }

    if config.enable_ipv6
        && let Some(ref mut net) = sn6
    {
        contents += &format!("RKL_IPV6_NETWORK={}\n", config.ipv6_network.unwrap());
        contents += &format!("RKL_IPV6_SUBNET={}/{}\n", net.ip(), net.prefix());
    }

    contents += &format!("RKL_MTU={mtu}\n");
    contents += &format!("RKL_IPMASQ={ip_masq}\n");

    fs::write(&temp_file, contents)?;
    fs::rename(&temp_file, path)?;

    info!("Subnet configuration written to: {}", path.display());
    Ok(())
}

/// Read IPv4 CIDRs from subnet file
pub fn read_cidrs_from_subnet_file(path: &str, cidr_key: &str) -> Vec<Ipv4Network> {
    let mut cidrs = Vec::new();
    if !Path::new(path).exists() {
        return cidrs;
    }

    match dotenvy::from_path_iter(path) {
        Ok(iter) => {
            for (key, value) in iter.flatten() {
                if key == cidr_key {
                    for s in value.split(',') {
                        match Ipv4Network::from_str(s.trim()) {
                            Ok(cidr) => cidrs.push(cidr),
                            Err(e) => error!(
                                "Couldn't parse previous {cidr_key} from subnet file at {path}: {e}"
                            ),
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Couldn't fetch previous {cidr_key} from subnet file at {path}: {e}");
        }
    }

    cidrs
}

/// Read single IPv6 CIDR from subnet file
pub fn read_ip6_cidr_from_subnet_file(path: &str, cidr_key: &str) -> Option<Ipv6Network> {
    let cidrs = read_ip6_cidrs_from_subnet_file(path, cidr_key);
    match cidrs.len() {
        0 => {
            warn!("no subnet found for key: {cidr_key} in file: {path}");
            None
        }
        1 => Some(cidrs[0]),
        _ => {
            error!(
                "error reading subnet: more than 1 entry found for key: {cidr_key} in file: {path}"
            );
            None
        }
    }
}

/// Read IPv6 CIDRs from subnet file
pub fn read_ip6_cidrs_from_subnet_file(path: &str, cidr_key: &str) -> Vec<Ipv6Network> {
    let mut cidrs = Vec::new();
    if !Path::new(path).exists() {
        return cidrs;
    }

    match dotenvy::from_path_iter(path) {
        Ok(iter) => {
            for (key, value) in iter.flatten() {
                if key == cidr_key {
                    for s in value.split(',') {
                        match Ipv6Network::from_str(s.trim()) {
                            Ok(cidr) => cidrs.push(cidr),
                            Err(e) => error!(
                                "Couldn't parse previous {cidr_key} from subnet file at {path}: {e}"
                            ),
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!("Couldn't fetch previous {cidr_key} from subnet file at {path}: {e}");
        }
    }

    cidrs
}

/// Subnet configuration receiver
/// This will be called when receiving subnet.env configuration from rks
/// TODO: This will be integrated with QUIC communication from rks
pub struct SubnetReceiver {
    pub subnet_file_path: String,
}

impl SubnetReceiver {
    pub fn new(subnet_file_path: String) -> Self {
        Self { subnet_file_path }
    }

    /// Handle received subnet configuration from rks
    /// This function will be called when rks sends subnet.env configuration
    pub async fn handle_subnet_config(
        &self,
        config: &NetworkConfig,
        ip_masq: bool,
        sn4: Option<Ipv4Network>,
        sn6: Option<Ipv6Network>,
        mtu: u32,
    ) -> Result<()> {
        info!("Received subnet configuration from rks");
        
        write_subnet_file(
            &self.subnet_file_path,
            config,
            ip_masq,
            sn4,
            sn6,
            mtu,
        )?;

        info!("Subnet configuration applied successfully");
        Ok(())
    }

    /// TODO: Placeholder for receiving subnet.env from rks via QUIC
    /// This will be implemented when QUIC communication is set up
    pub async fn receive_from_rks(&self) -> Result<()> {
        // TODO: Implement QUIC communication to receive subnet.env from rks
        // For now, this is just a placeholder
        warn!("QUIC communication with rks not yet implemented");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_subnet_key() {
        // IPv4 only
        let result = parse_subnet_key("10.0.1.0-24");
        assert!(result.is_some());
        let (ipv4, ipv6) = result.unwrap();
        assert_eq!(ipv4.to_string(), "10.0.1.0/24");
        assert!(ipv6.is_none());

        // IPv4 + IPv6
        let result = parse_subnet_key("10.0.1.0-24&fc00::-64");
        assert!(result.is_some());
        let (ipv4, ipv6) = result.unwrap();
        assert_eq!(ipv4.to_string(), "10.0.1.0/24");
        assert_eq!(ipv6.unwrap().to_string(), "fc00::/64");
    }

    #[test]
    fn test_make_subnet_key() {
        let ipv4: Ipv4Network = "10.0.1.0/24".parse().unwrap();
        let ipv6: Ipv6Network = "fc00::/64".parse().unwrap();

        // IPv4 only
        let key = make_subnet_key(&ipv4, None);
        assert_eq!(key, "10.0.1.0-24");

        // IPv4 + IPv6
        let key = make_subnet_key(&ipv4, Some(&ipv6));
        assert_eq!(key, "10.0.1.0-24&fc00::-64");
    }

    #[test]
    fn test_write_subnet_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("subnet.env");

        let config = NetworkConfig {
            enable_ipv4: true,
            enable_ipv6: false,
            network: Some("10.0.0.0/16".parse().unwrap()),
            ..Default::default()
        };

        let subnet: Ipv4Network = "10.0.1.0/24".parse().unwrap();
        
        write_subnet_file(&file_path, &config, true, Some(subnet), None, 1500).unwrap();
        
        let contents = fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("RKL_NETWORK=10.0.0.0/16"));
        assert!(contents.contains("RKL_SUBNET=10.0.1.0/24"));
        assert!(contents.contains("RKL_MTU=1500"));
        assert!(contents.contains("RKL_IPMASQ=true"));
    }
}
