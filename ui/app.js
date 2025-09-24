// 9PE Server Dashboard JavaScript

class Dashboard {
    constructor() {
        this.isServerRunning = false;
        this.metrics = {
            connections: 0,
            messagesPerSec: 0,
            throughput: 0,
            openFids: 0,
            errorRate: 0,
            memoryUsage: 0
        };
        this.chart = null;
        this.config = {
            protocol: 'tcp',
            port: 5640,
            root_path: '/tmp',
            max_msg_size: 65536,
            auth_enabled: false
        };

        this.init();
    }

    async init() {
        this.setupEventListeners();
        this.setupChart();
        await this.loadConfig();
        this.startMetricsLoop();
        this.addLog('info', 'Dashboard initialized');
    }

    setupEventListeners() {
        // Start/Stop Server
        document.getElementById('startStopBtn').addEventListener('click', () => {
            this.toggleServer();
        });

        // Configuration
        document.getElementById('configBtn').addEventListener('click', () => {
            this.showConfigModal();
        });

        document.getElementById('closeConfigModal').addEventListener('click', () => {
            this.hideConfigModal();
        });

        document.getElementById('cancelConfig').addEventListener('click', () => {
            this.hideConfigModal();
        });

        document.getElementById('saveConfig').addEventListener('click', () => {
            this.saveConfig();
        });

        // File Browser
        document.getElementById('refreshFiles').addEventListener('click', () => {
            this.refreshFiles();
        });

        // Logs
        document.getElementById('clearLogs').addEventListener('click', () => {
            this.clearLogs();
        });

        // Modal background click
        document.getElementById('configModal').addEventListener('click', (e) => {
            if (e.target.id === 'configModal') {
                this.hideConfigModal();
            }
        });
    }

    setupChart() {
        const ctx = document.getElementById('performanceChart').getContext('2d');
        this.chart = new Chart(ctx, {
            type: 'line',
            data: {
                labels: Array.from({length: 20}, (_, i) => `${19-i}s`),
                datasets: [
                    {
                        label: 'Messages/sec',
                        data: Array(20).fill(0),
                        borderColor: '#00ff88',
                        backgroundColor: 'rgba(0, 255, 136, 0.1)',
                        tension: 0.4
                    },
                    {
                        label: 'Throughput (MB/s)',
                        data: Array(20).fill(0),
                        borderColor: '#0066ff',
                        backgroundColor: 'rgba(0, 102, 255, 0.1)',
                        tension: 0.4
                    }
                ]
            },
            options: {
                responsive: true,
                plugins: {
                    legend: {
                        labels: {
                            color: '#e0e0e0'
                        }
                    }
                },
                scales: {
                    x: {
                        ticks: { color: '#888' },
                        grid: { color: '#333' }
                    },
                    y: {
                        ticks: { color: '#888' },
                        grid: { color: '#333' }
                    }
                }
            }
        });
    }

    async loadConfig() {
        try {
            if (window.__TAURI__) {
                // Tauri mode
                this.config = await window.__TAURI__.invoke('get_server_config');
            } else {
                // Web mode - use defaults
                console.log('Running in web mode, using default config');
            }
            this.updateSystemInfo();
        } catch (error) {
            console.error('Failed to load config:', error);
            this.addLog('error', 'Failed to load server configuration');
        }
    }

    async updateMetrics() {
        try {
            let newMetrics;

            if (window.__TAURI__) {
                // Tauri mode - get real metrics
                newMetrics = await window.__TAURI__.invoke('get_metrics');
            } else {
                // Web mode - simulate metrics
                newMetrics = {
                    connections: Math.floor(Math.random() * 10) + 1,
                    messages_per_sec: Math.random() * 100,
                    throughput: Math.random() * 50,
                    open_fids: Math.floor(Math.random() * 100) + 10,
                    error_rate: Math.random() * 5,
                    memory_mb: Math.floor(Math.random() * 100) + 50
                };
            }

            // Update metrics display
            document.getElementById('connectionsCount').textContent = newMetrics.connections || 0;
            document.getElementById('messagesPerSec').textContent = (newMetrics.messages_per_sec || 0).toFixed(1);
            document.getElementById('throughput').textContent = (newMetrics.throughput || 0).toFixed(1);
            document.getElementById('openFids').textContent = newMetrics.open_fids || 0;
            document.getElementById('errorRate').textContent = (newMetrics.error_rate || 0).toFixed(1) + '%';
            document.getElementById('memoryUsage').textContent = newMetrics.memory_mb || 0;

            // Update chart
            if (this.chart) {
                this.chart.data.datasets[0].data.shift();
                this.chart.data.datasets[0].data.push(newMetrics.messages_per_sec || 0);
                this.chart.data.datasets[1].data.shift();
                this.chart.data.datasets[1].data.push(newMetrics.throughput || 0);
                this.chart.update('none');
            }

            this.metrics = newMetrics;

        } catch (error) {
            console.error('Failed to update metrics:', error);
            this.addLog('error', 'Failed to fetch metrics');
        }
    }

    async toggleServer() {
        const btn = document.getElementById('startStopBtn');
        const icon = btn.querySelector('i');
        const text = btn.childNodes[1];

        try {
            if (this.isServerRunning) {
                // Stop server
                if (window.__TAURI__) {
                    await window.__TAURI__.invoke('stop_server');
                }

                this.isServerRunning = false;
                icon.className = 'fas fa-play';
                text.textContent = ' Start Server';
                btn.className = 'btn btn-primary';
                this.updateServerStatus('offline', 'Offline');
                this.addLog('info', 'Server stopped');

            } else {
                // Start server
                if (window.__TAURI__) {
                    await window.__TAURI__.invoke('start_server');
                }

                this.isServerRunning = true;
                icon.className = 'fas fa-stop';
                text.textContent = ' Stop Server';
                btn.className = 'btn btn-danger';
                this.updateServerStatus('online', 'Online');
                this.addLog('info', `Server started on port ${this.config.port}`);
            }
        } catch (error) {
            console.error('Failed to toggle server:', error);
            this.addLog('error', `Failed to ${this.isServerRunning ? 'stop' : 'start'} server: ${error}`);
        }
    }

