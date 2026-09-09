pub struct FieldSpec {
    pub name: &'static str,
    pub field_type: &'static str,
    pub description: &'static str,
}

const SEP6_DEPOSIT_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "birth_date",
        field_type: "date",
        description: "Date of birth",
    },
    FieldSpec {
        name: "id_type",
        field_type: "string",
        description: "Type of ID document",
    },
    FieldSpec {
        name: "id_country_code",
        field_type: "string",
        description: "Country of the ID document",
    },
    FieldSpec {
        name: "id_issue_date",
        field_type: "date",
        description: "ID document issue date",
    },
    FieldSpec {
        name: "id_expiration_date",
        field_type: "date",
        description: "ID document expiration date",
    },
    FieldSpec {
        name: "id_number",
        field_type: "string",
        description: "ID document number",
    },
    FieldSpec {
        name: "address",
        field_type: "string",
        description: "Full physical address",
    },
];

const BANK_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "bank_account_number",
        field_type: "string",
        description: "Bank account number",
    },
    FieldSpec {
        name: "bank_account_type",
        field_type: "string",
        description: "Bank account type (checking/savings)",
    },
    FieldSpec {
        name: "bank_number",
        field_type: "string",
        description: "Bank routing number",
    },
    FieldSpec {
        name: "bank_branch_number",
        field_type: "string",
        description: "Bank branch number",
    },
];

const SEP24_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "first_name",
        field_type: "string",
        description: "Given name",
    },
    FieldSpec {
        name: "last_name",
        field_type: "string",
        description: "Family name",
    },
    FieldSpec {
        name: "email_address",
        field_type: "string",
        description: "Email address",
    },
];

const SEP31_SENDER_FIELDS: &[FieldSpec] = &[
    FieldSpec {
        name: "first_name",
        field_type: "string",
        description: "Given name",
    },
    FieldSpec {
        name: "last_name",
        field_type: "string",
        description: "Family name",
    },
];

/// Required SEP-9 fields for a customer of the given `type`, mirroring the
/// reference server's per-kind required-KYC lists (SEP-6 deposit/withdrawal,
/// SEP-31 receiver bank details), collapsed into a static, per-type table
/// since this anchor doesn't track per-transaction dynamic requirements.
pub fn required_fields(customer_type: &str) -> Vec<&'static FieldSpec> {
    match customer_type {
        "sep6" | "sep6-withdrawal" => SEP6_DEPOSIT_FIELDS
            .iter()
            .chain(BANK_FIELDS.iter())
            .collect(),
        "sep6-deposit" => SEP6_DEPOSIT_FIELDS.iter().collect(),
        "sep24" => SEP24_FIELDS.iter().collect(),
        "sep31-sender" => SEP31_SENDER_FIELDS.iter().collect(),
        "sep31-receiver" => BANK_FIELDS.iter().collect(),
        _ => SEP24_FIELDS.iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sep31_receiver_requires_bank_fields_only() {
        let fields = required_fields("sep31-receiver");
        assert_eq!(fields.len(), 4);
        assert!(fields.iter().all(|f| f.name.starts_with("bank_")));
    }

    #[test]
    fn sep6_withdrawal_requires_deposit_fields_plus_bank() {
        let fields = required_fields("sep6");
        assert_eq!(fields.len(), SEP6_DEPOSIT_FIELDS.len() + BANK_FIELDS.len());
    }

    #[test]
    fn unknown_type_falls_back_to_sep24() {
        assert_eq!(
            required_fields("bogus").len(),
            required_fields("sep24").len()
        );
    }
}
