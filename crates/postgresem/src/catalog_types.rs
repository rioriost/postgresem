use postgresem_compiler::DataType;

pub fn postgres_data_type(value: &str) -> Option<DataType> {
    let value = value.to_ascii_lowercase();
    if matches!(value.as_str(), "boolean" | "bool") {
        Some(DataType::Boolean)
    } else if matches!(
        value.as_str(),
        "smallint" | "integer" | "bigint" | "int2" | "int4" | "int8"
    ) {
        Some(DataType::Integer)
    } else if value == "numeric"
        || value == "decimal"
        || value.starts_with("numeric(")
        || value.starts_with("decimal(")
    {
        Some(DataType::Numeric)
    } else if value == "text"
        || value == "character varying"
        || value.starts_with("character varying(")
        || value == "varchar"
        || value.starts_with("varchar(")
        || value == "character"
        || value.starts_with("character(")
        || value == "char"
        || value.starts_with("char(")
    {
        Some(DataType::Text)
    } else if value == "date" {
        Some(DataType::Date)
    } else if value == "timestamp without time zone"
        || value.starts_with("timestamp(") && value.ends_with(" without time zone")
    {
        Some(DataType::Timestamp)
    } else if value == "timestamp with time zone"
        || value == "timestamptz"
        || value.starts_with("timestamp(") && value.ends_with(" with time zone")
    {
        Some(DataType::TimestampTz)
    } else {
        None
    }
}

pub fn portable_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use postgresem_compiler::DataType;

    use super::{portable_identifier, postgres_data_type};

    #[test]
    fn maps_only_the_closed_postgresql_type_subset() {
        assert_eq!(postgres_data_type("bigint"), Some(DataType::Integer));
        assert_eq!(postgres_data_type("numeric(18,2)"), Some(DataType::Numeric));
        assert_eq!(
            postgres_data_type("timestamp with time zone"),
            Some(DataType::TimestampTz)
        );
        assert_eq!(postgres_data_type("jsonb"), None);
    }

    #[test]
    fn portable_identifiers_exclude_quoted_or_qualified_names() {
        assert!(portable_identifier("orders_2026"));
        assert!(!portable_identifier("Order Items"));
        assert!(!portable_identifier("commerce.orders"));
    }
}
