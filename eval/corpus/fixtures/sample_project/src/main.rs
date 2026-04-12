/// Application entry point
fn main() {
    let config = load_config();
    let server = Server::new(config);
    server.run();
}

/// Load configuration from environment
fn load_config() -> Config {
    Config {
        port: 8080,
        host: "localhost".to_string(),
    }
}

struct Config {
    port: u16,
    host: String,
}

struct Server {
    config: Config,
}

impl Server {
    fn new(config: Config) -> Self {
        Self { config }
    }

    fn run(&self) {
        println!("Running on {}:{}", self.config.host, self.config.port);
    }
}

enum Status {
    Active,
    Inactive,
    Pending,
}
