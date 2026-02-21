use crate::memory::{TourneyData, TourneyReader, TourneyState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::time::{self, Duration};

const POLLING_INTERVAL_MS: u64 = 16;
const SCAN_INTERVAL_MS: u64 = 1000;
const VALIDATE_INTERVAL: u32 = 62;

macro_rules! log_info {
    ($($arg:tt)*) => { log::info!("[server] {}", format!($($arg)*)) };
}
macro_rules! log_debug {
    ($($arg:tt)*) => { log::debug!("[server] {}", format!($($arg)*)) };
}
macro_rules! log_warn {
    ($($arg:tt)*) => { log::warn!("[server] {}", format!($($arg)*)) };
}

pub async fn run(port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    log_debug!("Initializing memory reader");
    let reader = Arc::new(TourneyReader::new());

    let reader_loop = reader.clone();
    tokio::spawn(async move {
        run_state_machine(reader_loop).await;
    });

    let app = Router::new()
        .route("/", get(ws_handler))
        .route("/json", get(json_handler))
        .with_state(reader);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    log_info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn run_state_machine(reader: Arc<TourneyReader>) {
    let mut poll_count: u32 = 0;

    loop {
        match reader.get_state().await {
            TourneyState::Scanning => {
                poll_count = 0;
                log_debug!("Scanning...");

                if !reader.try_connect().await {
                    time::sleep(Duration::from_millis(SCAN_INTERVAL_MS)).await;
                }
            }
            TourneyState::Connected => {
                time::sleep(Duration::from_millis(POLLING_INTERVAL_MS)).await;

                if !reader.poll().await {
                    log_warn!("Lost connection to client instances, reconnecting...");
                    reader.disconnect().await;
                    continue;
                }

                poll_count += 1;
                if poll_count >= VALIDATE_INTERVAL {
                    poll_count = 0;
                    if !reader.validate_clients().await {
                        log_info!("Tournament client instances changed, reconnecting...");
                        reader.disconnect().await;
                    }
                }
            }
        }
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(reader): State<Arc<TourneyReader>>,
) -> impl IntoResponse {
    log_debug!("New WebSocket connection");
    ws.on_upgrade(move |socket| handle_socket(socket, reader))
}

async fn handle_socket(mut socket: WebSocket, reader: Arc<TourneyReader>) {
    let mut interval = time::interval(Duration::from_millis(POLLING_INTERVAL_MS));

    loop {
        interval.tick().await;

        let data = reader.get_data().await;
        let json = match serde_json::to_string(&data) {
            Ok(j) => j,
            Err(_) => continue,
        };

        if socket.send(Message::Text(json)).await.is_err() {
            log_debug!("WebSocket client disconnected");
            break;
        }
    }
}

async fn json_handler(State(reader): State<Arc<TourneyReader>>) -> Json<TourneyData> {
    Json(reader.get_data().await)
}
