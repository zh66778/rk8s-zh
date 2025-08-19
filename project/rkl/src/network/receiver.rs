use anyhow::Result;
use log::{info, warn, error};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::network::{
    config::NetworkConfig,
    route::{RouteConfig, RouteManager, RouteReceiver},
    subnet::SubnetReceiver,
};

/// Main network configuration receiver that coordinates subnet and route configuration
/// This will be the primary interface for receiving network configurations from rks
pub struct NetworkReceiver {
    subnet_receiver: SubnetReceiver,
    route_receiver: RouteReceiver,
    route_manager: Arc<Mutex<RouteManager>>,
    node_id: String,
}

impl NetworkReceiver {
    pub fn new(
        subnet_file_path: String,
        link_index: u32,
        backend_type: String,
        node_id: String,
    ) -> Self {
        let route_manager = Arc::new(Mutex::new(RouteManager::new(link_index, backend_type)));
        let subnet_receiver = SubnetReceiver::new(subnet_file_path);
        let route_receiver = RouteReceiver::new(route_manager.clone());

        Self {
            subnet_receiver,
            route_receiver,
            route_manager,
            node_id,
        }
    }

    /// Handle received network configuration from rks
    /// This includes both subnet.env and route configurations
    pub async fn handle_network_config(
        &self,
        config: NetworkConfigMessage,
    ) -> Result<()> {
        info!("Received network configuration from rks for node: {}", self.node_id);

        match config {
            NetworkConfigMessage::SubnetConfig {
                network_config,
                ip_masq,
                ipv4_subnet,
                ipv6_subnet,
                mtu,
            } => {
                self.subnet_receiver
                    .handle_subnet_config(&network_config, ip_masq, ipv4_subnet, ipv6_subnet, mtu)
                    .await?;
            }
            NetworkConfigMessage::RouteConfig { routes } => {
                self.route_receiver.handle_route_config(routes).await?;
            }
            NetworkConfigMessage::FullConfig {
                network_config,
                ip_masq,
                ipv4_subnet,
                ipv6_subnet,
                mtu,
                routes,
            } => {
                // Handle both subnet and route configuration
                self.subnet_receiver
                    .handle_subnet_config(&network_config, ip_masq, ipv4_subnet, ipv6_subnet, mtu)
                    .await?;

                self.route_receiver.handle_route_config(routes).await?;
            }
        }

        info!("Network configuration applied successfully for node: {}", self.node_id);
        Ok(())
    }

    /// Start the network receiver service
    /// This will listen for network configurations from rks
    pub async fn start_service(&self) -> Result<()> {
        info!("Starting network receiver service for node: {}", self.node_id);

        // TODO: Implement QUIC server/client for communication with rks
        // For now, this is just a placeholder
        warn!("QUIC communication service not yet implemented");

        // Start route checking task
        let route_manager = self.route_manager.clone();
        let (shutdown_tx, shutdown_rx) = tokio::sync::mpsc::channel(1);
        
        tokio::spawn(async move {
            let manager = route_manager.lock().await;
            // Convert to Arc for the route check task
            // TODO: This needs to be refactored to properly handle the mutex
            warn!("Route checking task disabled due to mutex constraint");
            drop(manager);
        });

        info!("Network receiver service started for node: {}", self.node_id);
        Ok(())
    }

    /// Stop the network receiver service
    pub async fn stop_service(&self) -> Result<()> {
        info!("Stopping network receiver service for node: {}", self.node_id);
        // TODO: Implement proper service shutdown
        Ok(())
    }

    /// Get the current node ID
    pub fn get_node_id(&self) -> &str {
        &self.node_id
    }

    /// Get route manager for direct access (if needed)
    pub fn get_route_manager(&self) -> Arc<Mutex<RouteManager>> {
        self.route_manager.clone()
    }
}

/// Network configuration message types that can be received from rks
#[derive(Debug, Clone)]
pub enum NetworkConfigMessage {
    /// Subnet configuration only
    SubnetConfig {
        network_config: NetworkConfig,
        ip_masq: bool,
        ipv4_subnet: Option<ipnetwork::Ipv4Network>,
        ipv6_subnet: Option<ipnetwork::Ipv6Network>,
        mtu: u32,
    },
    /// Route configuration only
    RouteConfig {
        routes: Vec<RouteConfig>,
    },
    /// Full network configuration (subnet + routes)
    FullConfig {
        network_config: NetworkConfig,
        ip_masq: bool,
        ipv4_subnet: Option<ipnetwork::Ipv4Network>,
        ipv6_subnet: Option<ipnetwork::Ipv6Network>,
        mtu: u32,
        routes: Vec<RouteConfig>,
    },
}

/// Network receiver builder for easier configuration
pub struct NetworkReceiverBuilder {
    subnet_file_path: Option<String>,
    link_index: Option<u32>,
    backend_type: Option<String>,
    node_id: Option<String>,
}

