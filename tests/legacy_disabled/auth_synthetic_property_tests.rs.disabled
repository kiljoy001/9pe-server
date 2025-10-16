use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result as AnyhowResult};
use ninep_server::auth::{
    register_auth_controls, AuthConfig, AuthService, Capability, SessionToken,
};
use ninep_server::synth::SyntheticFilesystem;
use proptest::prelude::ProptestConfig;
use proptest::prelude::*;
use tempfile::{tempdir, TempDir};
use tokio::runtime::Runtime;

struct AuthTestHarness {
    runtime: Runtime,
    auth: Arc<AuthService>,
    fs: Arc<SyntheticFilesystem>,
    _tempdir: TempDir,
}

impl AuthTestHarness {
    fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime build");

        let tempdir = tempdir().expect("tempdir");
        let db_path = tempdir.path().join("users.db");

        let (auth, fs) = runtime.block_on(async {
            let config = AuthConfig {
                db_path,
                ..Default::default()
            };
            let auth = Arc::new(AuthService::new(config).await.expect("auth init"));
            let fs = Arc::new(SyntheticFilesystem::new());
            register_auth_controls(&fs, auth.clone())
                .await
                .expect("register controls");
            (auth, fs)
        });

        Self {
            runtime,
            auth,
            fs,
            _tempdir: tempdir,
        }
    }

    fn write_control(&self, path: &str, data: impl Into<Vec<u8>>) -> AnyhowResult<()> {
        let bytes = data.into();
        self.runtime
            .block_on(self.fs.write_file(Path::new(path), bytes))
            .with_context(|| format!("write failed for control {}", path))
    }

    fn read_control(&self, path: &str) -> AnyhowResult<Vec<u8>> {
        self.runtime
            .block_on(self.fs.read_file(Path::new(path)))
            .with_context(|| format!("read failed for control {}", path))
    }

    fn create_user(
        &self,
        username: &str,
        password: &str,
        uid: u32,
        gid: u32,
        capabilities: Option<&str>,
    ) -> AnyhowResult<()> {
        let mut payload = format!("{username} {password} {uid} {gid}");
        if let Some(caps) = capabilities {
            if !caps.is_empty() {
                payload.push(' ');
                payload.push_str(caps);
            }
        }
        self.write_control("/srv/auth/create", payload)
    }

    fn login(&self, username: &str, password: &str) -> AnyhowResult<SessionToken> {
        self.write_control("/srv/auth/login", format!("{username} {password}"))?;
        let token_bytes = self.read_control("/srv/auth/login")?;
        let token_str =
            String::from_utf8(token_bytes).context("login control emitted non UTF-8 response")?;
        let token_str = token_str.trim();
        if token_str.is_empty() {
            anyhow::bail!("login control returned empty token");
        }
        Ok(SessionToken::from_string(token_str))
    }

    fn logout(&self, token: &SessionToken) -> AnyhowResult<()> {
        self.write_control("/srv/auth/logout", format!("{}\n", token.as_str()))
    }

    fn delete_user(&self, username: &str) -> AnyhowResult<()> {
        self.write_control("/srv/auth/delete", format!("{username}\n"))
    }

    fn list_users(&self) -> AnyhowResult<Vec<String>> {
        let bytes = self.read_control("/srv/auth/users")?;
        let body = String::from_utf8(bytes).context("users control emitted non UTF-8")?;
        Ok(body
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.trim().to_string())
            .collect())
    }

    fn validate_session(&self, token: &SessionToken) -> AnyhowResult<()> {
        self.runtime
            .block_on(self.auth.validate_session(token))
            .map(|_| ())
            .context("session validation failed")
    }

    fn has_capability(&self, token: &SessionToken, capability: &Capability) -> AnyhowResult<bool> {
        self.runtime
            .block_on(self.auth.has_capability(token, capability))
            .context("capability check failed")
    }
}

fn error_chain_contains(err: &anyhow::Error, needle: &str) -> bool {
    err.chain().any(|cause| cause.to_string().contains(needle))
}

#[derive(Clone, Debug)]
struct CapabilityCase {
    token: String,
    expected: Capability,
}

#[derive(Clone, Debug)]
struct CapabilityInput {
    cases: Vec<CapabilityCase>,
    separators: Vec<Separator>,
}

#[derive(Clone, Debug)]
enum Separator {
    Space,
    Comma,
    CommaSpace,
}

fn randomize_case(token: &str, mask: &[bool]) -> String {
    token
        .chars()
        .zip(mask.iter().copied())
        .map(|(ch, upper)| {
            if upper {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            }
        })
        .collect()
}

fn username_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[a-z][a-z0-9]{2,10}").unwrap()
}

fn password_strategy() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[A-Za-z0-9]{8,18}").unwrap()
}

fn separator_strategy() -> impl Strategy<Value = Separator> {
    prop_oneof![
        Just(Separator::Space),
        Just(Separator::Comma),
        Just(Separator::CommaSpace),
    ]
}

