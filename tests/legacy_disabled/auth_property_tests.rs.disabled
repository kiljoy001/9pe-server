//! Property-based tests for authentication system

use chrono::Duration;
use ninep_server::auth::{AuthConfig, AuthService, Capability, MockClock, SessionToken};
use proptest::prelude::*;
use std::path::PathBuf;
use tempfile::tempdir;

/// Generate arbitrary usernames
fn arbitrary_username() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{2,15}".prop_map(|s| s.to_string())
}

/// Generate arbitrary passwords
fn arbitrary_password() -> impl Strategy<Value = String> {
    "[A-Za-z0-9!@#$%^&*()_+=\\-\\[\\]{}|;:,.<>?/]{8,32}".prop_map(|s| s.to_string())
}

/// Generate arbitrary capabilities
fn arbitrary_capabilities() -> impl Strategy<Value = Vec<Capability>> {
    prop::collection::vec(
        prop_oneof![
            Just(Capability::Read),
            Just(Capability::Write),
            Just(Capability::Execute),
            Just(Capability::Mount),
            Just(Capability::Admin),
            Just(Capability::CreateTranslator),
            Just(Capability::MeshAccess),
            "[A-Za-z]+".prop_map(Capability::Custom),
        ],
        0..10,
    )
}

proptest! {
    /// Property: Creating a user and authenticating with correct password should succeed
    #[test]
    fn prop_auth_correct_password_succeeds(
        username in arbitrary_username(),
        password in arbitrary_password(),
        uid in 1000u32..10000u32,
        gid in 1000u32..10000u32,
        capabilities in arbitrary_capabilities(),
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create user
            auth.create_user(&username, &password, uid, gid, capabilities.clone()).await.unwrap();

            // Authenticate with correct password should succeed
            let token = auth.authenticate(&username, &password, None).await;
            prop_assert!(token.is_ok(), "Authentication with correct password should succeed");

            // Validate session
            let session = auth.validate_session(&token.unwrap()).await;
            prop_assert!(session.is_ok(), "Session should be valid");

            let session = session.unwrap();
            prop_assert_eq!(session.username, username);
            prop_assert_eq!(session.uid, uid);
            prop_assert_eq!(session.gid, gid);
            prop_assert_eq!(session.capabilities, capabilities);

            Ok(())
        })?
    }

    /// Property: Authenticating with wrong password should fail
    #[test]
    fn prop_auth_wrong_password_fails(
        username in arbitrary_username(),
        correct_password in arbitrary_password(),
        wrong_password in arbitrary_password(),
        uid in 1000u32..10000u32,
        gid in 1000u32..10000u32,
    ) {
        prop_assume!(correct_password != wrong_password);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create user with correct password
            auth.create_user(&username, &correct_password, uid, gid, vec![]).await.unwrap();

            // Authenticate with wrong password should fail
            let token = auth.authenticate(&username, &wrong_password, None).await;
            prop_assert!(token.is_err(), "Authentication with wrong password should fail");

            Ok(())
        })?
    }

    /// Property: Account lockout after max failed attempts
    #[test]
    fn prop_account_lockout_after_failures(
        username in arbitrary_username(),
        password in arbitrary_password(),
        wrong_passwords in prop::collection::vec(arbitrary_password(), 5..10),
        max_attempts in 3u32..5u32,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                max_failed_attempts: max_attempts,
                lockout_duration: 60, // 1 minute
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create user
            auth.create_user(&username, &password, 1000, 1000, vec![]).await.unwrap();

            // Try wrong passwords up to max_attempts
            let mut failed_count = 0;
            for wrong_pass in wrong_passwords.iter().take(max_attempts as usize) {
                if wrong_pass != &password {
                    let result = auth.authenticate(&username, wrong_pass, None).await;
                    if result.is_err() {
                        failed_count += 1;
                    }
                }
            }

            // After max_attempts failures, account should be locked
            if failed_count >= max_attempts {
                let result = auth.authenticate(&username, &password, None).await;
                prop_assert!(result.is_err(), "Account should be locked after {} failed attempts", max_attempts);

                // Error message should indicate account is locked
                if let Err(e) = result {
                    let error_msg = e.to_string();
                    prop_assert!(
                        error_msg.contains("locked") || error_msg.contains("Invalid"),
                        "Error should indicate account is locked: {}",
                        error_msg
                    );
                }
            }

            Ok(())
        })?
    }

    /// Property: Session expiration is enforced
    #[test]
    fn prop_session_expires(
        username in arbitrary_username(),
        password in arbitrary_password(),
        timeout_secs in 1u64..10u64,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                session_timeout: timeout_secs,
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock.clone()).await.unwrap();

            // Create user and authenticate
            auth.create_user(&username, &password, 1000, 1000, vec![]).await.unwrap();
            let token = auth.authenticate(&username, &password, None).await.unwrap();

            // Session should be valid immediately
            let session = auth.validate_session(&token).await;
            prop_assert!(session.is_ok(), "Session should be valid immediately after creation");

            // Advance clock past timeout
            clock.advance(Duration::seconds((timeout_secs + 1) as i64)).await;

            // Session should now be expired
            let session = auth.validate_session(&token).await;
            prop_assert!(session.is_err(), "Session should be expired after timeout");

            Ok(())
        })?
    }

    /// Property: Admin capability grants all permissions
    #[test]
    fn prop_admin_has_all_capabilities(
        username in arbitrary_username(),
        password in arbitrary_password(),
        test_capabilities in arbitrary_capabilities(),
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create admin user
            auth.create_user(&username, &password, 1000, 1000, vec![Capability::Admin]).await.unwrap();
            let token = auth.authenticate(&username, &password, None).await.unwrap();

            // Admin should have all capabilities
            for capability in &test_capabilities {
                let has_cap = auth.has_capability(&token, capability).await.unwrap();
                prop_assert!(has_cap, "Admin should have capability: {:?}", capability);
            }

            Ok(())
        })?
    }

    /// Property: Password change invalidates old password
    #[test]
    fn prop_password_change_invalidates_old(
        username in arbitrary_username(),
        old_password in arbitrary_password(),
        new_password in arbitrary_password(),
    ) {
        prop_assume!(old_password != new_password);

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create user with old password
            auth.create_user(&username, &old_password, 1000, 1000, vec![]).await.unwrap();

            // Change password
            auth.change_password(&username, &old_password, &new_password).await.unwrap();

            // Old password should no longer work
            let result = auth.authenticate(&username, &old_password, None).await;
            prop_assert!(result.is_err(), "Old password should no longer work after change");

            // New password should work
            let result = auth.authenticate(&username, &new_password, None).await;
            prop_assert!(result.is_ok(), "New password should work after change");

            Ok(())
        })?
    }

    /// Property: Duplicate users cannot be created
    #[test]
    fn prop_duplicate_users_rejected(
        username in arbitrary_username(),
        password1 in arbitrary_password(),
        password2 in arbitrary_password(),
        uid1 in 1000u32..5000u32,
        uid2 in 5000u32..10000u32,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock).await.unwrap();

            // Create first user
            auth.create_user(&username, &password1, uid1, 1000, vec![]).await.unwrap();

            // Attempt to create duplicate user should fail
            let result = auth.create_user(&username, &password2, uid2, 1000, vec![]).await;
            prop_assert!(result.is_err(), "Creating duplicate user should fail");

            Ok(())
        })?
    }

    /// Property: Session cleanup removes expired sessions
    #[test]
    fn prop_session_cleanup_removes_expired(
        users in prop::collection::vec(
            (arbitrary_username(), arbitrary_password()),
            1..5
        ),
        timeout_secs in 60u64..300u64,
    ) {
        // Ensure unique usernames
        let mut seen = std::collections::HashSet::new();
        for (username, _) in &users {
            if !seen.insert(username.clone()) {
                return Ok(()); // Skip if duplicate username
            }
        }

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let dir = tempdir().unwrap();
            let config = AuthConfig {
                db_path: dir.path().join("test.db"),
                session_timeout: timeout_secs,
                ..Default::default()
            };

            let clock = MockClock::new();
            let auth = AuthService::new_with_clock(config, clock.clone()).await.unwrap();

            // Create users and sessions
            let mut tokens = Vec::new();
            for (i, (username, password)) in users.iter().enumerate() {
                auth.create_user(username, password, 1000 + i as u32, 1000, vec![]).await.unwrap();
                let token = auth.authenticate(username, password, None).await.unwrap();
                tokens.push(token);
            }

            // All sessions should be valid initially
            for token in &tokens {
                let session = auth.validate_session(token).await;
                prop_assert!(session.is_ok(), "Session should be valid initially");
            }

            // Advance clock past expiration
            clock.advance(Duration::seconds((timeout_secs + 1) as i64)).await;

            // Clean up sessions
            auth.cleanup_sessions().await.unwrap();

            // All sessions should now be invalid
            for token in &tokens {
                let session = auth.validate_session(token).await;
                prop_assert!(session.is_err(), "Session should be invalid after cleanup");
            }

            Ok(())
        })?
    }
}
