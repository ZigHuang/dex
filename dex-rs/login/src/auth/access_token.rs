const PERSONAL_ACCESS_TOKEN_PREFIX: &str = "at-";

pub(super) enum DexAccessToken<'a> {
    PersonalAccessToken(&'a str),
    AgentIdentityJwt(&'a str),
}

pub(super) fn classify_dex_access_token(access_token: &str) -> DexAccessToken<'_> {
    if access_token.starts_with(PERSONAL_ACCESS_TOKEN_PREFIX) {
        DexAccessToken::PersonalAccessToken(access_token)
    } else {
        DexAccessToken::AgentIdentityJwt(access_token)
    }
}

#[cfg(test)]
#[path = "access_token_tests.rs"]
mod tests;
