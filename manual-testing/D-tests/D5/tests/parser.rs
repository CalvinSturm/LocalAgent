use d5_number_parser::parser::parse_number;

#[test]
fn parses_clean_input() {
    assert_eq!(parse_number("12").unwrap(), 12);
}

#[test]
fn trims_whitespace_before_parsing() {
    assert_eq!(parse_number(" 12\n").unwrap(), 12);
}
