use super::toml_lite::*;

fn one(input: &str) -> Entry {
    let mut v = parse(input).unwrap();
    assert_eq!(v.len(), 1, "expected exactly one entry");
    v.remove(0)
}

#[test]
fn empty_and_comment_only_input() {
    assert!(parse("").unwrap().is_empty());
    assert!(parse("# just a comment\n\n   \n# another\n").unwrap().is_empty());
}

#[test]
fn string_value() {
    let e = one("[ui]\nvoice = \"plain\"\n");
    assert_eq!(e.section, "ui");
    assert_eq!(e.key, "voice");
    assert_eq!(e.value, Value::Str("plain".into()));
    assert_eq!(e.line, 2);
}

#[test]
fn integer_values() {
    assert_eq!(one("n = 42").value, Value::Int(42));
    assert_eq!(one("n = -7").value, Value::Int(-7));
}

#[test]
fn string_escapes() {
    let e = one(r#"s = "a\"b\\c\nd\te""#);
    assert_eq!(e.value, Value::Str("a\"b\\c\nd\te".into()));
}

#[test]
fn hash_inside_string_is_not_a_comment() {
    let e = one(r#"s = "a # b" # real comment"#);
    assert_eq!(e.value, Value::Str("a # b".into()));
}

#[test]
fn inline_array() {
    let e = one(r#"roots = ["~/code", "/srv/work"]"#);
    assert_eq!(e.value, Value::StrArray(vec!["~/code".into(), "/srv/work".into()]));
}

#[test]
fn empty_array() {
    assert_eq!(one("x = []").value, Value::StrArray(vec![]));
}

#[test]
fn multiline_array_with_comments_and_trailing_comma() {
    let input = "roots = [\n  \"a\",   # first\n  \"b\",\n]\n";
    assert_eq!(
        one(input).value,
        Value::StrArray(vec!["a".into(), "b".into()])
    );
}

#[test]
fn bracket_inside_array_string_does_not_confuse_completion() {
    let e = one(r#"x = ["a]b"]"#);
    assert_eq!(e.value, Value::StrArray(vec!["a]b".into()]));
}

#[test]
fn sections_scope_keys() {
    let v = parse("[a]\nk = 1\n[b]\nk = 2\n").unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!((v[0].section.as_str(), v[1].section.as_str()), ("a", "b"));
}

#[test]
fn keys_before_any_section_have_empty_section() {
    assert_eq!(one("k = 1").section, "");
}

#[test]
fn duplicate_key_in_same_section_is_an_error() {
    let e = parse("[a]\nk = 1\nk = 2\n").unwrap_err();
    assert_eq!(e.line, 3);
    assert!(e.message.contains("duplicate"));
}

#[test]
fn same_key_in_different_sections_is_fine() {
    assert!(parse("[a]\nk = 1\n[b]\nk = 2\n").is_ok());
}

#[test]
fn error_line_numbers_are_accurate() {
    let e = parse("[a]\nk = 1\n\nbroken line\n").unwrap_err();
    assert_eq!(e.line, 4);
}

#[test]
fn unterminated_string_is_an_error() {
    assert!(parse("s = \"oops\n").is_err());
}

#[test]
fn unterminated_array_is_an_error() {
    let e = parse("x = [\"a\",\n").unwrap_err();
    assert!(e.message.contains("unterminated"));
}

#[test]
fn unsupported_values_are_errors() {
    assert!(parse("x = true").is_err());
    assert!(parse("x = 1.5").is_err());
    assert!(parse("x = [1, 2]").is_err());
    assert!(parse("x = {a = 1}").is_err());
}

#[test]
fn unsupported_escape_is_an_error() {
    assert!(parse(r#"s = "\q""#).is_err());
}

#[test]
fn trailing_text_after_string_is_an_error() {
    assert!(parse(r#"s = "a" "b""#).is_err());
}

#[test]
fn invalid_section_and_key_names() {
    assert!(parse("[a.b]\n").is_err());
    assert!(parse("[]\n").is_err());
    assert!(parse("[open\n").is_err());
    assert!(parse("bad key = 1\n").is_err());
    assert!(parse("= 1\n").is_err());
}

#[test]
fn non_ascii_text_in_strings_is_preserved() {
    let e = one("s = \"caf\u{e9} \u{2713}\"");
    assert_eq!(e.value, Value::Str("caf\u{e9} \u{2713}".into()));
}
