use chrono::{Duration, Utc};
use jsonwebtoken::{encode, EncodingKey, Header};
use postgres_es::default_postgress_pool;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres};
use tracing::debug;

use crate::{auth::tenant::update_tenant_profile, configs::settings::SETTINGS};

use super::tenant::create_tenant_profile;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String, // Issuer
    pub sub: String, // Subject (often user_id)
    pub aud: String, // Audience
    pub exp: usize,  // Expiration time
    pub iat: usize,  // Issued at

    #[serde(rename = "scope")]
    pub scopes: Vec<String>,
    pub tenant_id: i32,
}

pub fn generate_secret_key(length: usize) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                            abcdefghijklmnopqrstuvwxyz\
                            0123456789)(*&^%$#@!~";
    let mut rng = thread_rng();

    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub async fn generate_jwt(service_id: &str, secret_key: &str) -> Result<String, sqlx::Error> {
    let expiration = Utc::now()
        .checked_add_signed(Duration::days(365))
        .expect("valid timestamp")
        .timestamp();

    let pool: Pool<Postgres> = default_postgress_pool(&SETTINGS.database.connection_string()).await;
    let tenant_id = create_tenant_profile(
        &pool,
        service_id,
        "bank-account:read bank-account:write ledger:read",
    )
    .await?;
    debug!("Tenant ID: {}", tenant_id);

    let claims = Claims {
        sub: service_id.to_owned(),
        exp: expiration as usize,
        iat: Utc::now().timestamp() as usize,
        iss: "bankie".to_owned(),
        aud: "service".to_owned(),
        scopes: vec![
            "bank-account:read".to_owned(),
            "bank-account:write".to_owned(),
            "ledger:read".to_owned(),
        ],
        tenant_id,
    };

    let header = Header::default();
    let encoding_key = EncodingKey::from_secret(secret_key.as_bytes());
    let jwt_token = encode(&header, &claims, &encoding_key).unwrap();

    update_tenant_profile(&pool, tenant_id, &jwt_token).await?;

    Ok(jwt_token)
}
