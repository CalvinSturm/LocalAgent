use d3_parse_int::parse_count;

#[test]
fn parses_plain_number() {
    assert_eq!(parse_count("7"), 7);
}

#[test]
fn trims_whitespace_before_parsing() {
    assert_eq!(parse_count(" 7 \n"), 7);
}
