#[derive(Clone, Debug)]
pub struct Config {
    pub max_buf_size: usize,
}

impl Config {
    pub fn with_max_buf_size(mut self, max_buf_size: usize) -> Self {
        self.max_buf_size = max_buf_size;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_buf_size: 1024 * 1024 * 4, // 4 MiB
        }
    }
}
