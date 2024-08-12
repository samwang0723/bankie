use rand::Rng;

pub fn generate_bank_account_number(length: usize) -> String {
    let mut rng = rand::thread_rng();
    let account_number: String = (0..length)
        .map(|_| rng.gen_range(0..10).to_string())
        .collect();
    account_number
}
