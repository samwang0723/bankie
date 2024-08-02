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
    jwt: &str,
    status: &str,
    scope: &str,
) -> Result<i32, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        INSERT INTO tenants (name, jwt, status, scope)
        VALUES ($1, $2, $3, $4)
        RETURNING id
        "#,
        name,
        jwt,
        status,
        scope
    )
    .fetch_one(pool)
    .await?;

    Ok(rec.id)
}

#[allow(dead_code)]
pub async fn get_tenant_profile(pool: &PgPool, tenant_id: i32) -> Result<Tenant, sqlx::Error> {
    let rec = sqlx::query!(
        r#"
        SELECT id, name, jwt, status, scope
        FROM tenants
        WHERE id = $1
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
