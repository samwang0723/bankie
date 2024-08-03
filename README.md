# bankie
Banking system use sub-accounts and ledger management with event-sourcing

## Database creation & migration
Before we start the service, need to make sure all related database and tables are created.
To create database and application user.
```bash
make db-pg-init-main
```
To migrate all table schemas in.
```bash
make db-pg-migrate
```

## Generate Secret
```bash
cargo run --bin bankie -- --mode secret_key
```
After retrieved the secret key, store into ENV variable `JWT_SECRET`

## Generate JWT for tenant
If other service want to use Bankie, they need to communicate with service token,
which is JWT token expired in 1 year, Please perform below script and it will trigger
to create a tenant profile with generated JWT token for runtime verification.
```bash
cargo run --bin bankie -- --mode jwt --service {service_name}
```

## Start server
```bash
cargo run --bin bankie -- --mode server
```

### Update logging level
Make sure you have ENV variable `RUST_LOG=info`, level has trace, debug, info, warning, error.
