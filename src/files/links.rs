use crate::config::AppConfig;
use std::fmt::Write as _;

/// Generate a `TeamTalk` link for quick join.
pub fn generate_tt_link(
    config: &AppConfig,
    username: &str,
    password: &str,
    nickname: &str,
) -> String {
    let host = config
        .teamtalk
        .tt_public_hostname
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&config.teamtalk.host_name);
    let encrypted = if config.teamtalk.encrypted { "1" } else { "0" };
    let nick = if nickname.trim().is_empty() {
        username
    } else {
        nickname
    };

    format!(
        "tt://{host}?tcpport={tcp}&udpport={udp}&encrypted={enc}&username={user}&password={pass}&nickname={nick}&channel=/&chanpasswd=",
        host = host,
        tcp = config.teamtalk.tcp_port,
        udp = config.teamtalk.udp_port.unwrap_or(config.teamtalk.tcp_port),
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
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            _ => {
                let _ = write!(&mut out, "%{b:02X}");
            }
        }
    }
    out
}
