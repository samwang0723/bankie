use chrono::Local;
use sqlx::PgPool;

#[allow(dead_code)]
#[derive(Debug)]
pub struct Tenant {
    pub id: i32,
    pub name: String,
    pub jwt: String,
    pub status: String,
    pub scope: Option<String>,
}

pub async fn create_tenant_profile(
    pool: &PgPool,
    name: &str,
    scope: &str,
) -> Result<i32, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        INSERT INTO tenants (name, status, jwt, scope)
        VALUES ($1, 'inactive', '', $2)
        RETURNING id
        "#,
        name,
        scope
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn update_tenant_profile(pool: &PgPool, id: i32, jwt: &str) -> Result<i32, sqlx::Error> {
    let dt = Local::now();
    let naive_utc = dt.naive_utc();
    let rec = sqlx::query!(
        r#"
        UPDATE tenants
        SET jwt = $2, status = 'active', updated_at = $3
        WHERE id = $1
        RETURNING id
        "#,
        id,
        jwt,
        naive_utc
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

pub async fn get_tenant_profile(pool: &PgPool, tenant_id: i32) -> Result<Tenant, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        SELECT id, name, jwt, status, scope
        FROM tenants
        WHERE id = $1 AND status='active'
        "#,
        tenant_id
    )
    .fetch_one(pool)
    .await?;

    Ok(Tenant {
        id: rec.id,
        name: rec.name,
        jwt: rec.jwt,
        status: rec.status.expect("no status"),
        scope: Some(rec.scope.expect("no scope")),
    })
}
