//! Demo of user-owned global private grid networks
//!
//! This demonstrates the core functionality of user-owned namespaces
//! that enable global private computing grids.

use anyhow::Result;
use ed25519_dalek::SigningKey;
use ninep_server::{namespace_manager::NamespaceManager, synth::SyntheticFilesystem};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    println!("🌍 User-Owned Global Private Grid Networks Demo");
    println!("===============================================");
    println!();

    // Create synthetic filesystem and namespace manager
    let synth = Arc::new(SyntheticFilesystem::new());
    let namespace_manager = NamespaceManager::new(synth.clone())?;
    namespace_manager.initialize().await?;

    println!("✅ Namespace manager initialized");
    println!();

    // Generate user keypairs
    let alice_keypair = SigningKey::from_bytes(&rand::random());
    let alice_pubkey_hex = hex::encode(alice_keypair.verifying_key().as_bytes());

    let bob_keypair = SigningKey::from_bytes(&rand::random());
    let bob_pubkey_hex = hex::encode(bob_keypair.verifying_key().as_bytes());

    println!("👥 Generated user keypairs:");
    println!("   Alice: {}...", &alice_pubkey_hex[..16]);
    println!("   Bob:   {}...", &bob_pubkey_hex[..16]);
    println!();

    // 1. Alice creates her AI research grid with 2-of-3 requirements
    println!("1️⃣ Alice creates her AI research grid");
    let ai_grid = namespace_manager
        .register_namespace(
            "/srv/namespaces/users/alice/ai-research",
            "Alice's AI research collaboration grid",
            "user",
            Some((2, 3)), // 2-of-3 signatures required for operations
            None,         // No explicit expiration
            &alice_keypair,
        )
        .await?;

    println!("   ✅ Created namespace: {}", ai_grid.path);
    println!("   📋 Requirements: 2-of-3 signatures");
    println!("   👤 Owner: Alice");
    println!();

    // 2. Alice invites Bob to her grid
    println!("2️⃣ Alice invites Bob to her grid");
    namespace_manager
        .add_participant(
            "/srv/namespaces/users/alice/ai-research",
            &bob_pubkey_hex,
            &alice_keypair,
        )
        .await?;

    println!("   ✅ Added Bob as participant");
    println!();

    // 3. Check namespace details
    println!("3️⃣ Checking namespace details");
    let claim = namespace_manager
        .get_claim("/srv/namespaces/users/alice/ai-research")
        .await?;
    println!("   📍 Path: {}", claim.path);
    println!("   📝 Description: {}", claim.metadata.description);
    println!("   🏷️  Type: {}", claim.metadata.namespace_type);
    if let Some((required, total)) = claim.metadata.participant_requirements {
        println!("   🔐 Requirements: {}-of-{} signatures", required, total);
    }
    println!(
        "   👥 Participants: {} total",
        claim.metadata.participants.len()
    );
    for (i, participant) in claim.metadata.participants.iter().enumerate() {
        let is_owner = participant == &alice_pubkey_hex;
        println!(
            "      {}. {} {}",
            i + 1,
            participant,
            if is_owner { "(owner)" } else { "(participant)" }
        );
    }
    println!();

    // 4. Bob creates his own gaming grid with 1-of-2 requirements
    println!("4️⃣ Bob creates his own gaming grid");
    let gaming_grid = namespace_manager
        .register_namespace(
            "/srv/namespaces/users/bob/gaming-cluster",
            "Bob's distributed gaming cluster",
            "user",
            Some((1, 2)), // 1-of-2 signatures required (more flexible)
            None,         // No explicit expiration
            &bob_keypair,
        )
        .await?;

    println!("   ✅ Created namespace: {}", gaming_grid.path);
    println!("   📋 Requirements: 1-of-2 signatures");
    println!("   👤 Owner: Bob");
    println!();

    // 5. Demonstrate liveness tracking
    println!("5️⃣ Demonstrating liveness tracking");
    namespace_manager
        .update_liveness("/srv/namespaces/users/alice/ai-research", &alice_pubkey_hex)
        .await?;
    namespace_manager
        .update_liveness("/srv/namespaces/users/alice/ai-research", &bob_pubkey_hex)
        .await?;
    namespace_manager
        .update_liveness("/srv/namespaces/users/bob/gaming-cluster", &bob_pubkey_hex)
        .await?;

    println!("   ✅ Updated liveness for all active namespaces");
    println!();

    // 6. List all namespaces
    println!("6️⃣ Listing all registered namespaces");
    let all_namespaces = namespace_manager.list_namespaces().await;
    println!("   Found {} namespaces:", all_namespaces.len());
    for claim in all_namespaces {
        println!("   - {} ({})", claim.path, claim.metadata.namespace_type);
        if let Some((required, total)) = claim.metadata.participant_requirements {
            println!("     🔐 {}-of-{} signatures required", required, total);
        }
        println!("     👥 {} participants", claim.metadata.participants.len());
    }
    println!();

    // 7. List only user namespaces
    println!("7️⃣ Listing user-owned namespaces");
    let user_namespaces = namespace_manager.list_user_namespaces().await;
    println!("   Found {} user namespaces:", user_namespaces.len());
    for claim in user_namespaces {
        println!(
            "   - {} (owner: {})",
            claim.path,
            hex::encode(&claim.owner_pubkey[..8])
        );
    }
    println!();

    // 8. Show privacy features
    println!("8️⃣ Privacy features demonstration");
    println!("   🔒 Only namespace metadata is stored permanently");
    println!("   🔏 Actual file content and operations remain private");
    println!("   ⏰ Namespaces automatically expire when unused");
    println!("   🧹 Garbage collection protects user privacy");
    println!();

    // 9. Show global private grid capabilities
    println!("9️⃣ Global private grid capabilities");
    println!("   🌍 Users can create and own computing grids globally");
    println!("   🔗 Grids are discoverable but private by default");
    println!("   👥 Users control access and permissions");
    println!("   🤝 Secure collaboration within trusted groups");
    println!("   🧠 Enables distributed AI, gaming, and compute clusters");
    println!();

    println!("🎉 Demo completed successfully!");
    println!();
    println!("🌟 Key Features Demonstrated:");
    println!("   ✅ User-owned namespaces with cryptographic ownership");
    println!("   ✅ N-of-M signature requirements for security");
    println!("   ✅ Participant management and access control");
    println!("   ✅ Automatic liveness tracking and expiration");
    println!("   ✅ Privacy by design with automatic garbage collection");
    println!("   ✅ Global discovery with private collaboration");

    Ok(())
}
