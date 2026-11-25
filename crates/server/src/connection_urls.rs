use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub(super) fn connection_urls(
    listen: SocketAddr,
    token: &str,
    web_title: Option<&str>,
) -> Vec<String> {
    let interfaces = if listen.ip() == IpAddr::V4(Ipv4Addr::UNSPECIFIED) {
        match if_addrs::get_if_addrs() {
            Ok(interfaces) => interfaces,
            Err(error) => {
                tracing::warn!(%error, "无法枚举本机网卡地址,仅显示回环访问链接");
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    urls_for_ips(
        listen,
        token,
        web_title,
        interfaces
            .into_iter()
            .filter(|iface| iface.is_oper_up())
            .map(|iface| iface.ip()),
    )
}

fn encode_form_component(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'*') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

fn format_fragment(token: &str, web_title: Option<&str>) -> String {
    let enc_tok = encode_form_component(token);
    match gateterm_server::config::normalize_web_title(web_title.map(str::to_string)) {
        Some(title) => format!("token={enc_tok}&title={}", encode_form_component(&title)),
        None => format!("token={enc_tok}"),
    }
}

fn urls_for_ips(
    listen: SocketAddr,
    token: &str,
    web_title: Option<&str>,
    ips: impl IntoIterator<Item = IpAddr>,
) -> Vec<String> {
    let fragment = format_fragment(token, web_title);
    if !listen.ip().is_unspecified() {
        return vec![format!("http://{listen}/#{fragment}")];
    }

    // 保留本机访问链接在首位;0.0.0.0 只监听 IPv4,不能把 IPv6 网卡地址当成可用链接。
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut addresses = vec![SocketAddr::new(localhost, listen.port())];
    if listen.is_ipv4() {
        let ips: BTreeSet<_> = ips
            .into_iter()
            .filter(|ip| {
                matches!(ip, IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_multicast() && !ip.is_broadcast())
            })
            .filter(|ip| *ip != localhost)
            .collect();
        addresses.extend(ips.into_iter().map(|ip| SocketAddr::new(ip, listen.port())));
    }
    addresses
        .into_iter()
        .map(|addr| format!("http://{addr}/#{fragment}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_lists_all_ipv4_addresses_with_localhost_first() {
        let ips = [
            "192.168.1.20",
            "100.64.0.2",
            "10.0.0.5",
            "192.168.1.20",
            "127.0.0.1",
            "169.254.1.2",
            "::1",
            "2001:db8::1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
        ]
        .map(|ip| ip.parse().unwrap());
        assert_eq!(
            urls_for_ips("0.0.0.0:9000".parse().unwrap(), "tok", None, ips),
            [
                "http://127.0.0.1:9000/#token=tok",
                "http://10.0.0.5:9000/#token=tok",
                "http://100.64.0.2:9000/#token=tok",
                "http://169.254.1.2:9000/#token=tok",
                "http://192.168.1.20:9000/#token=tok",
            ]
        );
    }

    #[test]
    fn missing_interfaces_still_provides_localhost() {
        assert_eq!(
            urls_for_ips("0.0.0.0:7683".parse().unwrap(), "tok", None, []),
            ["http://127.0.0.1:7683/#token=tok"]
        );
    }

    #[test]
    fn explicit_bind_only_lists_the_bound_address() {
        for host in [
            "127.0.0.1:7683",
            "192.168.1.20:9000",
            "[::1]:7683",
            "[2001:db8::1]:9000",
        ] {
            assert_eq!(
                urls_for_ips(host.parse().unwrap(), "tok", None, ["10.0.0.5".parse().unwrap()]),
                [format!("http://{host}/#token=tok")]
            );
        }
    }

    #[tokio::test]
    async fn ephemeral_port_uses_the_actual_listener_port() {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let listen = listener.local_addr().unwrap();
        assert_ne!(listen.port(), 0);
        assert_eq!(
            urls_for_ips(listen, "tok", None, []),
            [format!("http://127.0.0.1:{}/#token=tok", listen.port())]
        );
    }

    fn decode_form_component(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                b'%' => {
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                    out.push(u8::from_str_radix(hex, 16).unwrap());
                    i += 3;
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn encodes_token_and_optional_title_in_fragment() {
        let urls = urls_for_ips(
            "0.0.0.0:9000".parse().unwrap(),
            "a+b &=#密码",
            Some("  研发\n1号+a&b=#测试  "),
            ["192.168.1.20".parse().unwrap()],
        );
        assert_eq!(urls.len(), 2);
        for (url, expected_host) in urls.iter().zip(["127.0.0.1:9000", "192.168.1.20:9000"]) {
            let (base, fragment) = url.split_once('#').unwrap();
            assert_eq!(base, format!("http://{expected_host}/"));
            let pairs: Vec<(&str, String)> = fragment
                .split('&')
                .map(|kv| {
                    let (k, v) = kv.split_once('=').unwrap();
                    (k, decode_form_component(v))
                })
                .collect();
            assert_eq!(
                pairs,
                [
                    ("token", "a+b &=#密码".to_string()),
                    ("title", "研发 1号+a&b=#测试".to_string()),
                ]
            );
        }

        let blank_title_urls = urls_for_ips(
            "127.0.0.1:9000".parse().unwrap(),
            "a+b &=#密码",
            Some(" \r\n "),
            [],
        );
        assert_eq!(blank_title_urls.len(), 1);
        let (_, fragment) = blank_title_urls[0].split_once('#').unwrap();
        assert!(!fragment.contains("title="), "{fragment}");
        let (k, v) = fragment.split_once('=').unwrap();
        assert_eq!(k, "token");
        assert_eq!(decode_form_component(v), "a+b &=#密码");
    }
}
