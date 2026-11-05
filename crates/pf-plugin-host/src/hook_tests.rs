use super::*;

#[test]
fn hook_optional_nulls() {
    assert_eq!(hook_optional(String::new()), None);
    assert_eq!(hook_optional("null".into()), None);
    assert_eq!(hook_optional("NULL".into()), None);
    assert_eq!(hook_optional(" {}".into()), Some(" {}".to_string()));
}
