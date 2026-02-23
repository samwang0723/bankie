use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

use crate::common::money::default_precision;
use crate::domain::finance::SettlementReportRow;

/// Maximum allowed date range for a single report (90 days).
pub const MAX_REPORT_DAYS: i64 = 90;

/// UTF-8 BOM for Excel compatibility.
const UTF8_BOM: &str = "\u{FEFF}";

/// Escape a CSV field value to prevent CSV injection and handle special characters.
///
/// - Fields containing commas, quotes, or newlines are wrapped in double quotes.
/// - Double quotes within fields are escaped by doubling them.
/// - Fields starting with `=`, `+`, `-`, `@` are prefixed with a single quote
///   to prevent CSV injection in spreadsheet applications.
pub fn format_csv_field(value: &str) -> String {
    // CSV injection prevention: prefix dangerous leading characters
    let safe_value = if value.starts_with('=')
        || value.starts_with('+')
        || value.starts_with('-')
        || value.starts_with('@')
    {
        format!("'{}", value)
    } else {
        value.to_string()
    };

    // If the field contains commas, quotes, or newlines, wrap in quotes
    if safe_value.contains(',') || safe_value.contains('"') || safe_value.contains('\n') {
        let escaped = safe_value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        safe_value
    }
}

/// Format a decimal amount with currency-appropriate precision.
fn format_amount(amount: Decimal, currency: &str) -> String {
    let precision = default_precision(currency) as usize;
    if amount == Decimal::ZERO {
        String::new()
    } else {
        format!("{:.prec$}", amount, prec = precision)
    }
}

/// Format a decimal amount with currency-appropriate precision, always showing the value.
fn format_amount_always(amount: Decimal, currency: &str) -> String {
    let precision = default_precision(currency) as usize;
    format!("{:.prec$}", amount, prec = precision)
}

/// Compute running balances for each row starting from the opening balance.
///
/// Returns a vector of (row, running_balance) pairs where running_balance
/// reflects the account balance after that transaction.
pub fn compute_running_balances(
    rows: &[SettlementReportRow],
    opening_balance: Decimal,
) -> Vec<Decimal> {
    let mut balance = opening_balance;
    let mut balances = Vec::with_capacity(rows.len());

    for row in rows {
        // Credit increases balance, debit decreases it
        balance = balance + row.credit_amount - row.debit_amount;
        balances.push(balance);
    }

    balances
}

/// Write the CSV header comment block with report metadata.
pub fn write_csv_header(
    account_number: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    currency: &str,
    opening_balance: Decimal,
    generated_at: DateTime<Utc>,
) -> String {
    let precision = default_precision(currency) as usize;
    format!(
        "# Settlement Report\n\
         # Account: {}\n\
         # Period: {} to {}\n\
         # Currency: {}\n\
         # Generated: {}\n\
         # Opening Balance: {:.prec$}\n\
         #\n",
        account_number,
        start_date,
        end_date,
        currency,
        generated_at.format("%Y-%m-%dT%H:%M:%SZ"),
        opening_balance,
        prec = precision,
    )
}

/// Write the CSV footer with summary totals.
pub fn write_csv_footer(
    total_debits: Decimal,
    total_credits: Decimal,
    closing_balance: Decimal,
    count: usize,
    currency: &str,
) -> String {
    let precision = default_precision(currency) as usize;
    format!(
        "#\n\
         # Total Debits: {:.prec$}\n\
         # Total Credits: {:.prec$}\n\
         # Closing Balance: {:.prec$}\n\
         # Transaction Count: {}\n",
        total_debits,
        total_credits,
        closing_balance,
        count,
        prec = precision,
    )
}

