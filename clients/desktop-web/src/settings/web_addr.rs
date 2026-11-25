//! Configurable web open address selection and live NIC discovery.
//!
//! Replaces tray QR pairing: discovers active network interfaces and sorts them
//! by reachability so Tray "Open web client", "Copy access link", and the
//! tray status line can prefer a LAN/VPN address when listening on a wildcard.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use crate::i18n::Locale;
use crate::tr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub ip: IpAddr,
    pub interface: String,
}

impl Address {
    fn label(&self) -> String {
        format!("{} · {}", self.ip, self.interface)
    }
}

pub struct WebAddrPicker {
    listen: Option<SocketAddr>,
    raw_interfaces: Vec<Address>,
    preferred_v4: Option<IpAddr>,
    preferred_v6: Option<IpAddr>,
    candidates: Vec<Address>,
    family_is_v6: Option<bool>,
    interface_error: bool,
}

impl WebAddrPicker {
    pub fn new(listen: Option<SocketAddr>) -> Self {
        let mut picker = Self {
            listen,
            raw_interfaces: Vec::new(),
            preferred_v4: None,
            preferred_v6: None,
            candidates: Vec::new(),
            family_is_v6: None,
            interface_error: false,
        };
        picker.refresh(listen);
        picker
    }

    /// Scans live network interfaces and default-route preferences outside
    /// egui's multi-pass draw closure.
    pub fn refresh(&mut self, listen: Option<SocketAddr>) {
        self.listen = listen;
        let interfaces = scan_interfaces();
        self.interface_error = interfaces.is_err();
        self.raw_interfaces = interfaces.unwrap_or_default();
        self.preferred_v4 = probe_default_route_ip(false);
        self.preferred_v6 = probe_default_route_ip(true);
        let is_v6 = listen.is_some_and(|l| l.is_ipv6());
        // Force recomputation even if the address family did not change,
        // because raw_interfaces or preferred route may have changed.
        self.family_is_v6 = None;
        self.sync_family(wildcard_ip(is_v6));
    }

    /// Purely recomputes candidate list when the IPv4/IPv6 family changes,
    /// without performing any IO so it is safe to call across draw passes.
    pub fn sync_family(&mut self, listen_ip: IpAddr) {
        let is_v6 = listen_ip.is_ipv6();
        if self.family_is_v6 == Some(is_v6) {
            return;
        }
        self.family_is_v6 = Some(is_v6);
        let preferred = if is_v6 { self.preferred_v6 } else { self.preferred_v4 };
        let wildcard = SocketAddr::new(wildcard_ip(is_v6), 0);
        self.candidates = candidates_with_preferred(
            Some(wildcard),
            self.raw_interfaces.clone(),
            preferred,
        );
    }

    pub fn resolved(&self, listen: SocketAddr, pref: Option<IpAddr>) -> IpAddr {
        if !listen.ip().is_unspecified() {
            return listen.ip();
        }
        if self.family_is_v6 == Some(listen.is_ipv6()) {
            resolve_web_host(listen, pref, &self.candidates)
        } else {
            let preferred = if listen.is_ipv6() { self.preferred_v6 } else { self.preferred_v4 };
            let wildcard = SocketAddr::new(wildcard_ip(listen.is_ipv6()), listen.port());
            let cands = candidates_with_preferred(Some(wildcard), self.raw_interfaces.clone(), preferred);
            resolve_web_host(listen, pref, &cands)
        }
    }

