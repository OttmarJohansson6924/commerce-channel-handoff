use axum::{routing::post, Json, Router};
use commerce_channel_handoff::checkout_handoff::{
    process_order_update, DeliveryReceipt, OrderUpdate, ServiceError,
};
use commerce_channel_handoff::infrai::InfraiClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(InfraiClient::from_env()?);
    let app = Router::new()
        .route("/orders/update", post(handle_order_update))
        .with_state(client);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    println!("order update service listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn handle_order_update(
    axum::extract::State(client): axum::extract::State<Arc<InfraiClient>>,
    Json(update): Json<OrderUpdate>,
) -> Result<Json<DeliveryReceipt>, ServiceError> {
    process_order_update(&client, update).await.map(Json)
}
