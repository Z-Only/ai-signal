use ai_news_server::{router, scheduler, AppState, Database, HttpFetcher, SystemClock};
use std::{env, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = env::var("DATABASE_PATH").unwrap_or_else(|_| "news.sqlite3".into());
    let address = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let token = env::var("ADMIN_TOKEN").ok();
    let static_dir = env::var_os("STATIC_DIR").map(Into::into);
    let state = AppState::new(
        Database::open(database)?,
        Arc::new(HttpFetcher::new()?),
        Arc::new(SystemClock),
        token.as_deref(),
    );
    let enabled = env::var("ENABLE_SCHEDULER").map_or(true, |value| value != "false");
    // Bind before starting any background work; fail fast on an occupied port.
    let listener = tokio::net::TcpListener::bind(&address).await?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    state.db.set_schedule(false).await?;
    let job = if enabled {
        Some(tokio::spawn(scheduler(state.clone(), shutdown_rx)))
    } else {
        None
    };
    eprintln!("AI Signal listening on {}", listener.local_addr()?);
    let result = axum::serve(listener, router(state, static_dir))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = shutdown_tx.send(true);
        })
        .await;
    if let Some(job) = job {
        job.await??;
    }
    result?;
    Ok(())
}
