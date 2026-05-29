use std::env;

mod tasks;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let subcommand = &args[1];
    let sub_args = &args[2..];

    match subcommand.as_str() {
        "generate-skin-companions" => {
            tasks::generate_skin_companions::run(sub_args)?;
        }
        "hello" => {
            tasks::hello::run(sub_args)?;
        }
        "-h" | "--help" | "help" => {
            print_usage();
        }
        other => {
            eprintln!("Unknown task: {}", other);
            eprintln!();
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Usage: cargo run -p xtask -- <task> [args]");
    println!();
    println!("Available tasks:");
    println!("  generate-skin-companions    Generate skin companions suffixes and aliases");
    println!(
        "  hello                       A simple example task to demonstrate the multi-task setup"
    );
    println!();
    println!("Use 'cargo run -p xtask -- help' to display this help message.");
}