    updateServerStatus(status, text) {
        const statusEl = document.getElementById('serverStatus');
        statusEl.className = `server-status ${status}`;
        statusEl.innerHTML = `<i class="fas fa-circle"></i> ${text}`;
    }

    showConfigModal() {
        const modal = document.getElementById('configModal');

        // Load current config
        document.getElementById('configProtocol').value = this.config.protocol;
        document.getElementById('configPort').value = this.config.port;
        document.getElementById('configRootPath').value = this.config.root_path;
        document.getElementById('configMaxMsgSize').value = this.config.max_msg_size;
        document.getElementById('configAuth').checked = this.config.auth_enabled;

        modal.classList.add('active');
    }

    hideConfigModal() {
        document.getElementById('configModal').classList.remove('active');
    }

    async saveConfig() {
        const newConfig = {
            protocol: document.getElementById('configProtocol').value,
            port: parseInt(document.getElementById('configPort').value),
            root_path: document.getElementById('configRootPath').value,
            max_msg_size: parseInt(document.getElementById('configMaxMsgSize').value),
            auth_enabled: document.getElementById('configAuth').checked
        };

        try {
            if (window.__TAURI__) {
                await window.__TAURI__.invoke('update_config', { config: newConfig });
            }

            this.config = newConfig;
            this.updateSystemInfo();
            this.hideConfigModal();
            this.addLog('info', 'Configuration updated');

        } catch (error) {
            console.error('Failed to save config:', error);
            this.addLog('error', 'Failed to save configuration');
        }
    }

    updateSystemInfo() {
        document.getElementById('protocolInfo').textContent = this.config.protocol.toUpperCase();
        document.getElementById('portInfo').textContent = this.config.port;
        document.getElementById('rootPathInfo').textContent = this.config.root_path;
        document.getElementById('authInfo').textContent = this.config.auth_enabled ? 'Enabled' : 'Disabled';
    }

    async refreshFiles() {
        const browser = document.getElementById('fileBrowser');
        const path = document.getElementById('currentPath').value;

        browser.innerHTML = '<div class="loading"><i class="fas fa-spinner fa-spin"></i> Loading files...</div>';

        try {
            let files;

            if (window.__TAURI__) {
                files = await window.__TAURI__.invoke('list_files', { path });
            } else {
                // Web mode - simulate file list
                files = [
                    { name: '..', path: '/parent', is_dir: true, size: 0, modified: Date.now() },
                    { name: 'documents', path: '/tmp/documents', is_dir: true, size: 0, modified: Date.now() },
                    { name: 'test.txt', path: '/tmp/test.txt', is_dir: false, size: 1024, modified: Date.now() },
                    { name: 'config.json', path: '/tmp/config.json', is_dir: false, size: 256, modified: Date.now() }
                ];
            }

            browser.innerHTML = '';

            files.forEach(file => {
                const item = document.createElement('div');
                item.className = 'file-item';
                if (file.is_dir) item.classList.add('directory');

                const icon = file.is_dir ? 'fas fa-folder' : 'fas fa-file';
                const size = file.is_dir ? '' : this.formatFileSize(file.size);

                item.innerHTML = `
                    <i class="${icon}"></i>
                    <span class="file-name">${file.name}</span>
                    <span class="file-size">${size}</span>
                `;

                item.addEventListener('click', () => {
                    if (file.is_dir) {
                        document.getElementById('currentPath').value = file.path;
                        this.refreshFiles();
                    }
                });

                browser.appendChild(item);
            });

        } catch (error) {
            console.error('Failed to load files:', error);
            browser.innerHTML = '<div class="loading" style="color: #ff4444;"><i class="fas fa-exclamation-triangle"></i> Failed to load files</div>';
            this.addLog('error', 'Failed to load file list');
        }
    }

    formatFileSize(bytes) {
        if (bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
    }

    addLog(level, message) {
        const container = document.getElementById('logContainer');
        const entry = document.createElement('div');
        entry.className = 'log-entry';

        const time = new Date().toLocaleTimeString();

        entry.innerHTML = `
            <span class="log-time">${time}</span>
            <span class="log-level ${level}">${level.toUpperCase()}</span>
            <span class="log-message">${message}</span>
        `;

        container.appendChild(entry);
        container.scrollTop = container.scrollHeight;

        // Keep only last 100 entries
        while (container.children.length > 100) {
            container.removeChild(container.firstChild);
        }
    }

    clearLogs() {
        document.getElementById('logContainer').innerHTML = '';
        this.addLog('info', 'Logs cleared');
    }

    startMetricsLoop() {
        // Update metrics every 2 seconds
        setInterval(() => {
            this.updateMetrics();
        }, 2000);

        // Initial update
        this.updateMetrics();

        // Initial file load
        this.refreshFiles();
    }
}

// Initialize dashboard when page loads
document.addEventListener('DOMContentLoaded', () => {
    window.dashboard = new Dashboard();
});

// Handle Tauri-specific initialization
if (window.__TAURI__) {
    document.addEventListener('DOMContentLoaded', () => {
        console.log('Running in Tauri mode');
    });
} else {
    console.log('Running in web mode');
}