fn capability_token_strategy() -> impl Strategy<Value = CapabilityCase> {
    prop_oneof![
        standard_token_case("read", Capability::Read),
        standard_token_case("write", Capability::Write),
        standard_token_case("execute", Capability::Execute),
        standard_token_case("exec", Capability::Execute),
        standard_token_case("mount", Capability::Mount),
        standard_token_case("admin", Capability::Admin),
        standard_token_case("translator", Capability::CreateTranslator),
        standard_token_case("create_translator", Capability::CreateTranslator),
        standard_token_case("mesh", Capability::MeshAccess),
        standard_token_case("mesh_access", Capability::MeshAccess),
        prefixed_custom_token_case(),
        plain_custom_token_case(),
    ]
}

fn standard_token_case(
    token: &'static str,
    capability: Capability,
) -> impl Strategy<Value = CapabilityCase> {
    let len = token.len();
    proptest::collection::vec(any::<bool>(), len).prop_map(move |mask| CapabilityCase {
        token: randomize_case(token, &mask),
        expected: capability.clone(),
    })
}

fn prefixed_custom_token_case() -> impl Strategy<Value = CapabilityCase> {
    let prefix_mask = proptest::collection::vec(any::<bool>(), "custom:".len());
    let label = proptest::string::string_regex("[A-Za-z0-9_]{1,12}").unwrap();

    (prefix_mask, label).prop_map(|(mask, label)| CapabilityCase {
        token: format!("{}{}", randomize_case("custom:", &mask), label),
        expected: Capability::Custom(label.to_lowercase()),
    })
}

fn plain_custom_token_case() -> impl Strategy<Value = CapabilityCase> {
    proptest::string::string_regex("[A-Za-z0-9]{3,12}")
        .unwrap()
        .prop_filter(
            "custom token must include a digit to avoid reserved names",
            |token| token.chars().any(|ch| ch.is_ascii_digit()),
        )
        .prop_map(|token| CapabilityCase {
            expected: Capability::Custom(token.to_lowercase()),
            token,
        })
}

fn capability_input_strategy() -> impl Strategy<Value = CapabilityInput> {
    prop::collection::vec(capability_token_strategy(), 1..5).prop_flat_map(|cases| {
        let separator_count = cases.len().saturating_sub(1);
        prop::collection::vec(separator_strategy(), separator_count).prop_map(move |separators| {
            CapabilityInput {
                cases: cases.clone(),
                separators,
            }
        })
    })
}

fn capability_section(input: &CapabilityInput) -> String {
    if input.cases.is_empty() {
        return String::new();
    }

    let mut buf = String::new();
    for (idx, case) in input.cases.iter().enumerate() {
        if idx > 0 {
            let sep = input
                .separators
                .get(idx - 1)
                .cloned()
                .unwrap_or(Separator::Space);
            match sep {
                Separator::Space => buf.push(' '),
                Separator::Comma => buf.push(','),
                Separator::CommaSpace => {
                    buf.push(',');
                    buf.push(' ');
                }
            }
        }

        buf.push_str(&case.token);
    }
    buf
}

fn unique_capabilities(mut capabilities: Vec<Capability>) -> Vec<Capability> {
    let mut unique: Vec<Capability> = Vec::new();

    for capability in capabilities.drain(..) {
        if !unique.iter().any(|existing| existing == &capability) {
            unique.push(capability);
        }
    }

    unique
}

fn run_capability_case(
    username: String,
    password: String,
    input: CapabilityInput,
) -> Result<(), TestCaseError> {
    let harness = AuthTestHarness::new();
    let capabilities = capability_section(&input);

    harness
        .create_user(
            &username,
            &password,
            1000,
            1000,
            if capabilities.is_empty() {
                None
            } else {
                Some(&capabilities)
            },
        )
        .map_err(|e| TestCaseError::fail(format!("create control write failed: {e}")))?;

    let users = harness
        .list_users()
        .map_err(|e| TestCaseError::fail(format!("list users failed: {e}")))?;
    if !users.contains(&username) {
        return Err(TestCaseError::fail("user not created via control"));
    }

    let session_token = harness
        .login(&username, &password)
        .map_err(|e| TestCaseError::fail(format!("login via control failed: {e}")))?;

    let expected_caps = unique_capabilities(
        input
            .cases
            .iter()
            .map(|case| case.expected.clone())
            .collect::<Vec<_>>(),
    );

    for capability in expected_caps {
        let has = harness
            .has_capability(&session_token, &capability)
            .map_err(|e| TestCaseError::fail(format!("capability check failed: {e}")))?;
        if !has {
            return Err(TestCaseError::fail(format!(
                "session missing capability {capability:?}"
            )));
        }
    }

    if !input
        .cases
        .iter()
        .any(|case| matches!(case.expected, Capability::Admin))
    {
        let negative = Capability::Custom("unexpected".to_string());
        let has_negative = harness
            .has_capability(&session_token, &negative)
            .map_err(|e| TestCaseError::fail(format!("negative check failed: {e}")))?;
        if has_negative {
            return Err(TestCaseError::fail(
                "session unexpectedly grants unrelated capability",
            ));
        }
    }

    Ok(())
}

