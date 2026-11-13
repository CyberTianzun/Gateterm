use std::net::{IpAddr, SocketAddr};

fn configuration(args: impl Iterator<Item = String>) -> Result<(SocketAddr, Vec<String>), String> {
    let mut host: IpAddr = "127.0.0.1".parse().unwrap();
    let mut port = 17685;
    let mut tokens = vec![];
    let mut args = args;
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} requires a value"))?;
        match arg.as_str() {
            "--host" => {
                host = value
                    .parse()
                    .map_err(|_| "--host must be a loopback IP address")?
            }
            "--port" => port = value.parse().map_err(|_| "invalid port")?,
            "--token" => tokens.push(value),
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if !host.is_loopback() {
        return Err("local demo review permits loopback listeners only".into());
    }
    Ok((SocketAddr::new(host, port), tokens))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "gateterm-demo — Virtual demo — no host shell / no real files\nUsage: gateterm-demo [--host 127.0.0.1] [--port 17685] --token TOKEN [--token TOKEN...]\nAlternatively set GATETERM_DEMO_TOKENS to comma-separated tokens. No default token."
        );
        return Ok(());
    }
    let (address, mut tokens) = configuration(args.into_iter()).map_err(std::io::Error::other)?;
    if tokens.is_empty()
        && let Ok(value) = std::env::var("GATETERM_DEMO_TOKENS")
    {
        tokens = value.split(',').map(str::to_owned).collect();
    }
    let app = gateterm_demo::app(tokens).map_err(std::io::Error::other)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!(
        "Virtual demo — no host shell / no real files\nListening on http://{} (WebSocket /ws). Memory is erased on restart.",
        listener.local_addr()?
    );
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_defaults_local_and_refuses_public_bind() {
        assert_eq!(
            configuration(std::iter::empty()).unwrap().0,
            "127.0.0.1:17685".parse().unwrap()
        );
        assert!(configuration(["--host".into(), "0.0.0.0".into()].into_iter()).is_err());
        assert!(configuration(["--token".into()].into_iter()).is_err());
    }
}
