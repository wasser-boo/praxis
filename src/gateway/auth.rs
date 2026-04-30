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
    let secrets = crate::db::secrets::get_secrets();
    let config = crate::config::Config::from_env();
    let admin_password = secrets.dashboard_admin_password.unwrap_or(config.dashboard_admin_password);
    let gateway_api_key = secrets.gateway_api_key.unwrap_or(config.gateway_api_key);

    if payload.password != admin_password {
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
        &EncodingKey::from_secret(gateway_api_key.as_bytes()),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(LoginResponse { token }))
}

pub async fn auth_middleware_fn(
    axum::extract::State(state): axum::extract::State<crate::gateway::GatewayState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(header) => match header.strip_prefix("Bearer ") {
            Some(t) => t,
            None => return Err(StatusCode::UNAUTHORIZED),
        },
        None => {
            if req.uri().path() == "/ws" {
                return Ok(next.run(req).await);
            }
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let jwt_result = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.config.gateway_api_key.as_bytes()),
        &Validation::default(),
    );

    if jwt_result.is_ok() {
        return Ok(next.run(req).await);
    }

    if token == state.config.gateway_api_key {
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
