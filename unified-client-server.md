# 9pe-server: Unified Client-Server Architecture

## 🚀 **Revolutionary Concept: All-in-One 9P Solution**

Instead of separate tools like traditional 9P ecosystem, create **one app that does everything**:

```
┌─────────────────────────────────────────┐
│              9pe-server                 │
│         "The Complete 9P Suite"        │
├─────────────────────────────────────────┤
│  🖥️  GUI Dashboard (Tauri + Grafana)    │
│  🗂️  Built-in File Browser              │
│  📡  Server Mode (serve folders)        │
│  🔌  Client Mode (mount remote 9P)      │
│  🔄  Sync Mode (bidirectional sync)     │
│  🌐  Network Discovery (find 9P servers)│
│  📊  Monitoring (both local & remote)   │
└─────────────────────────────────────────┘
```

## **Current 9P Ecosystem vs Our Unified App**

| Function | Traditional | Our Unified App |
|----------|-------------|-----------------|
| **Serve folders** | `diod`, `u9fs` | ✅ Built-in server mode |
| **Mount remote** | `mount -t 9p` | ✅ Built-in client mode |
| **File browser** | Separate app | ✅ Integrated GUI browser |
| **Monitoring** | None | ✅ Grafana dashboard |
| **Configuration** | Text files | ✅ Visual config |
| **Network discovery** | Manual IPs | ✅ Auto-discovery |

## **Implementation Architecture**

### **Unified CLI Interface**
```bash
# Server mode (current functionality)
9pe-server serve --root /my/folder

# Client mode (mount remote 9P server)
9pe-server mount 192.168.1.10:564 /mnt/remote

# GUI mode (everything visual)
9pe-server gui

# Discovery mode (find 9P servers on network)
9pe-server discover

# Sync mode (bidirectional folder sync)
9pe-server sync /local/folder user@remote:564/remote/folder
```

### **Tauri GUI with All Modes**
```
┌─────────────────────────────────────────┐
│  📁 9pe-server - Complete 9P Suite      │
├─────────────────────────────────────────┤
│  [Server] [Client] [Browser] [Monitor]  │
├─────────────────────────────────────────┤
│                                         │
│  SERVER MODE:                           │
│  📁 Serving: /home/user/documents       │
│  🌐 Address: 192.168.1.100:564         │
│  👥 Connections: 3 active              │
│  [Stop Server] [Change Folder]         │
│                                         │
│  CLIENT MODE:                           │
│  🔗 Connected to: work-server:564       │
│  📂 Browse: /project/files              │
│  💾 Local mount: /mnt/work              │
│  [Disconnect] [Browse Files]           │
│                                         │
│  NETWORK DISCOVERY:                     │
│  🔍 Found servers:                      │
│  • 192.168.1.10:564 (home-nas)         │
│  • 192.168.1.20:564 (work-server)      │
│  [Connect] [Refresh]                   │
│                                         │
└─────────────────────────────────────────┘
```

## **Core Implementation**

### **Unified Core Library**
```rust
// src/core.rs - Shared functionality
pub enum Mode {
    Server(ServerConfig),
    Client(ClientConfig),
    Gui(GuiConfig),
    Discovery,
    Sync(SyncConfig),
}

pub struct NinePeeApp {
    mode: Mode,
    runtime: tokio::runtime::Runtime,
}

impl NinePeeApp {
    pub async fn start(&self) -> Result<()> {
        match &self.mode {
            Mode::Server(config) => self.start_server(config).await,
            Mode::Client(config) => self.start_client(config).await,
            Mode::Gui(config) => self.start_gui(config).await,
            Mode::Discovery => self.start_discovery().await,
            Mode::Sync(config) => self.start_sync(config).await,
        }
    }
}
```

### **Client Implementation**
```rust
// src/client.rs - 9P client functionality
use crate::transport::QuicClient;

pub struct NinePeeClient {
    connection: QuicClient,
    remote_addr: SocketAddr,
    mount_point: Option<PathBuf>,
}

impl NinePeeClient {
    pub async fn connect(addr: SocketAddr) -> Result<Self> {
        let client = QuicClient::new().await?;
        let connection = client.connect(addr, "9pe-server").await?;

        Ok(Self {
            connection,
            remote_addr: addr,
            mount_point: None,
        })
    }

    pub async fn mount(&mut self, local_path: PathBuf) -> Result<()> {
        // Create FUSE mount point
        self.mount_point = Some(local_path.clone());

        // Start FUSE filesystem
        tokio::spawn(async move {
            let fs = NinepeeFuse::new(connection);
            fuse::mount(fs, &local_path, &[]).await
        });

        Ok(())
    }

    pub async fn browse(&self, path: &str) -> Result<Vec<FileInfo>> {
        // Send 9P walk + stat messages
        let walk_msg = TwalkMessage {
            fid: 1,
            newfid: 2,
            wnames: path.split('/').map(|s| s.to_string()).collect(),
        };

        self.connection.send_message(&Message::Twalk(walk_msg)).await?;
        // ... process response
        Ok(vec![])
    }
}
```

