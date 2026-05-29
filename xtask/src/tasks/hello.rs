pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    println!("Hello from the hello xtask!");
    if !args.is_empty() {
        println!("Arguments provided: {:?}", args);
    }
    Ok(())
}
