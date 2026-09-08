use crate::auth::Claims;
use axum::{
    extract::{FromRequestParts, Request},
    http::{request::Parts, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use std::env;

/// Extractor that pulls the verified wallet address out of the JWT.
#[derive(Clone)]
pub struct AuthenticatedOwner(pub String);

#[async_trait::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthenticatedOwner {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or((StatusCode::UNAUTHORIZED, "Missing Authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or((StatusCode::UNAUTHORIZED, "Invalid Authorization header format"))?;

        let secret = env::var("JWT_SECRET").unwrap_or_default();
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid or expired token"))?;

        Ok(AuthenticatedOwner(data.claims.sub))
    }
}

/// Axum middleware that rejects requests without a valid JWT.
pub async fn require_auth(
    parts: Request,
    next: Next,
) -> Result<Response, (StatusCode, &'static str)> {
    let (mut p, body) = parts.into_parts();
    AuthenticatedOwner::from_request_parts(&mut p, &()).await?;
    Ok(next.run(Request::from_parts(p, body)).await)
}
