use clap::Parser;
use env_logger::Builder;
use std::io::Write;

mod memory;
mod server;

#[derive(Parser, Debug)]
#[command(name = "osu-tourney-data-reader")]
struct Args {
    /// port for the server
    #[arg(short, long, default_value = "25050")]
    port: u16,

    /// debug logging
    #[arg(short, long)]
    verbose: bool,
}

fn init_logger(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };

    Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format(|buf, record| {
            let level = record.level();
            let level_str = match level {
                log::Level::Error => "\x1b[31;1mERROR\x1b[0m", // Red bold
                log::Level::Warn => "\x1b[33mWARN \x1b[0m",    // Yellow
                log::Level::Info => "\x1b[32mINFO \x1b[0m",    // Green
                log::Level::Debug => "\x1b[36mDEBUG\x1b[0m",   // Cyan
                log::Level::Trace => "\x1b[35mTRACE\x1b[0m",   // Magenta
            };

            writeln!(
                buf,
                "{} {} {}",
                chrono::Local::now().format("%H:%M:%S"),
                level_str,
                record.args()
            )
        })
        .init();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    init_logger(args.verbose);

    server::run(args.port).await
}
