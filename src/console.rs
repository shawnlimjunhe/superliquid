use std::io::{self, Write};

fn handle_help() {
    println!("Commands:");
    println!("  create <id>");
    println!("  transfer <from> <to> <amount>");
    println!("  exit/quit");
}

fn handle_create(trimmed: &str) {
    let id = trimmed["create ".len()..].trim();
    println!("{}", id);
}

fn handle_transfer(trimmed: &str) {
    let parts: Vec<&str> = trimmed["transfer ".len()..].split_whitespace().collect();
    if parts.len() == 3 {
        println!("{:?}", parts);
    } else {
        println!("Usage: transfer <from> <to> <amount>");
    }
}

pub fn run_console() {
    const ANSI_ESC: &str = "\x1B[2J\x1B[1;1H";
    print!("{}", ANSI_ESC);
    loop {
        println!("HotStuff Client Console");
        println!("Type `help` to see commands.");
        print!("> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let trimmed = input.trim();

        match trimmed {
            "help" => handle_help(),
            _ if trimmed.starts_with("create ") => handle_create(trimmed),
            _ if trimmed.starts_with("transfer ") => handle_transfer(trimmed),
            "exit" | "quit" => break,
            _ => println!("Unknown command. Type `help` for options."),
        }
    }
}
