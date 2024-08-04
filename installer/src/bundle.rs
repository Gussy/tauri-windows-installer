pub trait Bundle {
    fn load() -> Self;
    fn load_data() -> Vec<u8>;
    fn is_installed() -> bool;
    fn install(&self, quiet: bool) -> Result<(), anyhow::Error>;
}
