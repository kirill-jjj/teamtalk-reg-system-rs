use crate::config::AppConfig;

pub fn generate_tt_link(
    config: &AppConfig,
    username: &str,
    password: &str,
    nickname: &str,
) -> String {
    let host = config
        .tt_public_hostname
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&config.host_name);
    let encrypted = if config.encrypted { "1" } else { "0" };
    let nick = if nickname.trim().is_empty() {
        username
    } else {
        nickname
    };

    format!(
        "tt://{host}?tcpport={tcp}&udpport={udp}&encrypted={enc}&username={user}&password={pass}&nickname={nick}&channel=/&chanpasswd=",
        host = host,
        tcp = config.tcp_port,
        udp = config.udp_port.unwrap_or(config.tcp_port),
        enc = encrypted,
        user = url_encode(username),
        pass = url_encode(password),
        nick = url_encode(nick),
    )
}

fn url_encode(input: &str) -> String {
    let mut out = String::new();
    for b in input.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
