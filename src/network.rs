use std::thread;
use std::process;
use std::time::Duration;
use std::sync::mpsc::{channel, Sender};
use std::error::Error;
use std::net::Ipv4Addr;

use network_manager::{AccessPoint, Connection, ConnectionState, Connectivity, Device, DeviceType,
                      NetworkManager, ServiceState};

use {exit, ExitResult};
use config::Config;
use dnsmasq::start_dnsmasq;
use server::start_server;

pub enum NetworkCommand {
    Activate,
    Timeout,
    Connect { ssid: String, passphrase: String },
}

pub enum NetworkCommandResponse {
    AccessPointsSsids(Vec<String>),
}

#[cfg_attr(feature = "cargo-clippy", allow(cyclomatic_complexity))]
pub fn process_network_commands(config: &Config, exit_tx: &Sender<ExitResult>) {
    let manager = NetworkManager::new();
    debug!("Network Manager connection initialized");

    let device = match find_device(&manager, &config.interface) {
        Ok(device) => device,
        Err(e) => {
            return exit(exit_tx, e);
        },
    };

    let mut access_points = match get_access_points(&device) {
        Ok(access_points) => access_points,
        Err(e) => {
            return exit(exit_tx, format!("Getting access points failed: {}", e));
        },
    };

    let portal_ssid = &config.ssid;
    let portal_passphrase = config.passphrase.as_ref().map(|p| p as &str);

    let mut portal_connection =
        match create_portal(&device, &config.ssid, &config.gateway, &portal_passphrase) {
            Ok(connection) => Some(connection),
            Err(e) => {
                return exit(exit_tx, format!("Creating the access point failed: {}", e));
            },
        };

    let dnsmasq = start_dnsmasq(config, &device).unwrap();

    let (server_tx, server_rx) = channel();
    let (network_tx, network_rx) = channel();

    let exit_tx_server = exit_tx.clone();
    let network_tx_timeout = network_tx.clone();
    let gateway = config.gateway;
    let ui_directory = config.ui_directory.clone();
    let activity_timeout = config.activity_timeout;

    thread::spawn(move || {
        start_server(gateway, server_rx, network_tx, exit_tx_server, &ui_directory);
    });

    if config.activity_timeout != 0 {
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(activity_timeout));

            if let Err(err) = network_tx_timeout.send(NetworkCommand::Timeout) {
                error!("Sending NetworkCommand::Timeout failed: {}", err.description());
            }
        });
    }

    let mut activated = false;

    loop {
        let command = match network_rx.recv() {
            Ok(command) => command,
            Err(e) => {
                // Sleep for a second, so that other threads may log error info.
                thread::sleep(Duration::from_secs(1));
                return exit_with_error(
                    exit_tx,
                    dnsmasq,
                    portal_connection,
                    portal_ssid,
                    format!("Receiving network command failed: {}", e.description()),
                );
            },
        };

        match command {
            NetworkCommand::Activate => {
                activated = true;

                let access_points_ssids = get_access_points_ssids_owned(&access_points);

                if let Err(e) = server_tx.send(NetworkCommandResponse::AccessPointsSsids(
                    access_points_ssids,
                )) {
                    return exit_with_error(
                        exit_tx,
                        dnsmasq,
                        portal_connection,
                        portal_ssid,
                        format!(
                            "Sending access point ssids results failed: {}",
                            e.description()
                        ),
                    );
                }
            },
            NetworkCommand::Timeout => {
                if activated == false {
                    info!("Timeout reached. Exiting...");

                    return exit_ok(
                        exit_tx,
                        dnsmasq,
                        portal_connection,
                        portal_ssid,
                    );
                }
            },
            NetworkCommand::Connect { ssid, passphrase } => {
                if let Some(connection) = portal_connection {
                    let result = stop_portal(&connection, &config.ssid);
                    if let Err(e) = result {
                        return exit_with_error(
                            exit_tx,
                            dnsmasq,
                            Some(connection),
                            portal_ssid,
                            format!("Stopping the access point failed: {}", e),
                        );
                    }
                    portal_connection = None;
                }

                access_points = match get_access_points(&device) {
                    Ok(access_points) => access_points,
                    Err(e) => {
                        return exit_with_error(
                            exit_tx,
                            dnsmasq,
                            portal_connection,
                            portal_ssid,
                            format!("Getting access points failed: {}", e),
                        );
                    },
                };

                {
                    let (access_point, access_point_ssid) =
                        find_access_point(&access_points, &ssid).unwrap();

                    let wifi_device = device.as_wifi_device().unwrap();

                    info!("Connecting to access point '{}'...", access_point_ssid);

                    match wifi_device.connect(access_point, &passphrase as &str) {
                        Ok((connection, state)) => {
                            if state == ConnectionState::Activated {
                                match wait_for_connectivity(&manager, 20) {
                                    Ok(has_connectivity) => {
                                        if has_connectivity {
                                            info!("Connectivity established");

                                            return exit_ok(
                                                exit_tx,
                                                dnsmasq,
                                                portal_connection,
                                                portal_ssid,
                                            );
                                        } else {
                                            warn!("Cannot establish connectivity");
                                        }
                                    },
                                    Err(err) => error!("Getting connectivity failed: {}", err),
                                }
                            }

                            if let Err(err) = connection.delete() {
                                error!("Deleting connection object failed: {}", err)
                            }

                            warn!(
                                "Connection to access point not activated '{}': {:?}",
                                access_point_ssid, state
                            );
                        },
                        Err(e) => {
                            warn!(
                                "Error connecting to access point '{}': {}",
                                access_point_ssid, e
                            );
                        },
                    }
                }

                access_points = match get_access_points(&device) {
                    Ok(access_points) => access_points,
                    Err(e) => {
                        return exit_with_error(
                            exit_tx,
                            dnsmasq,
                            portal_connection,
                            portal_ssid,
                            format!("Getting access points failed: {}", e),
                        );
                    },
                };

                portal_connection =
                    match create_portal(&device, &config.ssid, &config.gateway, &portal_passphrase)
                    {
                        Ok(connection) => Some(connection),
                        Err(e) => {
                            return exit_with_error(
                                exit_tx,
                                dnsmasq,
                                portal_connection,
                                portal_ssid,
                                format!("Creating the access point failed: {}", e),
                            );
                        },
                    };
            },
        }
    }
}

