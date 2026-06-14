fn main() {
    if let Err(error) = toolbox::run(std::env::args_os()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
