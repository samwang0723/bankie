CREATE TABLE tenants (
    id SERIAL PRIMARY KEY,  -- Unique identifier for the tenant
    name VARCHAR(255) NOT NULL,  -- Name of the tenant
    jwt TEXT NOT NULL,  -- JSON Web Token for the tenant
    status VARCHAR(50) DEFAULT 'active',  -- Status of the tenant (e.g., active, inactive)
    scope VARCHAR(255),  -- Scope of the tenant
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,  -- Timestamp when the tenant was created
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP  -- Timestamp when the tenant was last updated
);