pub fn init_networking() {
    start_network_manager_service();

    if let Err(err) = stop_access_point() {
        error!("Stopping access point failed: {}", err);
        process::exit(1);
    }
}

pub fn find_device(manager: &NetworkManager, interface: &Option<String>) -> Result<Device, String> {
    if let Some(ref interface) = *interface {
        let device = manager.get_device_by_interface(interface)?;

        if *device.device_type() == DeviceType::WiFi {
            info!("Targeted WiFi device: {}", interface);
            Ok(device)
        } else {
            Err(format!("Not a WiFi device: {}", interface))
        }
    } else {
        let devices = manager.get_devices()?;

        let index = devices
            .iter()
            .position(|d| *d.device_type() == DeviceType::WiFi);

        if let Some(index) = index {
            info!("WiFi device: {}", devices[index].interface());
            Ok(devices[index].clone())
        } else {
            Err("Cannot find a WiFi device".to_string())
        }
    }
}

fn get_access_points(device: &Device) -> Result<Vec<AccessPoint>, String> {
    let retries_allowed = 10;
    let mut retries = 0;

    // After stopping the hotspot we may have to wait a bit for the list
    // of access points to become available
    while retries < retries_allowed {
        let wifi_device = device.as_wifi_device().unwrap();
        let mut access_points = wifi_device.get_access_points()?;

        access_points.retain(|ap| ap.ssid().as_str().is_ok());

        if !access_points.is_empty() {
            info!(
                "Access points: {:?}",
                get_access_points_ssids(&access_points)
            );
            return Ok(access_points);
        }

        retries += 1;
        debug!("No access points found - retry #{}", retries);
        thread::sleep(Duration::from_secs(1));
    }

    warn!("No access points found - giving up...");
    Ok(vec![])
}

fn get_access_points_ssids(access_points: &[AccessPoint]) -> Vec<&str> {
    access_points
        .iter()
        .map(|ap| ap.ssid().as_str().unwrap())
        .collect()
}

