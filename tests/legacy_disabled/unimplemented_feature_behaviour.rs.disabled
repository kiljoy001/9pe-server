//! Targeted checks for currently unimplemented subsystems.
//! These tests exercise the code paths that still return placeholder
//! responses so we have a clear picture of what remains to be finished.

use std::sync::Arc;

use tempfile::TempDir;

use ninep_server::protocol::NinePeeMessage;
use ninep_server::server::handler::{PublicConnectionState, PublicNinePeeExtensionsHandler};
use ninep_server::settrans::VirtualSettransSystem;
use ninep_server::synth::SyntheticFilesystem;
use ninep_server::wasm::ThreadSafeTranslatorRegistry;

struct ExtensionsHarness {
    handler: PublicNinePeeExtensionsHandler,
    _temp: TempDir,
}

async fn build_extensions_harness() -> ExtensionsHarness {
    let temp_dir = TempDir::new().expect("temp dir");
    let registry = Arc::new(ThreadSafeTranslatorRegistry::new(
        temp_dir.path().to_path_buf(),
    ));
    let synth_fs = Arc::new(SyntheticFilesystem::new());
    let settrans = Arc::new(
        VirtualSettransSystem::new(synth_fs.clone(), registry.clone())
            .await
            .expect("virtual settrans"),
    );
    let connection_state = PublicConnectionState::new();
    let handler =
        PublicNinePeeExtensionsHandler::new(registry, settrans, synth_fs, connection_state);

    ExtensionsHarness {
        handler,
        _temp: temp_dir,
    }
}

#[tokio::test]
async fn wasm_invoke_without_translator_returns_enoent() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_wasm_invoke(
            "/srv/nonexistent".to_string(),
            "invoke".to_string(),
            vec![1, 2, 3],
        )
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 2, "expected ENOENT (2)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}

#[tokio::test]
async fn compute_invoke_is_currently_unimplemented() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_compute_invoke("kernel".into(), vec![42; 8])
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 38, "expected ENOSYS (38)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}

#[tokio::test]
async fn consensus_request_handler_is_placeholder() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_consensus_request(vec![0u8; 16])
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 38, "expected ENOSYS (38)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}

#[tokio::test]
async fn mesh_connect_handler_reports_not_supported() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_mesh_connect("peer-1".into(), "127.0.0.1:9650".into())
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 38, "expected ENOSYS (38)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}

#[tokio::test]
async fn work_submit_handler_reports_not_supported() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_work_submit("task-123".into(), vec![1, 2, 3])
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 38, "expected ENOSYS (38)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}

#[tokio::test]
async fn work_result_handler_reports_not_supported() {
    let harness = build_extensions_harness().await;

    let result = harness
        .handler
        .handle_work_result("task-123".into(), vec![9, 9, 9])
        .await
        .expect("handler result");

    match result {
        NinePeeMessage::Error { errno, .. } => assert_eq!(errno, 38, "expected ENOSYS (38)"),
        other => panic!("expected NinePeeMessage::Error, got {other:?}"),
    }
}
