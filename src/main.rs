mod env;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json,
    Router,
};
use env::Env;
use evmap::{ReadHandleFactory, WriteHandle};
use kv_store_lib::{InternalValue, KeyType, Store};
use parking_lot::Mutex;
use serde_json::value::Value;
use std::{error::Error, net::SocketAddr, sync::Arc};

pub struct AppState {
    pub store_reader: ReadHandleFactory<KeyType, InternalValue>,
    pub store_writer: Arc<Mutex<WriteHandle<KeyType, InternalValue>>>,
}

pub fn routes(app_state: Arc<AppState>) -> Router {
    Router::new()
        .route("/:key", get(get_key).delete(remove_key).post(add_key))
        .route("/:key/:ttl", post(add_key_with_ttl))
        .with_state(app_state)
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn Error>> {
    let _ = dotenv::dotenv().ok();
    let env = envy::from_env::<Env>()?;
    env_logger::init();
    let (read_handle, write_handle, timer_handler) = Store::init().await;
    let app_state = Arc::new(AppState {
        store_reader: read_handle,
        store_writer: write_handle,
    });

    let app = routes(app_state);
    let addr = SocketAddr::from(([127, 0, 0, 1], env.server_port as u16));
    log::info!("listening on {}", addr);
    let server_future = axum::Server::bind(&addr).serve(app.into_make_service());
    let _ = tokio::join!(server_future, timer_handler);
    Ok(())
}

async fn get_key(Path(key): Path<KeyType>, State(app_state): State<Arc<AppState>>) -> Response {
    if let Ok(Some(value)) = Store::get(app_state.store_reader.handle(), key) {
        return (StatusCode::OK, Json(&value)).into_response();
    }
    (StatusCode::NOT_FOUND, "Key not found").into_response()
}

pub async fn add_key(
    Path(key): Path<KeyType>,
    State(app_state): State<Arc<AppState>>,
    Json(value): Json<Value>,
) -> Response {
    if let Err(e) = Store::insert(&app_state.store_writer, key, value, None) {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }
    (StatusCode::OK, "Key queued for addition").into_response()
}

pub async fn add_key_with_ttl(
    Path((key, ttl)): Path<(KeyType, i64)>,
    State(app_state): State<Arc<AppState>>,
    Json(value): Json<Value>,
) -> Response {
    if let Err(e) = Store::insert(&app_state.store_writer, key, value, Some(ttl)) {
        return (StatusCode::BAD_REQUEST, e.to_string()).into_response();
    }
    (StatusCode::OK, "Key queued for addition").into_response()
}

pub async fn remove_key(Path(key): Path<KeyType>, State(app_state): State<Arc<AppState>>) -> Response {
    if let Err(e) = Store::delete(&app_state.store_writer, key) {
        return (StatusCode::NOT_FOUND, e.to_string()).into_response();
    }
    (StatusCode::OK, "Key queued for removal").into_response()
}
