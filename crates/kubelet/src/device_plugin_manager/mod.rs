pub mod manager;
pub mod node_patcher;
pub mod resources;
pub mod pod_devices;
use crate::device_plugin_api::v1beta1::Device;
use std::collections::{HashMap, HashSet};

/// `DeviceIdMap` contains ... TODO
type DeviceIdMap = HashMap<String,EndpointDeviceIds>;

/// EndpointDeviceIds contains the IDs of all the devices advertised by a single device plugin
type EndpointDeviceIds = HashSet<String>;

/// `DeviceMap` contains all the devices advertised by all device plugins. Key is resource name.
type DeviceMap = HashMap<String,EndpointDevicesMap>;

/// EndpointDevicesMap contains all of the devices advertised by a single device plugin. Key is device ID.
type EndpointDevicesMap = HashMap<String, Device>;

/// Healthy means the device is allocatable (whether already allocated or not)
pub const HEALTHY: &str = "Healthy";

/// Unhealthy means the device is not allocatable
pub const UNHEALTHY: &str = "Unhealthy";

// // Returns the set all healthy devices
// fn get_healthy_devices(devices: Arc<Mutex<DeviceMap>>) -> EndpointDeviceIds {

// }