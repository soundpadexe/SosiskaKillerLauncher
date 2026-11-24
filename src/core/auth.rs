/// Offline UUID exactly as vanilla servers compute it in offline mode:
/// UUID v3 (MD5) of `OfflinePlayer:<nickname>`.
pub fn offline_uuid(nickname: &str) -> String {
    let mut d = md5::compute(format!("OfflinePlayer:{nickname}")).0;
    d[6] = (d[6] & 0x0f) | 0x30;
    d[8] = (d[8] & 0x3f) | 0x80;
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn is_valid_nickname(nick: &str) -> bool {
    (3..=16).contains(&nick.len()) && nick.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    #[test]
    fn notch_uuid() {
        assert_eq!(super::offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }
}