    pub fn draw_selector(
        &mut self,
        ui: &mut egui::Ui,
        listen_ip: Option<IpAddr>,
        pref: &mut Option<IpAddr>,
        locale: Locale,
    ) -> (bool, bool) {
        let mut edited = false;
        let mut refresh = false;
        let effective_ip = listen_ip
            .or_else(|| self.listen.map(|l| l.ip()))
            .unwrap_or_else(|| IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        ui.vertical(|ui| {
            if !effective_ip.is_unspecified() {
                ui.monospace(effective_ip.to_string());
                ui.small(tr!(
                    locale,
                    "当前仅监听 {effective_ip}，如需局域网地址请将监听地址设为 0.0.0.0",
                    "Listening on {effective_ip} only; set listen address to 0.0.0.0 for LAN addresses"
                ));
                return;
            }

            self.sync_family(effective_ip);
            let loopback = loopback_for(effective_ip);
            let wildcard_listen = SocketAddr::new(effective_ip, 0);
            let auto_resolved = resolve_web_host(wildcard_listen, None, &self.candidates);
            let auto_label = tr!(
                locale,
                "自动（优先局域网，当前：{auto_resolved}）",
                "Auto (prefer LAN, now: {auto_resolved})"
            );
            let loopback_label = tr!(locale, "{loopback} · 本机回环", "{loopback} · Localhost");
            let selected_label = match *pref {
                None => auto_label.clone(),
                Some(ip) if ip == loopback => loopback_label.clone(),
                Some(ip) => {
                    if let Some(cand) = self.candidates.iter().find(|a| a.ip == ip) {
                        cand.label()
                    } else {
                        tr!(locale, "{ip} (离线，将自动回退)", "{ip} (offline, auto-fallback)")
                    }
                }
            };

            ui.horizontal(|ui| {
                let combo_width = (ui.available_width() - 80.0).clamp(180.0, 290.0);
                egui::ComboBox::from_id_salt("web-open-address")
                    .selected_text(selected_label)
                    .width(combo_width)
                    .show_ui(ui, |ui| {
                        edited |= ui.selectable_value(pref, None, auto_label.clone()).changed();
                        for addr in &self.candidates {
                            edited |= ui.selectable_value(pref, Some(addr.ip), addr.label()).changed();
                        }
                        edited |= ui.selectable_value(pref, Some(loopback), loopback_label.clone()).changed();
                        if let Some(ip) = *pref
                            && ip != loopback
                            && !self.candidates.iter().any(|a| a.ip == ip)
                        {
                            let offline_label = tr!(
                                locale,
                                "{ip} (离线，将自动回退)",
                                "{ip} (offline, auto-fallback)"
                            );
                            edited |= ui.selectable_value(pref, Some(ip), offline_label).changed();
                        }
                    });
                if ui.small_button(locale.text("刷新地址", "Refresh")).clicked() {
                    refresh = true;
                }
            });

            if self.interface_error {
                ui.small(locale.text(
                    "无法读取本机网卡，请重试。",
                    "Cannot read network interfaces. Please retry.",
                ));
            }
            ui.small(locale.text(
                "用于托盘「打开 Web 客户端」和「复制访问链接」",
                "Sets the URL for tray \"Open web client\" and \"Copy access link\"",
            ));
        });

        (edited, refresh)
    }
}

pub fn loopback_for(ip: IpAddr) -> IpAddr {
    if ip.is_ipv6() {
        IpAddr::V6(Ipv6Addr::LOCALHOST)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }
}

fn wildcard_ip(ipv6: bool) -> IpAddr {
    if ipv6 {
        IpAddr::V6(Ipv6Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    }
}

fn scan_interfaces() -> std::io::Result<Vec<Address>> {
    if_addrs::get_if_addrs().map(|interfaces| {
        interfaces
            .into_iter()
            .filter(|iface| iface.is_oper_up())
            .map(|iface| Address { ip: iface.ip(), interface: iface.name })
            .collect()
    })
}

#[cfg(test)]
fn candidates(listen: Option<SocketAddr>, addresses: Vec<Address>) -> Vec<Address> {
    candidates_with_preferred(listen, addresses, None)
}

fn candidates_with_preferred(
    listen: Option<SocketAddr>,
    addresses: Vec<Address>,
    preferred_ip: Option<IpAddr>,
) -> Vec<Address> {
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
                // Scoped link-local IPv6 requires the client's zone, not the host's.
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
    let mut seen = HashSet::new();
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

pub fn resolve_web_host(listen: SocketAddr, pref: Option<IpAddr>, candidates: &[Address]) -> IpAddr {
    if !listen.ip().is_unspecified() {
        return listen.ip();
    }
    let loopback = loopback_for(listen.ip());
    let same_family = |ip: &IpAddr| ip.is_ipv4() == listen.is_ipv4();
    if let Some(saved) = pref
        && same_family(&saved)
        && (saved == loopback || candidates.iter().any(|a| a.ip == saved))
    {
        return saved;
    }
    candidates
        .iter()
        .find(|a| same_family(&a.ip))
        .map(|a| a.ip)
        .unwrap_or(loopback)
}

pub fn resolve_live_web_host(listen: SocketAddr, pref: Option<IpAddr>) -> IpAddr {
    if !listen.ip().is_unspecified() {
        return listen.ip();
    }
    let loopback = loopback_for(listen.ip());
    if pref == Some(loopback) {
        return loopback;
    }
    let interfaces = scan_interfaces().unwrap_or_default();
    let preferred = probe_default_route_ip(listen.is_ipv6());
    let candidates = candidates_with_preferred(Some(listen), interfaces, preferred);
    resolve_web_host(listen, pref, &candidates)
}

pub fn format_access_url(host: IpAddr, port: u16, token: &str, title: Option<&str>) -> String {
    let mut ser = url::form_urlencoded::Serializer::new(String::new());
    ser.append_pair("token", token);
    if let Some(t) = gateterm_server::config::normalize_web_title(title.map(str::to_string)) {
        ser.append_pair("title", &t);
    }
    let fragment = ser.finish();
    format!("http://{}/#{fragment}", SocketAddr::new(host, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => {
                assert!(
                    text.pos.x + text.galley.size().x <= 560.5,
                    "selector text overflows 560px: {}",
                    text.galley.text()
                );
                out.push(text.galley.text().to_owned());
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_texts(s, out)),
            _ => {}
        }
    }

    fn addresses() -> Vec<Address> {
        [
            "127.0.0.1",
            "127.4.3.2",
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
            "ff02::1",
            "fd00::2",
        ]
        .map(|ip| Address { ip: ip.parse().unwrap(), interface: "test".into() })
        .into()
    }

    #[test]
    fn wildcard_filters_and_deduplicates_interfaces() {
        let ips: Vec<_> = candidates(Some("0.0.0.0:7683".parse().unwrap()), addresses())
            .into_iter()
            .map(|a| a.ip.to_string())
            .collect();
        assert_eq!(ips, ["192.168.1.2", "100.64.0.2", "169.254.2.1"]);
        assert_eq!(
            candidates(Some("[::]:7683".parse().unwrap()), addresses())[0]
                .ip
                .to_string(),
            "fd00::2"
        );
    }

    #[test]
    fn lan_ip_sorts_before_wsl_and_virtual_switch_addresses() {
        let addrs = vec![
            Address { ip: "172.28.176.1".parse().unwrap(), interface: "vEthernet (WSL)".into() },
            Address { ip: "172.18.0.1".parse().unwrap(), interface: "Ethernet 2".into() },
            Address { ip: "192.168.4.24".parse().unwrap(), interface: "WLAN".into() },
        ];
        let ips: Vec<_> = candidates(Some("0.0.0.0:7683".parse().unwrap()), addrs)
            .into_iter()
            .map(|a| a.ip.to_string())
            .collect();
        assert_eq!(ips, ["192.168.4.24", "172.18.0.1", "172.28.176.1"]);
    }

    #[test]
    fn specific_listener_does_not_advertise_other_interfaces() {
        assert_eq!(candidates(Some("192.168.1.2:7683".parse().unwrap()), addresses()).len(), 1);
        assert!(candidates(Some("127.0.0.1:7683".parse().unwrap()), addresses()).is_empty());
        assert!(candidates(None, addresses()).is_empty());
    }

    #[test]
    fn resolve_web_host_prefers_saved_active_ip_and_falls_back_when_offline() {
        let wildcard_v4: SocketAddr = "0.0.0.0:7683".parse().unwrap();
        let wildcard_v6: SocketAddr = "[::]:7683".parse().unwrap();
        let lan1: IpAddr = "192.168.1.2".parse().unwrap();
        let lan2: IpAddr = "100.64.0.2".parse().unwrap();
        let offline: IpAddr = "192.168.99.99".parse().unwrap();
        let loopback_v4: IpAddr = "127.0.0.1".parse().unwrap();
        let loopback_v6: IpAddr = "::1".parse().unwrap();
        let cands = vec![
            Address { ip: lan1, interface: "WLAN".into() },
            Address { ip: lan2, interface: "Tailscale".into() },
        ];

        assert_eq!(resolve_web_host(wildcard_v4, None, &cands), lan1);
        assert_eq!(resolve_web_host(wildcard_v4, Some(lan2), &cands), lan2);
        assert_eq!(resolve_web_host(wildcard_v4, Some(offline), &cands), lan1);
        assert_eq!(resolve_web_host(wildcard_v4, Some(loopback_v4), &cands), loopback_v4);
        assert_eq!(resolve_web_host(wildcard_v4, None, &[]), loopback_v4);
        assert_eq!(resolve_web_host(wildcard_v6, None, &[]), loopback_v6);
        assert_eq!(
            resolve_web_host(
                wildcard_v6,
                Some(loopback_v6),
                &[Address { ip: "fd00::2".parse().unwrap(), interface: "eth0".into() }]
            ),
            loopback_v6
        );

        let specific: SocketAddr = "192.168.1.10:7683".parse().unwrap();
        assert_eq!(resolve_web_host(specific, Some(lan2), &cands), specific.ip());
        assert_eq!(resolve_web_host(specific, None, &cands), specific.ip());
    }

    #[test]
    fn resolve_web_host_handles_ipv4_ipv6_family_mismatch() {
        let wildcard_v6: SocketAddr = "[::]:7683".parse().unwrap();
        let wildcard_v4: SocketAddr = "0.0.0.0:7683".parse().unwrap();
        let v4_ip: IpAddr = "192.168.1.2".parse().unwrap();
        let v6_ip: IpAddr = "fd00::2".parse().unwrap();
        let v6_cands = vec![Address { ip: v6_ip, interface: "eth0".into() }];
        let v4_cands = vec![Address { ip: v4_ip, interface: "eth0".into() }];

        assert_eq!(resolve_web_host(wildcard_v6, Some(v4_ip), &v6_cands), v6_ip);
        assert_eq!(resolve_web_host(wildcard_v6, Some(v4_ip), &[]), "::1".parse::<IpAddr>().unwrap());
        assert_eq!(resolve_web_host(wildcard_v4, Some(v6_ip), &v4_cands), v4_ip);
        assert_eq!(resolve_web_host(wildcard_v4, Some(v6_ip), &[]), "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn format_access_url_escapes_token_and_brackets_ipv6() {
        let url = format_access_url("fd00::2".parse().unwrap(), 49152, "a+b &=#密码", None);
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.port(), Some(49152));
        assert_eq!(parsed.host_str(), Some("[fd00::2]"));
        assert_eq!(
            url::form_urlencoded::parse(parsed.fragment().unwrap().as_bytes())
                .collect::<Vec<_>>(),
            [("token".into(), "a+b &=#密码".into())]
        );

        let v4 = format_access_url("192.168.1.5".parse().unwrap(), 7683, "tok", None);
        assert_eq!(v4, "http://192.168.1.5:7683/#token=tok");

        let with_title = format_access_url(
            "192.168.1.5".parse().unwrap(),
            7683,
            "a+b &=#密码",
            Some("  研发\n1号+a&b=#测试  "),
        );
        let parsed_with_title = url::Url::parse(&with_title).unwrap();
        assert_eq!(
            url::form_urlencoded::parse(parsed_with_title.fragment().unwrap().as_bytes())
                .collect::<Vec<_>>(),
            [
                ("token".into(), "a+b &=#密码".into()),
                ("title".into(), "研发 1号+a&b=#测试".into()),
            ]
        );

        let blank_title = format_access_url(
            "192.168.1.5".parse().unwrap(),
            7683,
            "tok",
            Some("  \n "),
        );
        assert_eq!(blank_title, "http://192.168.1.5:7683/#token=tok");
    }

    #[test]
    fn draw_selector_fits_and_syncs_families_without_io() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx);
        let mut picker = WebAddrPicker {
            listen: Some("127.0.0.1:7683".parse().unwrap()),
            raw_interfaces: vec![
                Address { ip: "192.168.100.100".parse().unwrap(), interface: "WLAN".into() },
                Address { ip: "fd00::2".parse().unwrap(), interface: "Ethernet".into() },
            ],
            preferred_v4: Some("192.168.100.100".parse().unwrap()),
            preferred_v6: Some("fd00::2".parse().unwrap()),
            candidates: Vec::new(),
            family_is_v6: None,
            interface_error: false,
        };
        picker.sync_family("127.0.0.1".parse().unwrap());

        for (listen_ip, mut pref) in [
            ("127.0.0.1".parse().unwrap(), None),
            ("0.0.0.0".parse().unwrap(), None),
            ("0.0.0.0".parse().unwrap(), Some("192.168.99.99".parse().unwrap())),
            ("::".parse().unwrap(), None),
        ] {
            for locale in [Locale::En, Locale::ZhCn] {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(560.0, 680.0))),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    picker.draw_selector(ui, Some(listen_ip), &mut pref, locale);
                });
                output.textures_delta.clear();
                let mut texts = vec![];
                for shape in &output.shapes {
                    collect_texts(&shape.shape, &mut texts);
                }
                if locale == Locale::En {
                    assert!(
                        !texts.iter().any(|t| t.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))),
                        "{texts:?}"
                    );
                }
            }
        }
        assert_eq!(
            picker.resolved("[::]:7683".parse().unwrap(), None),
            "fd00::2".parse::<IpAddr>().unwrap()
        );
        assert_eq!(
            picker.resolved("0.0.0.0:7683".parse().unwrap(), None),
            "192.168.100.100".parse::<IpAddr>().unwrap()
        );
    }
}
