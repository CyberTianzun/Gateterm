//! QR links describe the live listener, never uncommitted settings. Changing the
//! advertised interface does not rebind HTTP or detach existing terminals.
use std::net::{IpAddr, SocketAddr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub ip: IpAddr,
    pub interface: String,
}

impl Address {
    fn label(&self) -> String { format!("{} · {}", self.ip, self.interface) }
}

pub struct Pairing {
    listen: Option<SocketAddr>,
    token: String,
    addresses: Vec<Address>,
    selected: Option<IpAddr>,
    error: Option<String>,
    texture: Option<(String, egui::TextureHandle)>,
}

impl Pairing {
    pub fn new(listen: Option<SocketAddr>, token: Option<&str>) -> Self {
        let mut pairing = Self {
            listen, token: token.unwrap_or_default().into(), addresses: vec![],
            selected: None, error: None, texture: None,
        };
        pairing.refresh(listen, token);
        pairing
    }

    /// Called on open, explicit refresh and every settings apply, outside egui's
    /// multi-pass draw closure. No network requests or port probes are needed.
    pub fn refresh(&mut self, listen: Option<SocketAddr>, token: Option<&str>) {
        self.listen = listen;
        self.token = token.unwrap_or_default().into();
        let interfaces = if_addrs::get_if_addrs().map(|interfaces| interfaces.into_iter()
            .filter(|iface| iface.is_oper_up())
            .map(|iface| Address { ip: iface.ip(), interface: iface.name }).collect::<Vec<_>>());
        self.error = interfaces.as_ref().err().map(|_| "无法读取本机网卡，请重试。".into());
        self.addresses = candidates(listen, interfaces.unwrap_or_default());
        self.selected = self.selected.filter(|ip| self.addresses.iter().any(|a| a.ip == *ip))
            .or_else(|| self.addresses.first().map(|a| a.ip));
        self.texture = None;
    }

    pub fn draw(&mut self, ui: &mut egui::Ui) -> bool {
        let mut refresh = false;
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("Android 扫码连接");
                if ui.small_button("刷新地址").clicked() { refresh = true; }
            });
            let Some(listen) = self.listen.filter(|_| !self.token.is_empty()) else {
                ui.small("服务尚未启动，保存并应用后可生成连接二维码。");
                return;
            };
            if let Some(error) = &self.error { ui.small(error); }
            if self.addresses.is_empty() {
                ui.small(if listen.ip().is_loopback() {
                    "当前仅监听本机。将监听地址改为 0.0.0.0 并保存，手机才能连接。"
                } else { "未找到可供手机连接的网卡地址。连接网络后点击刷新地址。" });
                return;
            }
            ui.horizontal(|ui| {
                ui.label("连接 IP");
                let selected = self.addresses.iter().find(|a| Some(a.ip) == self.selected)
                    .map(Address::label).unwrap_or_default();
                egui::ComboBox::from_id_salt("pairing-address")
                    .selected_text(selected).width(380.0)
                    .show_ui(ui, |ui| {
                        for address in &self.addresses {
                            ui.selectable_value(&mut self.selected, Some(address.ip), address.label());
                        }
                    });
            });
            let ip = self.selected.unwrap();
            let url = connection_url(listen, ip, &self.token);
            ui.add_space(8.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    // This always uses the applied token and actual bound port,
                    // including when the configured port was zero.
                    if self.texture.as_ref().is_none_or(|(cached, _)| *cached != url) {
                        self.texture = qr_image(&url).ok().map(|image| (url.clone(), ui.ctx().load_texture(
                            "server-pairing-qr", image, egui::TextureOptions::NEAREST,
                        )));
                    }
                    if let Some((_, texture)) = &self.texture {
                        ui.image((texture.id(), egui::vec2(192.0, 192.0)));
                    } else { ui.small("连接信息过长，无法生成二维码。请手动添加服务器。"); }
                });
                ui.vertical(|ui| {
                    ui.monospace(format!("http://{}", SocketAddr::new(ip, listen.port())));
                    if ui.button("复制连接链接").clicked() { ui.ctx().copy_text(url); }
                    ui.add_space(8.0);
                    ui.small("Android 添加服务器 → 扫码，自动填入地址和 token。");
                    ui.small("请选择手机能访问的局域网或 VPN 地址。");
                    ui.small("修改监听地址、端口或 token 后，请先保存并应用。");
                });
            });
        });
        refresh
    }
}

