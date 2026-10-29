//! Network interface discovery and QR code pairing matrix for connecting Android/Web
//! clients to the embedded `gateterm-server`, aligned with `crates/desktop/src/settings/pairing.rs`.

use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkAddress {
    pub ip: IpAddr,
    pub interface: String,
}

impl NetworkAddress {
    pub fn label(&self) -> String {
        format!("{} · {}", self.ip, self.interface)
    }
}

#[derive(Clone, Debug)]
pub struct QrMatrix {
    pub size: usize,
    pub modules: Vec<bool>,
}

impl QrMatrix {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        if x < self.size && y < self.size {
            self.modules[y * self.size + x]
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PairingState {
    pub listen: Option<SocketAddr>,
    pub token: String,
    pub addresses: Vec<NetworkAddress>,
    pub selected: Option<IpAddr>,
    pub interface_error: bool,
    pub cached_qr: Option<(String, QrMatrix)>,
}

impl PairingState {
    pub fn new(listen: Option<SocketAddr>, token: Option<&str>) -> Self {
        let mut s = Self::default();
        s.refresh(listen, token);
        s
    }

    pub fn refresh(&mut self, listen: Option<SocketAddr>, token: Option<&str>) {
        self.listen = listen;
        self.token = token.unwrap_or_default().into();
        let interfaces = if_addrs::get_if_addrs().map(|ifaces| {
            ifaces
                .into_iter()
                .filter(|iface| iface.is_oper_up())
                .map(|iface| NetworkAddress {
                    ip: iface.ip(),
                    interface: iface.name,
                })
                .collect::<Vec<_>>()
        });
        self.interface_error = interfaces.is_err();
        let preferred = listen.and_then(|l| probe_default_route_ip(l.is_ipv6()));
        self.addresses =
            candidates_with_preferred(listen, interfaces.unwrap_or_default(), preferred);
        self.selected = self
            .selected
            .filter(|ip| self.addresses.iter().any(|a| a.ip == *ip))
            .or_else(|| self.addresses.first().map(|a| a.ip));
        self.update_qr();
    }

    pub fn select_ip(&mut self, ip: IpAddr) {
        self.selected = Some(ip);
        self.update_qr();
    }

    pub fn selected_connection_url(&self) -> Option<String> {
        let listen = self.listen.filter(|_| !self.token.is_empty())?;
        let ip = self.selected?;
        Some(connection_url(listen, ip, &self.token))
    }

    pub fn selected_host_origin(&self) -> Option<String> {
        let listen = self.listen?;
        let ip = self.selected?;
        Some(format!("http://{}", SocketAddr::new(ip, listen.port())))
    }

    fn update_qr(&mut self) {
        if let Some(url) = self.selected_connection_url() {
            if self.cached_qr.as_ref().is_none_or(|(u, _)| *u != url) {
                self.cached_qr = qr_matrix(&url).ok().map(|m| (url, m));
            }
        } else {
            self.cached_qr = None;
        }
    }
}

pub fn candidates(listen: Option<SocketAddr>, addresses: Vec<NetworkAddress>) -> Vec<NetworkAddress> {
    candidates_with_preferred(listen, addresses, None)
}

pub fn candidates_with_preferred(
    listen: Option<SocketAddr>,
    addresses: Vec<NetworkAddress>,
    preferred_ip: Option<IpAddr>,
) -> Vec<NetworkAddress> {
    let Some(listen) = listen else { return vec![] };
    let mut result: Vec<_> = addresses
        .into_iter()
        .filter(|a| {
            let reachable = match a.ip {
                IpAddr::V4(ip) => {
                    !ip.is_unspecified()
                        && !ip.is_loopback()
                        && !ip.is_multicast()
                        && !ip.is_broadcast()
                }
                IpAddr::V6(ip) => {
                    !ip.is_unspecified()
                        && !ip.is_loopback()
                        && !ip.is_multicast()
                        && !ip.is_unicast_link_local()
                }
            };
            reachable
                && if listen.ip().is_unspecified() {
                    a.ip.is_ipv4() == listen.is_ipv4()
                } else {
                    a.ip == listen.ip()
                }
        })
        .collect();
    result.sort_by_key(|a| {
        (
            matches!(a.ip, IpAddr::V4(ip) if ip.is_link_local()),
            is_virtual_interface(&a.interface),
            preferred_ip.is_some_and(|pref| a.ip != pref),
            ip_subnet_rank(a.ip),
            a.ip,
            a.interface.clone(),
        )
    });
    let mut seen = std::collections::HashSet::new();
    result.retain(|a| seen.insert(a.ip));
    result
}

fn is_virtual_interface(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "vethernet",
        "wsl",
        "hyper-v",
        "docker",
        "vbox",
        "virtualbox",
        "vmware",
        "vmnet",
        "virbr",
        "veth",
        "br-",
        "npcap",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
}

fn ip_subnet_rank(ip: IpAddr) -> u8 {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets[0] == 192 && octets[1] == 168 {
                0
            } else if octets[0] == 10 {
                1
            } else if octets[0] == 100 && (64..=127).contains(&octets[1]) {
                2
            } else if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                4
            } else if v4.is_link_local() {
                5
            } else {
                3
            }
        }
        IpAddr::V6(_) => 3,
    }
}

