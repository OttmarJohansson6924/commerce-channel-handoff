use crate::infrai::{InfraiClient, InfraiError};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignupChannel {
    Email,
    Sms,
}

impl SignupChannel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Sms => "sms",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderState {
    CheckoutConfirmed,
    FulfillmentStarted,
    ReceiptReady,
    Shipped,
}

#[derive(Debug, Deserialize)]
pub struct OrderUpdate {
    pub request_id: String,
    pub order_id: String,
    pub customer_name: String,
    pub email: String,
    pub phone: String,
    pub signup_channel: SignupChannel,
    pub state: OrderState,
}

#[derive(Debug, Serialize)]
pub struct DeliveryReceipt {
    pub order_id: String,
    pub channel: SignupChannel,
    pub message_id: String,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("neither customer channel can receive this order update")]
    NoDeliverableChannel,
    #[error(transparent)]
    Infrai(#[from] InfraiError),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NoDeliverableChannel => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Infrai(InfraiError::Rejected { status, .. }) if status.is_client_error() => {
                *status
            }
            Self::Infrai(InfraiError::MissingApiKey) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Infrai(_) => StatusCode::BAD_GATEWAY,
        };
        (
            status,
            Json(ErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Email,
    Sms,
}

fn choose_route(
    preferred: SignupChannel,
    email_suppressed: bool,
    sms_suppressed: bool,
) -> Result<Route, ServiceError> {
    match (preferred, email_suppressed, sms_suppressed) {
        (SignupChannel::Email, false, _) => Ok(Route::Email),
        (SignupChannel::Sms, _, false) => Ok(Route::Sms),
        (_, true, false) => Ok(Route::Sms),
        (_, false, true) => Ok(Route::Email),
        (_, true, true) => Err(ServiceError::NoDeliverableChannel),
    }
}

pub async fn process_order_update(
    client: &InfraiClient,
    update: OrderUpdate,
) -> Result<DeliveryReceipt, ServiceError> {
    if matches!(update.state, OrderState::CheckoutConfirmed) {
        client
            .create_user(
                &update.email,
                &update.customer_name,
                &update.order_id,
                update.signup_channel.as_str(),
                &format!("{}:account", update.request_id),
            )
            .await?;
    }

    let (email, sms) = tokio::try_join!(
        client.email_suppression(&update.email),
        client.sms_suppression(&update.phone)
    )?;
    let route = choose_route(update.signup_channel, email.suppressed, sms.suppressed)?;
    let (subject, message) = notification_copy(update.state, &update.order_id);
    let message_id = match route {
        Route::Email => {
            let html = format!("<p>{}</p>", message);
            client
                .send_email(
                    &update.email,
                    &subject,
                    &html,
                    &format!("{}:email", update.request_id),
                )
                .await?
                .message_id
        }
        Route::Sms => {
            client
                .send_sms(
                    &update.phone,
                    &message,
                    &format!("{}:sms", update.request_id),
                )
                .await?
                .message_id
        }
    };

    Ok(DeliveryReceipt {
        order_id: update.order_id,
        channel: match route {
            Route::Email => SignupChannel::Email,
            Route::Sms => SignupChannel::Sms,
        },
        message_id,
    })
}

fn notification_copy(state: OrderState, order_id: &str) -> (String, String) {
    let event = match state {
        OrderState::CheckoutConfirmed => "checkout is confirmed",
        OrderState::FulfillmentStarted => "fulfillment has started",
        OrderState::ReceiptReady => "receipt is ready",
        OrderState::Shipped => "order has shipped",
    };
    (
        format!("Order {order_id}: {event}"),
        format!("Order {order_id}: {event}."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_signup_falls_back_to_sms_when_email_is_suppressed() {
        let route = choose_route(SignupChannel::Email, true, false).unwrap();
        assert_eq!(route, Route::Sms);
    }

    #[test]
    fn two_suppressions_stop_delivery() {
        let result = choose_route(SignupChannel::Sms, true, true);
        assert!(matches!(result, Err(ServiceError::NoDeliverableChannel)));
    }
}