fn get_access_points_ssids_owned(access_points: &[AccessPoint]) -> Vec<String> {
    access_points
        .iter()
        .map(|ap| ap.ssid().as_str().unwrap().to_string())
        .collect()
}

fn find_access_point<'a>(
    access_points: &'a [AccessPoint],
    ssid: &str,
) -> Option<(&'a AccessPoint, &'a str)> {
    for access_point in access_points.iter() {
        if let Ok(access_point_ssid) = access_point.ssid().as_str() {
            if access_point_ssid == ssid {
                return Some((access_point, access_point_ssid));
            }
        }
    }

    None
}

fn create_portal(
    device: &Device,
    ssid: &str,
    gateway: &Ipv4Addr,
    passphrase: &Option<&str>,
) -> Result<Connection, String> {
    info!("Starting access point...");
    let wifi_device = device.as_wifi_device().unwrap();
    let (portal_connection, _) = wifi_device.create_hotspot(ssid, *passphrase, Some(*gateway))?;
    info!("Access point '{}' created", ssid);
    Ok(portal_connection)
}

fn stop_portal(connection: &Connection, ssid: &str) -> Result<(), String> {
    info!("Stopping access point '{}'...", ssid);
    connection.deactivate()?;
    connection.delete()?;
    thread::sleep(Duration::from_secs(1));
    info!("Access point '{}' stopped", ssid);
    Ok(())
}

fn exit_with_error(
    exit_tx: &Sender<ExitResult>,
    dnsmasq: process::Child,
    connection: Option<Connection>,
    ssid: &str,
    error: String,
) {
    exit_with_result(exit_tx, dnsmasq, connection, ssid, Err(error));
}

fn exit_ok(
    exit_tx: &Sender<ExitResult>,
    dnsmasq: process::Child,
    connection: Option<Connection>,
    ssid: &str,
) {
    exit_with_result(exit_tx, dnsmasq, connection, ssid, Ok(()));
}

fn exit_with_result(
    exit_tx: &Sender<ExitResult>,
    mut dnsmasq: process::Child,
    connection: Option<Connection>,
    ssid: &str,
    result: ExitResult,
) {
    let _ = dnsmasq.kill();

    if let Some(connection) = connection {
        let _ = stop_portal(&connection, ssid);
    }

    let _ = exit_tx.send(result);
}

fn wait_for_connectivity(manager: &NetworkManager, timeout: u64) -> Result<bool, String> {
    let mut total_time = 0;

    loop {
        let connectivity = manager.get_connectivity()?;

        if connectivity == Connectivity::Full || connectivity == Connectivity::Limited {
            debug!(
                "Connectivity established: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(true);
        } else if total_time >= timeout {
            debug!(
                "Timeout reached in waiting for connectivity: {:?} / {}s elapsed",
                connectivity, total_time
            );

            return Ok(false);
        }

        ::std::thread::sleep(::std::time::Duration::from_secs(1));

        total_time += 1;

        debug!(
            "Still waiting for connectivity: {:?} / {}s elapsed",
            connectivity, total_time
        );
    }
}

pub fn start_network_manager_service() {
    match NetworkManager::get_service_state() {
        Ok(state) => {
            if state != ServiceState::Active {
                match NetworkManager::start_service(15) {
                    Ok(state) => {
                        if state != ServiceState::Active {
                            error!(
                                "Cannot start the NetworkManager service with active state: {:?}",
                                state
                            );
                            process::exit(1);
                        } else {
                            info!("NetworkManager service started successfully");
                        }
                    },
                    Err(err) => {
                        error!(
                            "Starting the NetworkManager service state failed: {:?}",
                            err
                        );
                        process::exit(1);
                    },
                }
            } else {
                debug!("NetworkManager service already running");
            }
        },
        Err(err) => {
            error!("Getting the NetworkManager service state failed: {:?}", err);
            process::exit(1);
        },
    }
}

fn stop_access_point() -> Result<(), String> {
    let manager = NetworkManager::new();

    let connections = manager.get_active_connections()?;

    for connection in connections {
        if &connection.settings().kind == "802-11-wireless" && &connection.settings().mode == "ap" {
            debug!(
                "Deleting active access point connection profile to {:?}",
                connection.settings().ssid,
            );
            connection.delete()?;
        }
    }

    Ok(())
}
