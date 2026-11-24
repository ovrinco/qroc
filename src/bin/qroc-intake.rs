fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match qroc_standalone::intake::cli(&args) {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("{}", serde_json::json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