fn candidates(listen: Option<SocketAddr>, addresses: Vec<Address>) -> Vec<Address> {
    let Some(listen) = listen else { return vec![] };
    let mut result: Vec<_> = addresses.into_iter().filter(|a| {
        let reachable = match a.ip {
            IpAddr::V4(ip) => !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() && !ip.is_broadcast(),
            // Scoped link-local IPv6 requires the phone's zone, not the PC's.
            IpAddr::V6(ip) => !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() && !ip.is_unicast_link_local(),
        };
        reachable && if listen.ip().is_unspecified() { a.ip.is_ipv4() == listen.is_ipv4() }
            else { a.ip == listen.ip() }
    }).collect();
    result.sort_by_key(|a| (matches!(a.ip, IpAddr::V4(ip) if ip.is_link_local()), a.ip, a.interface.clone()));
    result.dedup_by_key(|a| a.ip);
    result
}

fn connection_url(listen: SocketAddr, ip: IpAddr, token: &str) -> String {
    let fragment = url::form_urlencoded::Serializer::new(String::new()).append_pair("token", token).finish();
    format!("http://{}/#{fragment}", SocketAddr::new(ip, listen.port()))
}

fn qr_image(url: &str) -> Result<egui::ColorImage, qrcode::types::QrError> {
    let code = qrcode::QrCode::new(url)?;
    let size = code.width() + 8; // Four white modules on every edge, even in dark themes.
    let mut image = egui::ColorImage::filled([size, size], egui::Color32::WHITE);
    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark { image[(x + 4, y + 4)] = egui::Color32::BLACK; }
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn addresses() -> Vec<Address> {
        ["127.0.0.1", "127.4.3.2", "0.0.0.0", "192.168.1.2", "192.168.1.2", "100.64.0.2",
            "169.254.2.1", "224.0.0.1", "255.255.255.255", "::", "::1", "fe80::1", "ff02::1", "fd00::2"]
            .map(|ip| Address { ip: ip.parse().unwrap(), interface: "test".into() }).into()
    }
    #[test]
    fn wildcard_filters_and_deduplicates_interfaces() {
        let ips: Vec<_> = candidates(Some("0.0.0.0:7683".parse().unwrap()), addresses()).into_iter().map(|a| a.ip.to_string()).collect();
        assert_eq!(ips, ["100.64.0.2", "192.168.1.2", "169.254.2.1"]);
        assert_eq!(candidates(Some("[::]:7683".parse().unwrap()), addresses())[0].ip.to_string(), "fd00::2");
    }
    #[test]
    fn specific_listener_does_not_advertise_other_interfaces() {
        assert_eq!(candidates(Some("192.168.1.2:7683".parse().unwrap()), addresses()).len(), 1);
        assert!(candidates(Some("127.0.0.1:7683".parse().unwrap()), addresses()).is_empty());
        assert!(candidates(None, addresses()).is_empty());
    }
    #[test]
    fn link_escapes_token_and_uses_the_live_port_with_ipv6_brackets() {
        let url = connection_url("[::]:49152".parse().unwrap(), "fd00::2".parse().unwrap(), "a+b &=#密码");
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.port(), Some(49152));
        assert_eq!(parsed.host_str(), Some("[fd00::2]"));
        assert_eq!(url::form_urlencoded::parse(parsed.fragment().unwrap().as_bytes()).next().unwrap().1, "a+b &=#密码");
        let image = qr_image(&url).unwrap();
        for i in 0..image.size[0] {
            assert_eq!(image[(i, 0)], egui::Color32::WHITE);
            assert_eq!(image[(0, i)], egui::Color32::WHITE);
        }
        assert!(image.pixels.contains(&egui::Color32::BLACK));
    }
}