#[test]
fn logout_control_revokes_sessions() {
    let harness = AuthTestHarness::new();
    harness
        .create_user("logout_user", "secret123", 101, 101, None)
        .expect("create user");

    let token = harness.login("logout_user", "secret123").expect("login");
    harness
        .validate_session(&token)
        .expect("session should be valid before logout");

    harness.logout(&token).expect("logout succeeds");
    assert!(
        harness.validate_session(&token).is_err(),
        "session should be invalid after logout"
    );

    let err = harness
        .write_control("/srv/auth/logout", "")
        .expect_err("empty token rejected");
    assert!(
        error_chain_contains(&err, "Token cannot be empty"),
        "expected empty token error, got {err:?}"
    );
}

#[test]
fn create_control_rejects_invalid_inputs() {
    let harness = AuthTestHarness::new();

    let err = harness
        .write_control("/srv/auth/create", "baduser pass not_a_number 100")
        .expect_err("non-numeric uid should fail");
    assert!(
        error_chain_contains(&err, "uid must be numeric"),
        "expected uid error, got {err:?}"
    );

    let err = harness
        .write_control("/srv/auth/create", "baduser pass 100 not_a_number")
        .expect_err("non-numeric gid should fail");
    assert!(
        error_chain_contains(&err, "gid must be numeric"),
        "expected gid error, got {err:?}"
    );

    let err = harness
        .write_control("/srv/auth/create", vec![0xff, 0xfe, 0xfd])
        .expect_err("non-utf8 payload should fail");
    assert!(
        error_chain_contains(&err, "Input must be UTF-8"),
        "expected utf8 error, got {err:?}"
    );

    let users = harness.list_users().expect("list users");
    assert!(
        !users.contains(&"baduser".to_string()),
        "invalid create attempts must not register users"
    );
}

#[test]
fn delete_control_clears_users_and_sessions() {
    let harness = AuthTestHarness::new();
    harness
        .create_user("todelete", "delete123", 303, 404, Some("write"))
        .expect("create user");

    let token = harness.login("todelete", "delete123").expect("login");
    harness
        .validate_session(&token)
        .expect("session should be valid before delete");

    harness.delete_user("todelete").expect("delete succeeds");

    let users = harness.list_users().expect("list users");
    assert!(
        !users.contains(&"todelete".to_string()),
        "delete control should remove user from listing"
    );

    assert!(
        harness.validate_session(&token).is_err(),
        "session should be invalidated after delete"
    );
}

#[test]
fn delete_control_protects_admin_account() {
    let harness = AuthTestHarness::new();
    let err = harness
        .delete_user("admin")
        .expect_err("admin delete must fail");
    assert!(
        error_chain_contains(&err, "Cannot delete admin user"),
        "expected admin protection message, got {err:?}"
    );
}

#[test]
fn list_users_control_returns_sorted_names() {
    let harness = AuthTestHarness::new();
    harness
        .create_user("zoe", "pass12345", 500, 500, None)
        .expect("create zoe");
    harness
        .create_user("amy", "pass12345", 501, 500, Some("write"))
        .expect("create amy");
    harness
        .create_user("mike", "pass12345", 502, 500, Some("execute"))
        .expect("create mike");

    let mut users = harness.list_users().expect("list users");
    assert!(
        users.contains(&"admin".to_string()),
        "admin should always be present"
    );
    users.retain(|name| name != "admin");

    let mut sorted = users.clone();
    sorted.sort();
    assert_eq!(users, sorted, "users listing should be sorted");
}

fn run_default_capability_case(username: String, password: String) -> Result<(), TestCaseError> {
    let harness = AuthTestHarness::new();
    harness
        .create_user(&username, &password, 4242, 4242, None)
        .map_err(|e| TestCaseError::fail(format!("create control write failed: {e}")))?;

    let token = harness
        .login(&username, &password)
        .map_err(|e| TestCaseError::fail(format!("login via control failed: {e}")))?;

    let has_read = harness
        .has_capability(&token, &Capability::Read)
        .map_err(|e| TestCaseError::fail(format!("capability check failed: {e}")))?;
    if !has_read {
        return Err(TestCaseError::fail(
            "session missing default read capability",
        ));
    }

    let negative = Capability::Custom("unexpected".to_string());
    let has_negative = harness
        .has_capability(&token, &negative)
        .map_err(|e| TestCaseError::fail(format!("negative check failed: {e}")))?;
    if has_negative {
        return Err(TestCaseError::fail(
            "session unexpectedly grants unrelated capability",
        ));
    }

    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(8))]
    #[test]
    fn create_control_parses_capabilities(
        username in username_strategy(),
        password in password_strategy(),
        input in capability_input_strategy(),
    ) {
        run_capability_case(username, password, input)?;
    }

    #[test]
    fn create_control_defaults_to_read_capability(
        username in username_strategy(),
        password in password_strategy(),
    ) {
        run_default_capability_case(username, password)?;
    }
}
