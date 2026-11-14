//! Static server with HTTP range requests for the pack viewer (P15).
//!
//! ```sh
//! cargo run --release -p skimap-pack --features pipeline --example serve -- [root=.] [addr=127.0.0.1:8765]
//! ```

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned()))
        .init();
    let mut args = std::env::args().skip(1);
    let root = std::path::PathBuf::from(args.next().unwrap_or_else(|| ".".to_owned()));
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:8765".to_owned());
    println!(
        "serving {} on http://{addr}/ — viewer: http://{addr}/tools/viewer/index.html",
        root.display()
    );
    skimap_pack::serve::serve(&root, addr.as_str())?;
    Ok(())
}
