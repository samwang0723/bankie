use rand::Rng;

pub fn generate_bank_account_number(length: usize) -> String {
    let mut rng = rand::thread_rng();
    let account_number: String = (0..length)
        .map(|_| rng.gen_range(0..10).to_string())
        .collect();
    account_number
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_bank_account_number_length() {
        let length = 10;
        let account_number = generate_bank_account_number(length);
        assert_eq!(account_number.len(), length);
    }

    #[test]
    fn test_generate_bank_account_number_digits() {
        let length = 10;
        let account_number = generate_bank_account_number(length);
        assert!(account_number.chars().all(|c| c.is_ascii_digit()));
    }
}
