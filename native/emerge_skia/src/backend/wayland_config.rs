#[derive(Clone, Debug)]
pub struct WaylandConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WaylandConfig {
    fn default() -> Self {
        Self {
            title: "Emerge".to_string(),
            width: 800,
            height: 600,
        }
    }
}
