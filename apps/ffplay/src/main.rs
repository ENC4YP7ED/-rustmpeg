fn main() {
    let code = rm_cli::run(rm_cli::Program::Ffplay, std::env::args_os());
    std::process::exit(code);
}
