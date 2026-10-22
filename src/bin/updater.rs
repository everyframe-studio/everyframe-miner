fn main() {
    if let Err(e) = everyframe_miner::updater_vm::run() {
        eprintln!("{}", everyframe_miner::diagnostics::safe_code(e.0));
        std::process::exit(1);
    }
}