fn probe_default_route_ip(ipv6: bool) -> Option<IpAddr> {
    let (bind_addr, target_addr) = if ipv6 {
        ("[::]:0", "[2606:4700:4700::1111]:80")
    } else {
        ("0.0.0.0:0", "223.5.5.5:80")
    };
    let socket = std::net::UdpSocket::bind(bind_addr).ok()?;
    socket.connect(target_addr).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

pub fn connection_url(listen: SocketAddr, ip: IpAddr, token: &str) -> String {
    let fragment = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("token", token)
        .finish();
    format!("http://{}/#{fragment}", SocketAddr::new(ip, listen.port()))
}

pub fn qr_matrix(url: &str) -> Result<QrMatrix, qrcode::types::QrError> {
    let code = qrcode::QrCode::new(url)?;
    let quiet = 2;
    let size = code.width() + quiet * 2;
    let mut modules = vec![false; size * size];
    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                modules[(y + quiet) * size + (x + quiet)] = true;
            }
        }
    }
    Ok(QrMatrix { size, modules })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_addresses() -> Vec<NetworkAddress> {
        [
            "127.0.0.1",
            "0.0.0.0",
            "192.168.1.2",
            "192.168.1.2",
            "100.64.0.2",
            "169.254.2.1",
            "224.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fe80::1",
            "fd00::2",
        ]
        .map(|ip| NetworkAddress {
            ip: ip.parse().unwrap(),
            interface: "test".into(),
        })
        .into()
    }

    #[test]
    fn wildcard_filters_and_deduplicates_interfaces() {
        let ips: Vec<_> = candidates(Some("0.0.0.0:7683".parse().unwrap()), sample_addresses())
            .into_iter()
            .map(|a| a.ip.to_string())
            .collect();
        assert_eq!(ips, ["192.168.1.2", "100.64.0.2", "169.254.2.1"]);
        assert_eq!(
            candidates(Some("[::]:7683".parse().unwrap()), sample_addresses())[0]
                .ip
                .to_string(),
            "fd00::2"
        );
    }

    #[test]
    fn lan_ip_sorts_before_wsl_and_virtual_switch_addresses() {
        let addrs = vec![
            NetworkAddress {
                ip: "172.28.176.1".parse().unwrap(),
                interface: "vEthernet (WSL)".into(),
            },
            NetworkAddress {
                ip: "172.18.0.1".parse().unwrap(),
                interface: "Ethernet 2".into(),
            },
            NetworkAddress {
                ip: "192.168.4.24".parse().unwrap(),
                interface: "WLAN".into(),
            },
        ];
        let ips: Vec<_> = candidates(Some("0.0.0.0:7683".parse().unwrap()), addrs)
            .into_iter()
            .map(|a| a.ip.to_string())
            .collect();
        assert_eq!(ips, ["192.168.4.24", "172.18.0.1", "172.28.176.1"]);
    }

    #[test]
    fn specific_listener_does_not_advertise_other_interfaces() {
        assert_eq!(
            candidates(
                Some("192.168.1.2:7683".parse().unwrap()),
                sample_addresses()
            )
            .len(),
            1
        );
        assert!(
            candidates(
                Some("127.0.0.1:7683".parse().unwrap()),
                sample_addresses()
            )
            .is_empty()
        );
    }

    #[test]
    fn link_escapes_token_and_builds_qr_matrix() {
        let url = connection_url(
            "[::]:49152".parse().unwrap(),
            "fd00::2".parse().unwrap(),
            "a+b &=#密码",
        );
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.port(), Some(49152));
        assert_eq!(parsed.host_str(), Some("[fd00::2]"));
        let matrix = qr_matrix(&url).unwrap();
        assert!(matrix.size > 20);
        assert!(matrix.modules.iter().any(|&dark| dark));
    }
}
