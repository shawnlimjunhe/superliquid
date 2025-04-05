#[macro_export]
macro_rules! replica_log {
    ($node_id:expr, $($arg:tt)*) => {{
        use chrono::Local;
        let now = Local::now().format("%H:%M:%S%.3f");
        let prefix = format!(
            "\x1b[90m[{}]\x1b[0m \x1b[94m[Node {}]\x1b[0m",
            now, $node_id
        );
        println!("{} {}", prefix, format!($($arg)*));
    }};
}
