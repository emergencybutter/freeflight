fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = ff_etl::run() {
        tracing::error!("pipeline stopped: {err}");
        std::process::exit(1);
    }
}
