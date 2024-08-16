use lazy_static::lazy_static;
use snowflake::SnowflakeIdGenerator;
use std::sync::Mutex;

lazy_static! {
    static ref GENERATOR: Mutex<SnowflakeIdGenerator> = Mutex::new(SnowflakeIdGenerator::new(1, 1));
}

pub fn generate_transaction_reference(prefix: &str) -> String {
    let mut generator = GENERATOR.lock().unwrap();
    let unique_id = generator.real_time_generate();
    format!("{}{}", prefix, unique_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_transaction_reference() {
        let prefix = "TXN";
        let result = generate_transaction_reference(prefix);
        assert!(result.starts_with(prefix));
        assert!(result.len() > prefix.len());
    }
}
