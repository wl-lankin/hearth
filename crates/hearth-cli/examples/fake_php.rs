//! Test stand-in for PHP: prints the program path, PHPRC and every argument on its own line, and
//! exits with the code given as `--exit=<n>`.

fn main() {
    let exe = std::env::current_exe().unwrap();
    println!("exe:{}", exe.display());
    println!("phprc:{}", std::env::var("PHPRC").unwrap_or_default());
    println!("pid:{}", std::process::id());
    let mut code = 0;
    for arg in std::env::args().skip(1) {
        if let Some(n) = arg.strip_prefix("--exit=") {
            code = n.parse().unwrap_or(1);
        }
        println!("arg:{arg}");
    }
    std::process::exit(code);
}
