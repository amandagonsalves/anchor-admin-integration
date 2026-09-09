use rust_decimal::Decimal;

/// Flat 10% fee, mirroring the reference server's `RateService.calculateFee`.
pub fn flat_fee(amount: Decimal) -> Decimal {
    (amount * Decimal::new(10, 2)).round_dp(7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_percent_of_fifty_is_five() {
        assert_eq!(flat_fee(Decimal::new(50, 0)), Decimal::new(500, 2));
    }

    #[test]
    fn rounds_to_seven_decimal_places() {
        let fee = flat_fee(Decimal::new(1, 7));
        assert!(fee.scale() <= 7);
    }
}
