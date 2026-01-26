use teamtalk::client::ffi::UserRight;

/// Build `TeamTalk` rights bitmask from string rights list.
pub fn get_user_rights_mask(rights_list: &[String]) -> u32 {
    let mut mask: u32 = 0;
    for r in rights_list {
        let flag = match r.to_uppercase().as_str() {
            "MULTI_LOGIN" => UserRight::USERRIGHT_MULTI_LOGIN,
            "VIEW_ALL_USERS" => UserRight::USERRIGHT_VIEW_ALL_USERS,
            "CREATE_TEMPORARY_CHANNEL" => UserRight::USERRIGHT_CREATE_TEMPORARY_CHANNEL,
            "MODIFY_CHANNELS" => UserRight::USERRIGHT_MODIFY_CHANNELS,
            "TEXTMESSAGE_BROADCAST" => UserRight::USERRIGHT_TEXTMESSAGE_BROADCAST,
            "KICK_USERS" => UserRight::USERRIGHT_KICK_USERS,
            "BAN_USERS" => UserRight::USERRIGHT_BAN_USERS,
            "MOVE_USERS" => UserRight::USERRIGHT_MOVE_USERS,
            "OPERATOR_ENABLE" => UserRight::USERRIGHT_OPERATOR_ENABLE,
            "UPLOAD_FILES" => UserRight::USERRIGHT_UPLOAD_FILES,
            "DOWNLOAD_FILES" => UserRight::USERRIGHT_DOWNLOAD_FILES,
            "UPDATE_SERVERPROPERTIES" => UserRight::USERRIGHT_UPDATE_SERVERPROPERTIES,
            "TRANSMIT_VOICE" => UserRight::USERRIGHT_TRANSMIT_VOICE,
            "TRANSMIT_VIDEOCAPTURE" => UserRight::USERRIGHT_TRANSMIT_VIDEOCAPTURE,
            "TRANSMIT_DESKTOP" => UserRight::USERRIGHT_TRANSMIT_DESKTOP,
            "TRANSMIT_DESKTOPINPUT" => UserRight::USERRIGHT_TRANSMIT_DESKTOPINPUT,
            "TRANSMIT_MEDIAFILE" => UserRight::USERRIGHT_TRANSMIT_MEDIAFILE,
            "LOCKED_NICKNAME" => UserRight::USERRIGHT_LOCKED_NICKNAME,
            "LOCKED_STATUS" => UserRight::USERRIGHT_LOCKED_STATUS,
            "RECORD_VOICE" => UserRight::USERRIGHT_RECORD_VOICE,
            "VIEW_HIDDEN_CHANNELS" => UserRight::USERRIGHT_VIEW_HIDDEN_CHANNELS,
            "TEXTMESSAGE_USER" => UserRight::USERRIGHT_TEXTMESSAGE_USER,
            "TEXTMESSAGE_CHANNEL" => UserRight::USERRIGHT_TEXTMESSAGE_CHANNEL,
            _ => UserRight::USERRIGHT_NONE,
        };
        mask |= flag as u32;
    }
    mask
}