/// Generate a complete settlement report as a CSV string.
///
/// The CSV includes:
/// - UTF-8 BOM for Excel compatibility
/// - Metadata header (account info, period, opening balance)
/// - Column headers
/// - Transaction rows with running balances
/// - Summary footer (totals, closing balance, count)
pub fn generate_settlement_csv(
    rows: &[SettlementReportRow],
    opening_balance: Decimal,
    account_number: &str,
    start_date: NaiveDate,
    end_date: NaiveDate,
    currency: &str,
    generated_at: DateTime<Utc>,
) -> String {
    let mut csv = String::new();

    // UTF-8 BOM
    csv.push_str(UTF8_BOM);

    // Header
    csv.push_str(&write_csv_header(
        account_number,
        start_date,
        end_date,
        currency,
        opening_balance,
        generated_at,
    ));

    // Column headers
    csv.push_str("transaction_date,value_date,transaction_reference,transaction_type,debit_amount,credit_amount,currency,description,status,running_balance,account_number,journal_entry_id\n");

    // Compute running balances
    let running_balances = compute_running_balances(rows, opening_balance);

    // Compute totals
    let mut total_debits = Decimal::ZERO;
    let mut total_credits = Decimal::ZERO;

    for (i, row) in rows.iter().enumerate() {
        total_debits += row.debit_amount;
        total_credits += row.credit_amount;

        let tx_type = if row.debit_amount > Decimal::ZERO {
            if row.transaction_reference.contains("TR") {
                "transfer"
            } else {
                "withdrawal"
            }
        } else {
            "deposit"
        };

        let description = row.description.as_deref().unwrap_or("");
        let journal_id = row
            .journal_entry_id
            .map(|id| id.to_string())
            .unwrap_or_default();
        let acct_num = row.account_number.as_deref().unwrap_or("");

        let line = format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            row.transaction_date.format("%Y-%m-%d"),
            row.transaction_date.format("%Y-%m-%d"), // value_date = transaction_date for now
            format_csv_field(&row.transaction_reference),
            tx_type,
            format_amount(row.debit_amount, currency),
            format_amount(row.credit_amount, currency),
            currency,
            format_csv_field(description),
            row.status,
            format_amount_always(running_balances[i], currency),
            format_csv_field(acct_num),
            journal_id,
        );
        csv.push_str(&line);
    }

    // Closing balance
    let closing_balance = running_balances.last().copied().unwrap_or(opening_balance);

    // Footer
    csv.push_str(&write_csv_footer(
        total_debits,
        total_credits,
        closing_balance,
        rows.len(),
        currency,
    ));

    csv
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_row(
        reference: &str,
        debit: Decimal,
        credit: Decimal,
        currency: &str,
        description: Option<&str>,
    ) -> SettlementReportRow {
        SettlementReportRow {
            transaction_date: Utc.with_ymd_and_hms(2026, 2, 15, 10, 0, 0).unwrap(),
            transaction_reference: reference.to_string(),
            amount: if debit > Decimal::ZERO { debit } else { credit },
            currency: currency.to_string(),
            description: description.map(|s| s.to_string()),
            status: "completed".to_string(),
            journal_entry_id: Some(Uuid::new_v4()),
            debit_amount: debit,
            credit_amount: credit,
            account_number: Some("8001234567".to_string()),
        }
    }

    #[test]
    fn test_format_csv_field_normal() {
        assert_eq!(format_csv_field("hello"), "hello");
        assert_eq!(format_csv_field("simple text"), "simple text");
        assert_eq!(format_csv_field("12345"), "12345");
    }

    #[test]
    fn test_format_csv_field_with_commas() {
        assert_eq!(format_csv_field("hello, world"), "\"hello, world\"");
        assert_eq!(format_csv_field("one, two, three"), "\"one, two, three\"");
    }

    #[test]
    fn test_format_csv_field_with_quotes() {
        assert_eq!(format_csv_field("say \"hello\""), "\"say \"\"hello\"\"\"");
    }

    #[test]
    fn test_format_csv_field_injection_prevention() {
        // Fields starting with dangerous characters get prefixed with '
        assert_eq!(format_csv_field("=SUM(A1:A10)"), "'=SUM(A1:A10)");
        assert_eq!(format_csv_field("+cmd"), "'+cmd");
        assert_eq!(format_csv_field("-evil"), "'-evil");
        assert_eq!(format_csv_field("@import"), "'@import");
    }

    #[test]
    fn test_format_csv_field_injection_with_commas() {
        // Injection prevention + quoting
        assert_eq!(format_csv_field("=SUM(A1,A2)"), "\"'=SUM(A1,A2)\"");
    }

    #[test]
    fn test_compute_running_balances_empty() {
        let rows: Vec<SettlementReportRow> = vec![];
        let balances = compute_running_balances(&rows, dec!(1000.00));
        assert!(balances.is_empty());
    }

    #[test]
    fn test_compute_running_balances_credits_only() {
        let rows = vec![
            make_row("DE-001", dec!(0), dec!(500.00), "USD", None),
            make_row("DE-002", dec!(0), dec!(300.00), "USD", None),
        ];
        let balances = compute_running_balances(&rows, dec!(1000.00));
        assert_eq!(balances, vec![dec!(1500.00), dec!(1800.00)]);
    }

    #[test]
    fn test_compute_running_balances_mixed() {
        let rows = vec![
            make_row("DE-001", dec!(0), dec!(1000.00), "USD", None),
            make_row("WI-001", dec!(200.00), dec!(0), "USD", None),
            make_row("DE-002", dec!(0), dec!(50.00), "USD", None),
        ];
        let balances = compute_running_balances(&rows, dec!(0));
        assert_eq!(balances, vec![dec!(1000.00), dec!(800.00), dec!(850.00)]);
    }

    #[test]
    fn test_format_amount_zero() {
        assert_eq!(format_amount(dec!(0), "USD"), "");
        assert_eq!(format_amount(dec!(0), "BTC"), "");
    }

    #[test]
    fn test_format_amount_nonzero() {
        assert_eq!(format_amount(dec!(100.50), "USD"), "100.50");
        assert_eq!(format_amount(dec!(5000), "TWD"), "5000");
        assert_eq!(format_amount(dec!(0.00123456), "BTC"), "0.00123456");
    }

    #[test]
    fn test_format_amount_always() {
        assert_eq!(format_amount_always(dec!(0), "USD"), "0.00");
        assert_eq!(format_amount_always(dec!(1000), "USD"), "1000.00");
        assert_eq!(format_amount_always(dec!(0), "TWD"), "0");
    }

    #[test]
    fn test_csv_header_format() {
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let header = write_csv_header(
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "USD",
            dec!(1000.00),
            generated,
        );
        assert!(header.starts_with("# Settlement Report\n"));
        assert!(header.contains("# Account: 8001234567\n"));
        assert!(header.contains("# Period: 2026-02-01 to 2026-02-28\n"));
        assert!(header.contains("# Currency: USD\n"));
        assert!(header.contains("# Generated: 2026-02-23T14:30:00Z\n"));
        assert!(header.contains("# Opening Balance: 1000.00\n"));
        assert!(header.ends_with("#\n"));
    }

    #[test]
    fn test_csv_footer_format() {
        let footer = write_csv_footer(dec!(200.00), dec!(1000.00), dec!(800.00), 2, "USD");
        assert!(footer.contains("# Total Debits: 200.00\n"));
        assert!(footer.contains("# Total Credits: 1000.00\n"));
        assert!(footer.contains("# Closing Balance: 800.00\n"));
        assert!(footer.contains("# Transaction Count: 2\n"));
    }

    #[test]
    fn test_generate_settlement_csv_basic() {
        let rows = vec![
            make_row(
                "DE-001",
                dec!(0),
                dec!(1000.00),
                "USD",
                Some("Initial deposit"),
            ),
            make_row(
                "WI-001",
                dec!(200.00),
                dec!(0),
                "USD",
                Some("ATM withdrawal"),
            ),
        ];
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let csv = generate_settlement_csv(
            &rows,
            dec!(0),
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "USD",
            generated,
        );

        // Check BOM
        assert!(csv.starts_with(UTF8_BOM));

        // Check header
        assert!(csv.contains("# Settlement Report\n"));
        assert!(csv.contains("# Opening Balance: 0.00\n"));

        // Check column headers
        assert!(csv.contains("transaction_date,value_date,transaction_reference,transaction_type,debit_amount,credit_amount,currency,description,status,running_balance,account_number,journal_entry_id\n"));

        // Check data rows contain expected content
        assert!(csv.contains("DE-001"));
        assert!(csv.contains("deposit"));
        assert!(csv.contains("1000.00"));
        assert!(csv.contains("WI-001"));
        assert!(csv.contains("withdrawal"));
        assert!(csv.contains("200.00"));

        // Check footer
        assert!(csv.contains("# Total Debits: 200.00\n"));
        assert!(csv.contains("# Total Credits: 1000.00\n"));
        assert!(csv.contains("# Closing Balance: 800.00\n"));
        assert!(csv.contains("# Transaction Count: 2\n"));
    }

    #[test]
    fn test_generate_settlement_csv_empty() {
        let rows: Vec<SettlementReportRow> = vec![];
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let csv = generate_settlement_csv(
            &rows,
            dec!(500.00),
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "USD",
            generated,
        );

        // Should still have header and footer
        assert!(csv.starts_with(UTF8_BOM));
        assert!(csv.contains("# Settlement Report\n"));
        assert!(csv.contains("# Opening Balance: 500.00\n"));
        assert!(csv.contains("# Total Debits: 0.00\n"));
        assert!(csv.contains("# Total Credits: 0.00\n"));
        assert!(csv.contains("# Closing Balance: 500.00\n"));
        assert!(csv.contains("# Transaction Count: 0\n"));
    }

    #[test]
    fn test_generate_settlement_csv_transfer() {
        let rows = vec![make_row(
            "TR-001",
            dec!(100.00),
            dec!(0),
            "USD",
            Some("Transfer out"),
        )];
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let csv = generate_settlement_csv(
            &rows,
            dec!(1000.00),
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "USD",
            generated,
        );

        assert!(csv.contains("transfer"));
        assert!(csv.contains("# Closing Balance: 900.00\n"));
    }

    #[test]
    fn test_generate_settlement_csv_twd_precision() {
        let rows = vec![make_row(
            "DE-001",
            dec!(0),
            dec!(5000),
            "TWD",
            Some("Deposit"),
        )];
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let csv = generate_settlement_csv(
            &rows,
            dec!(10000),
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "TWD",
            generated,
        );

        assert!(csv.contains("# Opening Balance: 10000\n"));
        assert!(csv.contains("# Closing Balance: 15000\n"));
    }

    #[test]
    fn test_description_with_special_characters() {
        let rows = vec![make_row(
            "DE-001",
            dec!(0),
            dec!(100.00),
            "USD",
            Some("Payment for \"services, Inc.\""),
        )];
        let generated = Utc.with_ymd_and_hms(2026, 2, 23, 14, 30, 0).unwrap();
        let csv = generate_settlement_csv(
            &rows,
            dec!(0),
            "8001234567",
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            "USD",
            generated,
        );

        // Description with commas and quotes should be properly escaped
        assert!(csv.contains("\"Payment for \"\"services, Inc.\"\"\""));
    }
}
