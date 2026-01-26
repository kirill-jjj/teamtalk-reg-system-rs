use crate::config::AppConfig;

/// Generate `TeamTalk` `.tt` file XML content.
pub fn generate_tt_file_content(
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

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE teamtalk>
<teamtalk version="5.0">
    <host>
        <name>{server_name}</name>
        <address>{host}</address>
        <tcpport>{tcp}</tcpport>
        <udpport>{udp}</udpport>
        <encrypted>{enc}</encrypted>
        <trusted-certificate>
            <certificate-authority-pem></certificate-authority-pem>
            <client-certificate-pem></client-certificate-pem>
            <client-private-key-pem></client-private-key-pem>
            <verify-peer>false</verify-peer>
        </trusted-certificate>
        <auth>
            <username>{user}</username>
            <password>{pass}</password>
            <nickname>{nick}</nickname>
        </auth>
        <join>
            <channel>{join_chan}</channel>
            <password>{join_pass}</password>
        </join>
    </host>
</teamtalk>"#,
        server_name = escape_xml(&config.teamtalk.server_name),
        host = escape_xml(host),
        tcp = config.teamtalk.tcp_port,
        udp = config.teamtalk.udp_port.unwrap_or(config.teamtalk.tcp_port),
        enc = if config.teamtalk.encrypted {
            "true"
        } else {
            "false"
        },
        user = escape_xml(username),
        pass = escape_xml(password),
        nick = escape_xml(nickname),
        join_chan = escape_xml(config.teamtalk.tt_join_channel.as_deref().unwrap_or("")),
        join_pass = escape_xml(
            config
                .teamtalk
                .tt_join_channel_password
                .as_deref()
                .unwrap_or("")
        ),
    )
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
