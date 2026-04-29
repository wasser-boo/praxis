use axum::{extract::Request, middleware::Next, response::Response, http::StatusCode, Json};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
}

pub async fn login_handler(
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let config = crate::config::Config::from_env();

    if payload.password != config.dashboard_admin_password {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let claims = Claims {
        sub: "admin".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(24)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.gateway_api_key.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
}

pub async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    let config = crate::config::Config::from_env();

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_result = decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.gateway_api_key.as_bytes()),
        &Validation::default(),
    );

    if jwt_result.is_ok() {
        return Ok(next.run(req).await);
    }

    if token == config.gateway_api_key {
        return Ok(next.run(req).await);
    }

    Err(StatusCode::UNAUTHORIZED)
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_claims_serialization() {
        let claims = Claims {
            sub: "test".to_string(),
            exp: 1234567890,
            iat: 1234567800,
        };
        let json = serde_json::to_string(&claims).unwrap();
        assert!(json.contains("test"));
    }
}
