fn main() {
    if let Err(e) = everyframe_miner::worker::run_live() {
        eprintln!("{}", e.0);
        std::process::exit(1)
    }
}
