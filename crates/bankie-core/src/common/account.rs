use rand::Rng;

/// Compute Luhn check digit for a numeric string.
fn luhn_check_digit(number: &str) -> u8 {
    let mut sum: u32 = 0;
    let mut double = true; // start doubling from rightmost

    for ch in number.chars().rev() {
        if let Some(digit) = ch.to_digit(10) {
            let val = if double {
                let d = digit * 2;
                if d > 9 {
                    d - 9
                } else {
                    d
                }
            } else {
                digit
            };
            sum += val;
            double = !double;
        }
    }

    let check = (10 - (sum % 10)) % 10;
    check as u8
}

/// Generate a bank account number with Luhn check digit.
/// Format: {random digits}{luhn_check} = total `length` digits.
/// For production, replace the random portion with a DB sequence.
pub fn generate_bank_account_number(length: usize) -> String {
    if length < 2 {
        return "0".repeat(length);
    }
    let mut rng = rand::thread_rng();
    let prefix_len = length - 1;
    let prefix: String = (0..prefix_len)
        .map(|_| rng.gen_range(0..10).to_string())
        .collect();
    let check = luhn_check_digit(&prefix);
    format!("{}{}", prefix, check)
}

/// Generate a bank account number from a sequence value with Luhn check digit.
/// Format: {sequence padded to length-1}{luhn_check}
#[allow(dead_code)]
pub fn generate_account_number_from_sequence(sequence: i64, length: usize) -> String {
    if length < 2 {
        return format!("{}", sequence % 10);
    }
    let prefix_len = length - 1;
    let prefix = format!("{:0>width$}", sequence, width = prefix_len);
    // Truncate if sequence is too large
    let prefix = &prefix[prefix.len().saturating_sub(prefix_len)..];
    let check = luhn_check_digit(prefix);
    format!("{}{}", prefix, check)
}

/// Validate a number string against Luhn algorithm.
#[allow(dead_code)]
pub fn validate_luhn(number: &str) -> bool {
    if number.len() < 2 {
        return false;
    }
    let prefix = &number[..number.len() - 1];
    let expected_check = luhn_check_digit(prefix);
    let actual_check = number
        .chars()
        .last()
        .and_then(|c| c.to_digit(10))
        .unwrap_or(255) as u8;
    expected_check == actual_check
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

    #[test]
    fn test_generate_bank_account_number_luhn_valid() {
        for _ in 0..100 {
            let account_number = generate_bank_account_number(10);
            assert!(
                validate_luhn(&account_number),
                "Failed Luhn for: {}",
                account_number
            );
        }
    }

    #[test]
    fn test_generate_from_sequence() {
        let number = generate_account_number_from_sequence(1000000001, 12);
        assert_eq!(number.len(), 12);
        assert!(validate_luhn(&number));
    }

    #[test]
    fn test_validate_luhn() {
        // Known valid Luhn numbers
        assert!(validate_luhn("79927398713"));
        assert!(!validate_luhn("79927398710"));
    }
}
