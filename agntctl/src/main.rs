use std::env;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let code = run(args.as_slice());
    std::process::exit(code);
}

fn run(args: &[String]) -> i32 {
    if args.is_empty() || is_help(args[0].as_str()) {
        print_help();
        return 0;
    }
    if is_version(args[0].as_str()) {
        println!("agntctl {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }

    match args[0].as_str() {
        "status" => {
            println!("secureagntd status: unknown (daemon/API wiring pending)");
            0
        }
        "config" => run_config(&args[1..]),
        "skills" => run_skills(&args[1..]),
        "policy" => run_policy(&args[1..]),
        "audit" => run_audit(&args[1..]),
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            2
        }
    }
}

fn run_config(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("validate")) {
        println!("config validation: ok (schema checks pending)");
        return 0;
    }
    eprintln!("usage: agntctl config validate");
    2
}

fn run_skills(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("list") => {
            println!("skills list: not yet connected");
            0
        }
        Some("info") => {
            if let Some(id) = args.get(1) {
                println!("skills info {id}: not yet connected");
                0
            } else {
                eprintln!("usage: agntctl skills info <id>");
                2
            }
        }
        Some("install") => {
            if let Some(source) = args.get(1) {
                println!("skills install {source}: not yet connected");
                0
            } else {
                eprintln!("usage: agntctl skills install <source>");
                2
            }
        }
        _ => {
            eprintln!("usage: agntctl skills <list|info|install> ...");
            2
        }
    }
}

fn run_policy(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("allow") => {
            println!("policy allow: not yet connected");
            0
        }
        Some("deny") => {
            println!("policy deny: not yet connected");
            0
        }
        _ => {
            eprintln!("usage: agntctl policy <allow|deny> ...");
            2
        }
    }
}

fn run_audit(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("tail")) {
        println!("audit tail: not yet connected");
        return 0;
    }
    eprintln!("usage: agntctl audit tail");
    2
}

fn is_help(value: &str) -> bool {
    matches!(value, "-h" | "--help" | "help")
}

fn is_version(value: &str) -> bool {
    matches!(value, "-V" | "--version" | "version")
}

fn print_help() {
    println!(
        "agntctl - SecureAgnt control CLI\n\n\
Usage:\n\
  agntctl status\n\
  agntctl config validate\n\
  agntctl skills list\n\
  agntctl skills info <id>\n\
  agntctl skills install <source>\n\
  agntctl policy allow ...\n\
  agntctl policy deny ...\n\
  agntctl audit tail\n\
  agntctl --help\n\
  agntctl --version"
    );
}

#[cfg(test)]
mod tests {
    use super::run;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn status_command_succeeds() {
        assert_eq!(run(args(&["status"]).as_slice()), 0);
    }

    #[test]
    fn config_validate_succeeds() {
        assert_eq!(run(args(&["config", "validate"]).as_slice()), 0);
    }

    #[test]
    fn unknown_command_fails() {
        assert_eq!(run(args(&["unknown"]).as_slice()), 2);
    }
}
