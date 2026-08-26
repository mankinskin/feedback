use std::{
    net::SocketAddr,
    path::PathBuf,
};

use feedback_http::{
    AppState,
    run,
};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("feedback_http=info".parse().unwrap()),
        )
        .with_writer(std::io::stderr)
        .init();

    let state = AppState {
        store_root: resolve_store_root(),
        workspace_slug: resolve_workspace_slug(),
    };
    let addr = resolve_addr();

    if let Err(err) = run(state, addr).await {
        eprintln!("feedback-http error: {err}");
        std::process::exit(1);
    }
}

fn resolve_store_root() -> PathBuf {
    if let Ok(path) = std::env::var("FEEDBACK_STORE_ROOT") {
        return PathBuf::from(path);
    }
    match memory_kernel::workspace::resolve_consumer_store_root(
        None,
        None,
        None,
        ".feedback",
    ) {
        Ok(store_root) => store_root,
        Err(error) => {
            eprintln!("Fatal error: {error}");
            std::process::exit(1);
        },
    }
}

fn resolve_workspace_slug() -> String {
    std::env::var("FEEDBACK_WORKSPACE_SLUG")
        .unwrap_or_else(|_| "default".to_string())
}

fn resolve_addr() -> SocketAddr {
    let raw = std::env::var("FEEDBACK_HTTP_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3222".to_string());
    raw.parse()
        .unwrap_or_else(|_| "127.0.0.1:3222".parse().unwrap())
}
