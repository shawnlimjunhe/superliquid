use chrono::Local;

pub trait Logger: Send + Sync {
    fn log(&self, level: &str, msg: &str);
}

pub struct ConsoleLogger {
    node_id: usize,
}

impl ConsoleLogger {
    pub fn new(node_id: usize) -> Self {
        Self { node_id }
    }
}

impl Logger for ConsoleLogger {
    fn log(&self, level: &str, msg: &str) {
        let now = Local::now().format("%H:%M:%S%.3f");

        let formatted = format!(
            "\x1b[90m[{}]\x1b[0m \x1b[34m[Node {}]\x1b[0m \x1b[93m[{}]\x1b[0m {}",
            now,
            self.node_id,
            level.to_uppercase(),
            msg
        );

        match level {
            "warn" | "error" => eprintln!("{}", formatted),
            _ => println!("{}", formatted),
        }
    }
}