impl NetworkReceiverBuilder {
    pub fn new() -> Self {
        Self {
            subnet_file_path: None,
            link_index: None,
            backend_type: None,
            node_id: None,
        }
    }

    pub fn subnet_file_path(mut self, path: String) -> Self {
        self.subnet_file_path = Some(path);
        self
    }

    pub fn link_index(mut self, index: u32) -> Self {
        self.link_index = Some(index);
        self
    }

    pub fn backend_type(mut self, backend: String) -> Self {
        self.backend_type = Some(backend);
        self
    }

    pub fn node_id(mut self, id: String) -> Self {
        self.node_id = Some(id);
        self
    }

    pub fn build(self) -> Result<NetworkReceiver> {
        let subnet_file_path = self
            .subnet_file_path
            .unwrap_or_else(|| "/etc/cni/net.d/subnet.env".to_string());
        let link_index = self.link_index.unwrap_or(1);
        let backend_type = self.backend_type.unwrap_or_else(|| "hostgw".to_string());
        let node_id = self
            .node_id
            .ok_or_else(|| anyhow::anyhow!("Node ID is required"))?;

        Ok(NetworkReceiver::new(
            subnet_file_path,
            link_index,
            backend_type,
            node_id,
        ))
    }
}

impl Default for NetworkReceiverBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Network service manager for rkl
/// This provides a high-level interface for managing network configuration
pub struct NetworkService {
    receiver: NetworkReceiver,
    is_running: Arc<Mutex<bool>>,
}

impl NetworkService {
    pub fn new(receiver: NetworkReceiver) -> Self {
        Self {
            receiver,
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// Start the network service
    pub async fn start(&self) -> Result<()> {
        let mut running = self.is_running.lock().await;
        if *running {
            warn!("Network service is already running");
            return Ok(());
        }

        info!("Starting network service");
        self.receiver.start_service().await?;
        *running = true;
        info!("Network service started successfully");
        Ok(())
    }

    /// Stop the network service
    pub async fn stop(&self) -> Result<()> {
        let mut running = self.is_running.lock().await;
        if !*running {
            warn!("Network service is not running");
            return Ok(());
        }

        info!("Stopping network service");
        self.receiver.stop_service().await?;
        *running = false;
        info!("Network service stopped successfully");
        Ok(())
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        *self.is_running.lock().await
    }

    /// Get the network receiver
    pub fn get_receiver(&self) -> &NetworkReceiver {
        &self.receiver
    }

    /// Handle a network configuration message
    pub async fn handle_config(&self, config: NetworkConfigMessage) -> Result<()> {
        self.receiver.handle_network_config(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_network_receiver_builder() {
        let receiver = NetworkReceiverBuilder::new()
            .subnet_file_path("/tmp/subnet.env".to_string())
            .link_index(2)
            .backend_type("overlay".to_string())
            .node_id("test-node-1".to_string())
            .build()
            .unwrap();

        assert_eq!(receiver.get_node_id(), "test-node-1");
    }

    #[test]
    fn test_network_receiver_builder_defaults() {
        let receiver = NetworkReceiverBuilder::new()
            .node_id("test-node-2".to_string())
            .build()
            .unwrap();

        assert_eq!(receiver.get_node_id(), "test-node-2");
    }

    #[test]
    fn test_network_receiver_builder_missing_node_id() {
        let result = NetworkReceiverBuilder::new().build();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_network_service_lifecycle() {
        let receiver = NetworkReceiverBuilder::new()
            .node_id("test-node-3".to_string())
            .build()
            .unwrap();

        let service = NetworkService::new(receiver);
        
        assert!(!service.is_running().await);
        
        service.start().await.unwrap();
        assert!(service.is_running().await);
        
        service.stop().await.unwrap();
        assert!(!service.is_running().await);
    }

    #[tokio::test]
    async fn test_network_config_message_handling() {
        let dir = tempdir().unwrap();
        let subnet_file = dir.path().join("subnet.env");

        let receiver = NetworkReceiverBuilder::new()
            .subnet_file_path(subnet_file.to_string_lossy().to_string())
            .node_id("test-node-4".to_string())
            .build()
            .unwrap();

        let config_msg = NetworkConfigMessage::SubnetConfig {
            network_config: NetworkConfig {
                enable_ipv4: true,
                enable_ipv6: false,
                network: Some("10.0.0.0/16".parse().unwrap()),
                ..Default::default()
            },
            ip_masq: true,
            ipv4_subnet: Some("10.0.1.0/24".parse().unwrap()),
            ipv6_subnet: None,
            mtu: 1500,
        };

        let result = receiver.handle_network_config(config_msg).await;
        assert!(result.is_ok());
    }
}
