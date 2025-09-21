//! Web UI Server for 9P.e
//!
//! Provides a modern web interface for browsing and managing the 9P.e filesystem

use std::path::PathBuf;
use std::net::SocketAddr;
use std::sync::Arc;
use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use axum::{
    Router,
    routing::{get, post},
    response::{Html, Json},
    extract::{State, Path as AxumPath, Query},
    http::StatusCode,
};
use serde::{Serialize, Deserialize};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

/// Web UI configuration
#[derive(Clone)]
pub struct WebConfig {
    pub root_path: PathBuf,
    pub bind_addr: SocketAddr,
}

/// File/directory information for JSON API
#[derive(Serialize, Deserialize)]
struct FileInfo {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: u64,
}

/// Directory listing response
#[derive(Serialize)]
struct DirectoryListing {
    path: String,
    entries: Vec<FileInfo>,
}

/// Start the web UI server
pub async fn start_web_ui(config: WebConfig) -> Result<()> {
    info!("🌐 Starting Web UI on http://{}", config.bind_addr);

    // Create shared state
    let state = Arc::new(AppState {
        root_path: config.root_path,
    });

    // Build the router
    let app = Router::new()
        // Main UI
        .route("/", get(serve_index))
        .route("/browse/*path", get(browse_directory))

        // API endpoints
        .route("/api/list/*path", get(api_list_directory))
        .route("/api/file/*path", get(api_get_file))
        .route("/api/upload/*path", post(api_upload_file))
        .route("/api/mkdir/*path", post(api_create_directory))
        .route("/api/delete/*path", post(api_delete_path))

        // Static assets (if we add CSS/JS files later)
        .nest_service("/static", ServeDir::new("static"))

        // Add CORS for API access
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Start the server
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    info!("✅ Web UI listening on http://{}", config.bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Application state
#[derive(Clone)]
struct AppState {
    root_path: PathBuf,
}

/// Serve the main index page
async fn serve_index() -> Html<String> {
    Html(format!(r#"
<!DOCTYPE html>
<html>
<head>
    <title>9P.e File Browser</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 0;
            padding: 20px;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
            background: white;
            border-radius: 12px;
            box-shadow: 0 20px 60px rgba(0,0,0,0.3);
            overflow: hidden;
        }}
        .header {{
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            padding: 30px;
        }}
        .header h1 {{
            margin: 0;
            font-size: 2em;
        }}
        .header p {{
            margin: 10px 0 0 0;
            opacity: 0.9;
        }}
        .content {{
            padding: 30px;
        }}
        .file-browser {{
            background: #f7f7f7;
            border-radius: 8px;
            padding: 20px;
        }}
        .path-bar {{
            background: white;
            padding: 15px;
            border-radius: 6px;
            margin-bottom: 20px;
            font-family: monospace;
            display: flex;
            align-items: center;
            gap: 10px;
        }}
        .file-list {{
            background: white;
            border-radius: 6px;
            overflow: hidden;
        }}
        .file-item {{
            padding: 15px 20px;
            border-bottom: 1px solid #eee;
            cursor: pointer;
            display: flex;
            align-items: center;
            gap: 15px;
            transition: background 0.2s;
        }}
        .file-item:hover {{
            background: #f0f0f0;
        }}
        .file-icon {{
            font-size: 1.5em;
        }}
        .file-name {{
            flex: 1;
            font-weight: 500;
        }}
        .file-size {{
            color: #666;
            font-size: 0.9em;
        }}
        .toolbar {{
            display: flex;
            gap: 10px;
            margin-bottom: 20px;
        }}
        .btn {{
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: white;
            border: none;
            padding: 10px 20px;
            border-radius: 6px;
            cursor: pointer;
            font-size: 14px;
            transition: transform 0.2s;
        }}
        .btn:hover {{
            transform: translateY(-2px);
        }}
        .upload-area {{
            border: 2px dashed #667eea;
            border-radius: 8px;
            padding: 40px;
            text-align: center;
            margin: 20px 0;
            transition: background 0.2s;
        }}
        .upload-area.dragover {{
            background: #f0f0ff;
        }}
        .stats {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-top: 30px;
        }}
        .stat-card {{
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }}
        .stat-value {{
            font-size: 2em;
            font-weight: bold;
            color: #667eea;
        }}
        .stat-label {{
            color: #666;
            margin-top: 5px;
        }}
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <h1>🚀 9P.e File Browser</h1>
            <p>Modern filesystem protocol with QUIC transport</p>
        </div>

        <div class="content">
            <div class="toolbar">
                <button class="btn" onclick="createDirectory()">📁 New Folder</button>
                <button class="btn" onclick="document.getElementById('upload').click()">📤 Upload</button>
                <input type="file" id="upload" style="display: none" multiple onchange="uploadFiles(this.files)">
                <button class="btn" onclick="refreshView()">🔄 Refresh</button>
            </div>

            <div class="file-browser">
                <div class="path-bar">
                    <span>📁</span>
                    <span id="current-path">/</span>
                </div>

                <div class="upload-area" id="dropzone">
                    <p>📤 Drag and drop files here or click Upload button</p>
                </div>

                <div class="file-list" id="file-list">
                    <div class="file-item">
                        <span class="file-icon">📄</span>
                        <span class="file-name">Loading...</span>
                    </div>
                </div>
            </div>

            <div class="stats">
                <div class="stat-card">
                    <div class="stat-value" id="total-files">0</div>
                    <div class="stat-label">Total Files</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value" id="total-dirs">0</div>
                    <div class="stat-label">Directories</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value" id="total-size">0 B</div>
                    <div class="stat-label">Total Size</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">✅</div>
                    <div class="stat-label">Server Status</div>
                </div>
            </div>
        </div>
    </div>

    <script>
        let currentPath = '/';

        async function loadDirectory(path) {{
            currentPath = path;
            document.getElementById('current-path').textContent = path || '/';

            try {{
                const response = await fetch('/api/list' + path);
                const data = await response.json();

                displayFiles(data.entries);
                updateStats(data.entries);
            }} catch (error) {{
                console.error('Failed to load directory:', error);
            }}
        }}

        function displayFiles(entries) {{
            const list = document.getElementById('file-list');
            list.innerHTML = '';

            // Add parent directory link if not at root
            if (currentPath !== '/') {{
                const parent = document.createElement('div');
                parent.className = 'file-item';
                parent.innerHTML = `
                    <span class="file-icon">⬆️</span>
                    <span class="file-name">..</span>
                `;
                parent.onclick = () => {{
                    const parentPath = currentPath.split('/').slice(0, -1).join('/') || '/';
                    loadDirectory(parentPath);
                }};
                list.appendChild(parent);
            }}

            // Add entries
            entries.forEach(entry => {{
                const item = document.createElement('div');
                item.className = 'file-item';
                item.innerHTML = `
                    <span class="file-icon">${{entry.is_dir ? '📁' : '📄'}}</span>
                    <span class="file-name">${{entry.name}}</span>
                    <span class="file-size">${{formatSize(entry.size)}}</span>
                `;

                item.onclick = () => {{
                    if (entry.is_dir) {{
                        loadDirectory(entry.path);
                    }} else {{
                        downloadFile(entry.path);
                    }}
                }};

                list.appendChild(item);
            }});
        }}

        function updateStats(entries) {{
            const files = entries.filter(e => !e.is_dir);
            const dirs = entries.filter(e => e.is_dir);
            const totalSize = files.reduce((sum, f) => sum + f.size, 0);

            document.getElementById('total-files').textContent = files.length;
            document.getElementById('total-dirs').textContent = dirs.length;
            document.getElementById('total-size').textContent = formatSize(totalSize);
        }}

        function formatSize(bytes) {{
            if (bytes === 0) return '0 B';
            const k = 1024;
            const sizes = ['B', 'KB', 'MB', 'GB'];
            const i = Math.floor(Math.log(bytes) / Math.log(k));
            return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
        }}

        async function createDirectory() {{
            const name = prompt('Enter directory name:');
            if (!name) return;

            try {{
                await fetch('/api/mkdir' + currentPath + '/' + name, {{
                    method: 'POST'
                }});
                loadDirectory(currentPath);
            }} catch (error) {{
                alert('Failed to create directory');
            }}
        }}

        async function uploadFiles(files) {{
            for (const file of files) {{
                const formData = new FormData();
                formData.append('file', file);

                try {{
                    await fetch('/api/upload' + currentPath + '/' + file.name, {{
                        method: 'POST',
                        body: formData
                    }});
                }} catch (error) {{
                    console.error('Upload failed:', file.name);
                }}
            }}
            loadDirectory(currentPath);
        }}

        function downloadFile(path) {{
            window.location.href = '/api/file' + path;
        }}

        function refreshView() {{
            loadDirectory(currentPath);
        }}

        // Drag and drop
        const dropzone = document.getElementById('dropzone');

        dropzone.addEventListener('dragover', (e) => {{
            e.preventDefault();
            dropzone.classList.add('dragover');
        }});

        dropzone.addEventListener('dragleave', () => {{
            dropzone.classList.remove('dragover');
        }});

        dropzone.addEventListener('drop', (e) => {{
            e.preventDefault();
            dropzone.classList.remove('dragover');
            uploadFiles(e.dataTransfer.files);
        }});

        // Initial load
        loadDirectory('/');
    </script>
</body>
</html>
"#))
}

/// Browse directory endpoint
async fn browse_directory(
    AxumPath(path): AxumPath<String>,
    State(_state): State<Arc<AppState>>,
) -> Html<String> {
    // Redirect to main UI - the JS will handle the path
    Html(format!(r#"
        <script>
            window.location.href = '/?path={}';
        </script>
    "#, path))
}

/// API: List directory contents
async fn api_list_directory(
    AxumPath(path): AxumPath<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<DirectoryListing>, StatusCode> {
    let full_path = state.root_path.join(&path);

    // Security: ensure path is within root
    if !full_path.starts_with(&state.root_path) {
        return Err(StatusCode::FORBIDDEN);
    }

    let mut entries = Vec::new();

    match tokio::fs::read_dir(&full_path).await {
        Ok(mut dir) => {
            while let Ok(Some(entry)) = dir.next_entry().await {
                if let Ok(metadata) = entry.metadata().await {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let relative_path = format!("{}/{}", path.trim_end_matches('/'), name);

                    entries.push(FileInfo {
                        name,
                        path: relative_path,
                        is_dir: metadata.is_dir(),
                        size: metadata.len(),
                        modified: metadata.modified()
                            .map(|t| t.duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs())
                            .unwrap_or(0),
                    });
                }
            }
        }
        Err(_) => return Err(StatusCode::NOT_FOUND),
    }

    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        }
    });

    Ok(Json(DirectoryListing {
        path: path.clone(),
        entries,
    }))
}

/// API: Get file contents
async fn api_get_file(
    AxumPath(path): AxumPath<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Vec<u8>, StatusCode> {
    let full_path = state.root_path.join(&path);

    // Security: ensure path is within root
    if !full_path.starts_with(&state.root_path) {
        return Err(StatusCode::FORBIDDEN);
    }

    tokio::fs::read(&full_path).await
        .map_err(|_| StatusCode::NOT_FOUND)
}

/// API: Upload file
async fn api_upload_file(
    AxumPath(path): AxumPath<String>,
    State(state): State<Arc<AppState>>,
    body: axum::body::Bytes,
) -> StatusCode {
    let full_path = state.root_path.join(&path);

    // Security: ensure path is within root
    if !full_path.starts_with(&state.root_path) {
        return StatusCode::FORBIDDEN;
    }

    match tokio::fs::write(&full_path, &body).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// API: Create directory
async fn api_create_directory(
    AxumPath(path): AxumPath<String>,
    State(state): State<Arc<AppState>>,
) -> StatusCode {
    let full_path = state.root_path.join(&path);

    // Security: ensure path is within root
    if !full_path.starts_with(&state.root_path) {
        return StatusCode::FORBIDDEN;
    }

    match tokio::fs::create_dir(&full_path).await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// API: Delete path
async fn api_delete_path(
    AxumPath(path): AxumPath<String>,
    State(state): State<Arc<AppState>>,
) -> StatusCode {
    let full_path = state.root_path.join(&path);

    // Security: ensure path is within root
    if !full_path.starts_with(&state.root_path) {
        return StatusCode::FORBIDDEN;
    }

    let metadata = match tokio::fs::metadata(&full_path).await {
        Ok(m) => m,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let result = if metadata.is_dir() {
        tokio::fs::remove_dir_all(&full_path).await
    } else {
        tokio::fs::remove_file(&full_path).await
    };

    match result {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}