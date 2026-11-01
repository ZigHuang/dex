use super::*;

#[test]
fn classifies_personal_access_tokens_by_prefix() {
    assert!(matches!(
        classify_dex_access_token("at-example"),
        DexAccessToken::PersonalAccessToken("at-example")
    ));
    assert!(matches!(
        classify_dex_access_token("header.payload.signature"),
        DexAccessToken::AgentIdentityJwt("header.payload.signature")
    ));
}
