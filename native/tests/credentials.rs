use otools_releasehub_native::credential::{credential_user, validate_token};
#[test]
fn tokens_are_bounded_and_scoped_without_including_secret_material() {
    assert!(validate_token("ghp_fixtureToken123").is_ok());
    for invalid in ["", "a\nb", "Bearer secret", "https://github.com/?token=x"] {
        assert!(validate_token(invalid).is_err());
    }
    assert!(validate_token(&"x".repeat(4097)).is_err());
    let a = credential_user("/data/plugin-a");
    let b = credential_user("/data/plugin-b");
    assert_ne!(a, b);
    assert!(!a.contains("/data"));
}
