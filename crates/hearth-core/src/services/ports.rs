//! Free ports for services. A port counts as taken when Hearth cannot bind it on 127.0.0.1 or
//! when something already answers there: a database outside Hearth listening on all addresses
//! (a MariaDB Windows service on 3306) does not always block a bind to 127.0.0.1 on Windows.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use crate::services::kinds;
use crate::services::store;
use crate::{CoreError, HearthPaths};

pub fn port_free(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    if TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok() {
        return false;
    }
    TcpListener::bind(addr).is_ok()
}

/// Ports reserved by Hearth's own instances (running or not), except `own_id`'s.
fn reserved(paths: &HearthPaths, own_id: Option<&str>) -> Vec<(u16, String)> {
    store::load(paths)
        .into_iter()
        .filter(|i| Some(i.id.as_str()) != own_id)
        .flat_map(|i| kinds::ports(&i).into_iter().map(move |p| (p, i.name.clone())))
        .collect()
}

/// The first port from `start` upward where `count` ports in a row are free and unreserved.
pub fn suggest_from(paths: &HearthPaths, start: u16, count: u16) -> u16 {
    let reserved: Vec<u16> = reserved(paths, None).into_iter().map(|(p, _)| p).collect();
    let usable = |p: u16| !reserved.contains(&p) && port_free(p);
    (start..start.saturating_add(200))
        .find(|&p| (0..count).all(|i| p.checked_add(i).is_some_and(usable)))
        .unwrap_or(start)
}

/// A free port for a new instance of `kind`, from its usual one upward.
pub fn suggest(paths: &HearthPaths, kind: &str) -> u16 {
    let count = if matches!(kind, "silo" | "typesense") { 2 } else { 1 };
    suggest_from(paths, kinds::default_port(kind), count)
}

/// Fails with what is in the way when one of `ports` is taken (another instance, or another program).
pub fn check(paths: &HearthPaths, ports: &[u16], own_id: Option<&str>) -> Result<(), CoreError> {
    let reserved = reserved(paths, own_id);
    for &port in ports {
        if let Some((_, name)) = reserved.iter().find(|(p, _)| *p == port) {
            return Err(CoreError::Php(format!("Port {port} is used by {name}")));
        }
        if !port_free(port) {
            let next = suggest_from(paths, port.saturating_add(1), ports.len() as u16);
            return Err(CoreError::Php(format!("Port {port} is used by another program ({next} is free)")));
        }
    }
    Ok(())
}
