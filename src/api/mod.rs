pub mod art;
pub mod curseforge;
pub mod modrinth;
pub mod mojang;
pub mod skins;
pub mod store;

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("SosiskaKillerLauncher/", env!("CARGO_PKG_VERSION")))
        // No total deadline: large jars/assets may legitimately take minutes.
        // Fail only on a dead connection or a stalled transfer.
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("TLS backend init")
}
