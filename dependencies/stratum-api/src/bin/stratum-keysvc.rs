use std::path::Path;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = if args.len() == 4 && args[1] == "serve" {
        stratum_api::keysvc::serve(Path::new(&args[2]), Path::new(&args[3]))
    } else if args.len() == 3 && args[1] == "import-legacy" {
        stratum_api::keysvc::Store::open(Path::new(&args[2]))
            .and_then(|mut store| store.import_legacy())
    } else if args.len() == 4 && args[1] == "import-legacy" && args[3] == "-" {
        // The legacy key text arrives on standard input, never on the command line.
        stratum_api::keysvc::Store::open(Path::new(&args[2]))
            .and_then(|mut store| store.import_legacy_from(std::io::stdin().lock()))
    } else {
        Err(stratum_api::Error::Invalid(
            "usage: stratum-keysvc serve STORE_DIRECTORY SOCKET | import-legacy STORE_DIRECTORY [-]"
                .into(),
        ))
    };
    if result.is_err() {
        eprintln!("{{\"error\":\"key service failed\"}}");
        std::process::exit(1);
    }
}
