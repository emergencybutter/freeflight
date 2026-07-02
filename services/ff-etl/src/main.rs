mod pipeline;

fn main() {
    tracing_subscriber::fmt::init();
    if let Err(err) = pipeline::run() {
        tracing::error!("pipeline stopped: {err}");
        std::process::exit(1);
    }
}
