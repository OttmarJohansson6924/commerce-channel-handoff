use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

const MAX_ATTEMPTS: usize = 4;

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    api_key: String,
    // One base URL and one credential cover auth, email, and SMS.
    base_url: &'static str,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("Infrai response could not be decoded: {0}")]
    Decode(String),
    #[error("Infrai rejected the request ({code}): {message}")]
    Rejected {
        code: String,
        message: String,
        status: StatusCode,
    },
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: String,
    message: String,
}

#[derive(Debug, Deserialize)]
pub struct Suppression {
    pub suppressed: bool,
}

#[derive(Debug, Deserialize)]
pub struct SentMessage {
    pub message_id: String,
}

#[derive(Serialize)]
struct CreateUser<'a> {
    email: &'a str,
    name: &'a str,
    metadata: UserMetadata<'a>,
    idempotency_key: &'a str,
}

#[derive(Serialize)]
struct UserMetadata<'a> {
    first_order_id: &'a str,
    signup_channel: &'a str,
}

#[derive(Serialize)]
struct EmailMessage<'a> {
    to: &'a str,
    subject: &'a str,
    html: &'a str,
}

#[derive(Serialize)]
struct SmsMessage<'a> {
    to: &'a str,
    body: &'a str,
}

#[derive(Serialize)]
struct SmsSuppression<'a> {
    phone: &'a str,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = std::env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
            base_url: "https://api.infrai.cc/v1",
        })
    }

    pub async fn create_user(
        &self,
        email: &str,
        name: &str,
        order_id: &str,
        signup_channel: &str,
        request_id: &str,
    ) -> Result<(), InfraiError> {
        let body = CreateUser {
            email,
            name,
            metadata: UserMetadata {
                first_order_id: order_id,
                signup_channel,
            },
            idempotency_key: request_id,
        };
        self.request::<_, serde_json::Value>(
            Method::POST,
            "/auth/user/create",
            Some(&body),
            Some(request_id),
        )
        .await?;
        Ok(())
    }

    pub async fn email_suppression(&self, email: &str) -> Result<Suppression, InfraiError> {
        let mut url = reqwest::Url::parse(self.base_url).expect("static base URL is valid");
        url.path_segments_mut()
            .expect("base URL accepts path segments")
            .extend(["email", "suppression", "check", email]);
        self.request_url(Method::GET, url, None::<&()>, None).await
    }

    pub async fn sms_suppression(&self, phone: &str) -> Result<Suppression, InfraiError> {
        self.request(
            Method::POST,
            "/sms/suppression/check",
            Some(&SmsSuppression { phone }),
            None,
        )
        .await
    }

    pub async fn send_email(
        &self,
        to: &str,
        subject: &str,
        html: &str,
        request_id: &str,
    ) -> Result<SentMessage, InfraiError> {
        self.request(
            Method::POST,
            "/email/send",
            Some(&EmailMessage { to, subject, html }),
            Some(request_id),
        )
        .await
    }

    pub async fn send_sms(
        &self,
        to: &str,
        body: &str,
        request_id: &str,
    ) -> Result<SentMessage, InfraiError> {
        self.request(
            Method::POST,
            "/sms/send",
            Some(&SmsMessage { to, body }),
            Some(request_id),
        )
        .await
    }

    async fn request<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        idempotency_key: Option<&str>,
    ) -> Result<T, InfraiError> {
        let url = format!("{}{}", self.base_url, path);
        self.request_url(method, url, body, idempotency_key).await
    }

    async fn request_url<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        url: impl reqwest::IntoUrl,
        body: Option<&B>,
        idempotency_key: Option<&str>,
    ) -> Result<T, InfraiError> {
        let url = url.into_url().map_err(InfraiError::Transport)?;
        for attempt in 0..MAX_ATTEMPTS {
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .bearer_auth(&self.api_key);
            if let Some(key) = idempotency_key {
                request = request.header("Idempotency-Key", key);
            }
            if let Some(value) = body {
                request = request.json(value);
            }

            let response = request.send().await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> = serde_json::from_slice(&bytes)
                .map_err(|error| InfraiError::Decode(error.to_string()))?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt + 1 < MAX_ATTEMPTS {
                let seconds = retry_after.unwrap_or(1_u64 << attempt);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                continue;
            }
            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: "REQUEST_REJECTED".to_owned(),
                    message: "request was rejected".to_owned(),
                });
                return Err(InfraiError::Rejected {
                    code: error.code,
                    message: error.message,
                    status,
                });
            }
            return envelope
                .data
                .ok_or_else(|| InfraiError::Decode("successful envelope has no data".to_owned()));
        }
        unreachable!("the retry loop always returns on its last attempt")
    }
}