### **Network Discovery**
```rust
// src/discovery.rs - Find 9P servers on network
use tokio::net::UdpSocket;

pub struct NetworkDiscovery {
    socket: UdpSocket,
    servers: Vec<DiscoveredServer>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredServer {
    pub address: SocketAddr,
    pub name: String,
    pub version: String,
    pub exports: Vec<String>,
}

impl NetworkDiscovery {
    pub async fn scan_network(&mut self) -> Result<Vec<DiscoveredServer>> {
        // Send broadcast/multicast discovery packets
        let discovery_msg = b"9PE_DISCOVERY_v1.0";

        // Broadcast on common 9P ports
        for port in [564, 9000, 9001] {
            let broadcast_addr = format!("255.255.255.255:{}", port);
            self.socket.send_to(discovery_msg, &broadcast_addr).await?;
        }

        // Listen for responses
        let mut buffer = [0u8; 1024];
        while let Ok((len, addr)) = self.socket.recv_from(&mut buffer).await {
            if let Ok(response) = String::from_utf8(buffer[..len].to_vec()) {
                if response.starts_with("9PE_SERVER") {
                    let server = self.parse_server_response(&response, addr)?;
                    self.servers.push(server);
                }
            }
        }

        Ok(self.servers.clone())
    }
}
```

### **Bidirectional Sync**
```rust
// src/sync.rs - rsync-like functionality over 9P
pub struct NinePeeSync {
    local_path: PathBuf,
    remote_client: NinePeeClient,
    remote_path: String,
}

impl NinePeeSync {
    pub async fn sync_bidirectional(&self) -> Result<SyncReport> {
        let local_files = self.scan_local_files().await?;
        let remote_files = self.scan_remote_files().await?;

        let changes = self.compute_changes(&local_files, &remote_files)?;

        for change in changes {
            match change {
                Change::Upload(file) => self.upload_file(file).await?,
                Change::Download(file) => self.download_file(file).await?,
                Change::Delete(file) => self.delete_file(file).await?,
                Change::Conflict(file) => self.handle_conflict(file).await?,
            }
        }

        Ok(SyncReport { /* ... */ })
    }
}
```

## **Tauri GUI Integration**

### **Multi-Mode Interface**
```jsx
// src/Dashboard.jsx - Unified interface
import { invoke } from '@tauri-apps/api/tauri'

function UnifiedDashboard() {
    const [mode, setMode] = useState('server')
    const [servers, setServers] = useState([])
    const [connections, setConnections] = useState([])

    return (
        <div className="unified-dashboard">
            <nav className="mode-tabs">
                <button
                    onClick={() => setMode('server')}
                    className={mode === 'server' ? 'active' : ''}
                >
                    🗂️ Server
                </button>
                <button
                    onClick={() => setMode('client')}
                    className={mode === 'client' ? 'active' : ''}
                >
                    🔌 Client
                </button>
                <button
                    onClick={() => setMode('browser')}
                    className={mode === 'browser' ? 'active' : ''}
                >
                    📁 Browser
                </button>
                <button
                    onClick={() => setMode('monitor')}
                    className={mode === 'monitor' ? 'active' : ''}
                >
                    📊 Monitor
                </button>
            </nav>

            <div className="mode-content">
                {mode === 'server' && <ServerMode />}
                {mode === 'client' && <ClientMode servers={servers} />}
                {mode === 'browser' && <FileBrowser />}
                {mode === 'monitor' && <GrafanaMonitoring />}
            </div>
        </div>
    )
}

function ServerMode() {
    return (
        <div className="server-panel">
            <h2>9P.e Server</h2>
            <div className="server-config">
                <label>Folder to serve:</label>
                <input type="text" defaultValue="/home/user/documents" />
                <button>Browse...</button>
            </div>
            <div className="server-status">
                <div className="status-card">
                    <h3>📁 Serving</h3>
                    <p>/home/user/documents</p>
                </div>
                <div className="status-card">
                    <h3>🌐 Address</h3>
                    <p>192.168.1.100:564</p>
                </div>
                <div className="status-card">
                    <h3>👥 Connections</h3>
                    <p>3 active</p>
                </div>
            </div>
            <button className="primary">Start Server</button>
        </div>
    )
}

function ClientMode({ servers }) {
    return (
        <div className="client-panel">
            <h2>9P.e Client</h2>
            <div className="discovery">
                <h3>🔍 Available Servers</h3>
                {servers.map(server => (
                    <div key={server.address} className="server-card">
                        <h4>{server.name}</h4>
                        <p>{server.address}</p>
                        <button onClick={() => connectToServer(server)}>
                            Connect
                        </button>
                    </div>
                ))}
                <button onClick={discoverServers}>Refresh</button>
            </div>
        </div>
    )
}
```

## **Competitive Advantage**

### **vs Traditional 9P Tools**
| Feature | Traditional | Our Unified App |
|---------|-------------|-----------------|
| **Learning curve** | Multiple tools to learn | Single interface |
| **Installation** | Multiple packages | One installer |
| **Configuration** | Text files, man pages | Visual interface |
| **Monitoring** | None | Built-in Grafana |
| **File browsing** | Command line only | GUI file browser |
| **Network discovery** | Manual IP entry | Auto-discovery |

### **Market Position**
- **"The Dropbox of 9P"** - Easy GUI for complex protocol
- **"All-in-one 9P solution"** - Server + client + monitoring
- **"Enterprise 9P made simple"** - Professional tools, simple interface

## **Implementation Phases**

### **Phase 1: Enhanced Server (DONE)**
✅ Basic server functionality
✅ CLI interface
✅ Path security

### **Phase 2: Client Mode (1-2 weeks)**
- 9P client implementation
- FUSE integration for mounting
- Basic file operations

### **Phase 3: GUI Integration (1 week)**
- Tauri app with server/client modes
- File browser interface
- Server management

### **Phase 4: Advanced Features (2 weeks)**
- Network discovery
- Grafana monitoring
- Sync functionality
- Enterprise features

**Result**: The **first comprehensive 9P solution** that makes the protocol accessible to everyone, not just Unix experts. This would completely revolutionize how people use 9P! 🚀