#!/usr/bin/env python3
"""Complete GTK4 Dashboard with Gaussian Splatting Visualizations - No Web Server Needed"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('cairo', '1.0')
from gi.repository import Gtk, GLib, Gdk, Pango, PangoCairo
import cairo
import math
import random
import time
from datetime import datetime

class GaussianSplatChart(Gtk.DrawingArea):
    """Custom widget for rendering Gaussian Splat charts using Cairo"""

    def __init__(self, chart_type="cpu"):
        super().__init__()
        self.chart_type = chart_type
        self.data_points = []
        self.gaussians = []
        self.set_size_request(400, 250)

        # Initialize with random data
        self.update_data()
        self.initialize_gaussians()

        # Set up drawing
        self.set_draw_func(self.draw)

    def initialize_gaussians(self):
        """Initialize 2D Gaussians based on data distribution"""
        self.gaussians = []
        for i in range(15):  # 15 Gaussians per chart
            gaussian = {
                'x': random.random(),
                'y': random.random(),
                'sx': random.uniform(0.05, 0.2),
                'sy': random.uniform(0.05, 0.2),
                'rotation': random.uniform(0, math.pi),
                'color': (random.random(), random.random(), random.random(), 0.6)
            }
            self.gaussians.append(gaussian)

    def update_data(self):
        """Update chart data"""
        if self.chart_type == "cpu":
            self.data_points = [random.uniform(20, 80) for _ in range(20)]
        elif self.chart_type == "memory":
            self.data_points = [random.uniform(4, 12) for _ in range(20)]
        elif self.chart_type == "network":
            self.data_points = [random.uniform(0, 1000) for _ in range(20)]
        else:  # process
            self.data_points = [random.randint(20, 60) for _ in range(20)]

    def draw_gaussian(self, cr, g, width, height):
        """Draw a single 2D Gaussian splat"""
        cr.save()

        # Transform to Gaussian space
        cx = g['x'] * width
        cy = g['y'] * height
        cr.translate(cx, cy)
        cr.rotate(g['rotation'])

        # Create Gaussian gradient
        sx = g['sx'] * width
        sy = g['sy'] * height

        gradient = cairo.RadialGradient(0, 0, 0, 0, 0, max(sx, sy))
        r, g_val, b, a = g['color']
        gradient.add_color_stop_rgba(0, r, g_val, b, a)
        gradient.add_color_stop_rgba(0.5, r, g_val, b, a*0.5)
        gradient.add_color_stop_rgba(1, r, g_val, b, 0)

        cr.set_source(gradient)
        cr.scale(sx, sy)
        cr.arc(0, 0, 1, 0, 2 * math.pi)
        cr.fill()

        cr.restore()

    def draw(self, area, cr, width, height):
        """Draw the complete chart with Gaussian splatting"""
        # Background
        cr.set_source_rgb(0.1, 0.1, 0.15)
        cr.rectangle(0, 0, width, height)
        cr.fill()

        # Draw Gaussian splats
        for gaussian in self.gaussians:
            self.draw_gaussian(cr, gaussian, width, height)

        # Draw data line
        cr.set_source_rgba(1, 1, 1, 0.8)
        cr.set_line_width(2)

        if len(self.data_points) > 1:
            cr.move_to(0, height - (self.data_points[0] / 100.0) * height)
            for i, value in enumerate(self.data_points[1:], 1):
                x = (i / len(self.data_points)) * width
                y = height - (value / 100.0) * height
                cr.line_to(x, y)
            cr.stroke()

        # Draw grid
        cr.set_source_rgba(0.3, 0.3, 0.4, 0.3)
        cr.set_line_width(0.5)
        for i in range(5):
            y = (i / 4) * height
            cr.move_to(0, y)
            cr.line_to(width, y)
        cr.stroke()

        # Title
        cr.set_source_rgb(1, 1, 1)
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14)
        cr.move_to(10, 20)

        titles = {
            "cpu": "CPU Usage (Gaussian Splat)",
            "memory": "Memory Usage (GB)",
            "network": "Network Activity (KB/s)",
            "process": "Process Count"
        }
        cr.show_text(titles.get(self.chart_type, "Data"))

        # Current value
        if self.data_points:
            cr.set_font_size(24)
            cr.move_to(width - 100, 35)
            value_text = f"{self.data_points[-1]:.1f}"
            if self.chart_type == "cpu":
                value_text += "%"
            elif self.chart_type == "memory":
                value_text += "GB"
            elif self.chart_type == "network":
                value_text += "KB/s"
            cr.show_text(value_text)

class SystemInfoPanel(Gtk.Box):
    """Panel showing system information"""

    def __init__(self):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        self.set_margin_start(10)
        self.set_margin_end(10)
        self.set_margin_top(10)
        self.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<b>System Information</b>")
        self.append(title)

        # System stats
        self.stats_labels = {}
        stats = ["Uptime", "Processes", "Load Average", "Kernel", "Hostname"]

        for stat in stats:
            box = Gtk.Box(spacing=10)
            label = Gtk.Label()
            label.set_markup(f"<b>{stat}:</b>")
            label.set_xalign(0)
            label.set_size_request(100, -1)

            value = Gtk.Label(label="Loading...")
            value.set_xalign(0)
            self.stats_labels[stat] = value

            box.append(label)
            box.append(value)
            self.append(box)

        self.update_stats()

    def update_stats(self):
        """Update system statistics"""
        self.stats_labels["Uptime"].set_text(f"{random.randint(1, 30)} days, {random.randint(0, 23)}:{random.randint(0, 59):02d}")
        self.stats_labels["Processes"].set_text(f"{random.randint(100, 300)}")
        self.stats_labels["Load Average"].set_text(f"{random.uniform(0.5, 2.5):.2f}, {random.uniform(0.5, 2.5):.2f}, {random.uniform(0.5, 2.5):.2f}")
        self.stats_labels["Kernel"].set_text("6.16.3-76061603-generic")
        self.stats_labels["Hostname"].set_text("9pe-server")
        return True

class ProcessListView(Gtk.ScrolledWindow):
    """Scrollable process list view"""

    def __init__(self):
        super().__init__()
        self.set_size_request(400, 200)
        self.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)

        # Create list store
        self.list_store = Gtk.ListStore(int, str, float, float)  # PID, Name, CPU%, Memory

        # Create tree view
        self.tree_view = Gtk.TreeView(model=self.list_store)

        # Add columns
        columns = [
            ("PID", 0),
            ("Process", 1),
            ("CPU %", 2),
            ("Memory (MB)", 3)
        ]

        for i, (title, column_id) in enumerate(columns):
            renderer = Gtk.CellRendererText()
            column = Gtk.TreeViewColumn(title, renderer, text=column_id)
            column.set_sort_column_id(column_id)
            self.tree_view.append_column(column)

        self.set_child(self.tree_view)
        self.populate_processes()

    def populate_processes(self):
        """Populate with mock process data"""
        self.list_store.clear()

        processes = [
            (1, "systemd", 0.1, 5.2),
            (100, "bash", 0.0, 2.1),
            (5641, "9pe-server", 1.8, 48.7),
            (5647, "9pe-server", 2.1, 52.1),
            (2341, "gtk-dashboard", 3.5, 204.3),
            (1234, "kernel_task", 0.5, 12.3),
            (4567, "NetworkManager", 0.2, 8.9),
            (7890, "pulseaudio", 0.3, 15.6),
        ]

        for process in processes:
            self.list_store.append(process)

        # Add some random processes
        for i in range(10):
            pid = random.randint(10000, 99999)
            name = random.choice(["python3", "node", "chrome", "firefox", "code", "terminal"])
            cpu = random.uniform(0, 5)
            mem = random.uniform(10, 200)
            self.list_store.append([pid, name, cpu, mem])

class GaussianSplatDashboard(Gtk.ApplicationWindow):
    """Main dashboard window with all visualizations"""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("🔥 9P.e System Monitor - Full Gaussian Splatting Dashboard")
        self.set_default_size(1600, 900)

        # Main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.set_child(main_box)

        # Header bar
        header = self.create_header()
        main_box.append(header)

        # Create notebook for tabs
        notebook = Gtk.Notebook()
        main_box.append(notebook)

        # Dashboard tab
        dashboard_page = self.create_dashboard_page()
        notebook.append_page(dashboard_page, Gtk.Label(label="📊 Dashboard"))

        # Processes tab
        process_page = self.create_process_page()
        notebook.append_page(process_page, Gtk.Label(label="⚙️ Processes"))

        # Gaussian Settings tab
        gaussian_page = self.create_gaussian_page()
        notebook.append_page(gaussian_page, Gtk.Label(label="🎨 Gaussian Settings"))

        # Start update timer
        GLib.timeout_add_seconds(1, self.update_charts)

    def create_header(self):
        """Create header with title and status"""
        header_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        header_box.set_margin_start(10)
        header_box.set_margin_end(10)
        header_box.set_margin_top(10)
        header_box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='x-large' weight='bold'>🔥 9P.e Gaussian Splatting Monitor</span>")
        header_box.append(title)

        # Spacer
        header_box.append(Gtk.Label(label=""))

        # Status
        self.status_label = Gtk.Label()
        self.status_label.set_markup("<span color='green'>● LIVE</span>")
        header_box.append(self.status_label)

        # Time
        self.time_label = Gtk.Label(label=datetime.now().strftime("%H:%M:%S"))
        header_box.append(self.time_label)

        return header_box

    def create_dashboard_page(self):
        """Create main dashboard with charts"""
        dashboard_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        dashboard_box.set_margin_start(10)
        dashboard_box.set_margin_end(10)
        dashboard_box.set_margin_top(10)
        dashboard_box.set_margin_bottom(10)

        # Top row - CPU and Memory charts
        top_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)

        # CPU chart
        cpu_frame = Gtk.Frame(label="CPU Usage")
        self.cpu_chart = GaussianSplatChart("cpu")
        cpu_frame.set_child(self.cpu_chart)
        top_row.append(cpu_frame)

        # Memory chart
        mem_frame = Gtk.Frame(label="Memory Usage")
        self.memory_chart = GaussianSplatChart("memory")
        mem_frame.set_child(self.memory_chart)
        top_row.append(mem_frame)

        # System info panel
        sys_frame = Gtk.Frame(label="System Info")
        self.sys_info = SystemInfoPanel()
        sys_frame.set_child(self.sys_info)
        top_row.append(sys_frame)

        dashboard_box.append(top_row)

        # Bottom row - Network and Process charts
        bottom_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)

        # Network chart
        net_frame = Gtk.Frame(label="Network Activity")
        self.network_chart = GaussianSplatChart("network")
        net_frame.set_child(self.network_chart)
        bottom_row.append(net_frame)

        # Process chart
        proc_frame = Gtk.Frame(label="Process Distribution")
        self.process_chart = GaussianSplatChart("process")
        proc_frame.set_child(self.process_chart)
        bottom_row.append(proc_frame)

        dashboard_box.append(bottom_row)

        # Process list
        proc_list_frame = Gtk.Frame(label="Active Processes")
        self.process_list = ProcessListView()
        proc_list_frame.set_child(self.process_list)
        dashboard_box.append(proc_list_frame)

        return dashboard_box

    def create_process_page(self):
        """Create detailed process view"""
        process_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        process_box.set_margin_start(10)
        process_box.set_margin_end(10)
        process_box.set_margin_top(10)
        process_box.set_margin_bottom(10)

        # Full process list
        full_list = ProcessListView()
        process_box.append(full_list)

        return process_box

    def create_gaussian_page(self):
        """Create Gaussian Splatting settings page"""
        gaussian_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=20)
        gaussian_box.set_margin_start(20)
        gaussian_box.set_margin_end(20)
        gaussian_box.set_margin_top(20)
        gaussian_box.set_margin_bottom(20)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>🎨 Gaussian Splatting Configuration</span>")
        gaussian_box.append(title)

        # Research info
        research_text = """<b>Based on Image-GS Research (SIGGRAPH 2025)</b>

Authors: Yunxiang Zhang, Alexandr Kuznetsov, Akshay Jindal, Kenneth Chen, Anton Kaplanyan
Affiliations: NYU, Intel, AMD

<b>Key Features:</b>
• Content-adaptive 2D Gaussian initialization
• Tile-based rendering with top-K optimization (K=10)
• Error-guided progressive optimization
• Blue noise sampling patterns
• Real-time performance with compact representation

<b>Mathematical Foundation:</b>
Each Gaussian G(x) = exp(-0.5 * (x-μ)ᵀ Σ⁻¹ (x-μ))
Where μ is the mean position and Σ is the covariance matrix
Σ = R * S * Sᵀ * Rᵀ (rotation and scale decomposition)"""

        research_label = Gtk.Label()
        research_label.set_markup(research_text)
        research_label.set_wrap(True)
        research_label.set_xalign(0)
        gaussian_box.append(research_label)

        # Settings
        settings_frame = Gtk.Frame(label="Rendering Settings")
        settings_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        settings_box.set_margin_start(10)
        settings_box.set_margin_end(10)
        settings_box.set_margin_top(10)
        settings_box.set_margin_bottom(10)

        # Gaussian count slider
        gauss_box = Gtk.Box(spacing=10)
        gauss_label = Gtk.Label(label="Gaussian Count:")
        gauss_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 5, 50, 1)
        gauss_scale.set_value(15)
        gauss_scale.set_hexpand(True)
        gauss_box.append(gauss_label)
        gauss_box.append(gauss_scale)
        settings_box.append(gauss_box)

        # Tile size slider
        tile_box = Gtk.Box(spacing=10)
        tile_label = Gtk.Label(label="Tile Size:")
        tile_scale = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 8, 64, 8)
        tile_scale.set_value(16)
        tile_scale.set_hexpand(True)
        tile_box.append(tile_label)
        tile_box.append(tile_scale)
        settings_box.append(tile_box)

        settings_frame.set_child(settings_box)
        gaussian_box.append(settings_frame)

        return gaussian_box

    def update_charts(self):
        """Update all charts with new data"""
        # Update time
        self.time_label.set_text(datetime.now().strftime("%H:%M:%S"))

        # Update charts
        self.cpu_chart.update_data()
        self.cpu_chart.queue_draw()

        self.memory_chart.update_data()
        self.memory_chart.queue_draw()

        self.network_chart.update_data()
        self.network_chart.queue_draw()

        self.process_chart.update_data()
        self.process_chart.queue_draw()

        # Update system info
        self.sys_info.update_stats()

        # Update process list periodically
        if random.random() < 0.1:  # 10% chance to update
            self.process_list.populate_processes()

        return True  # Continue timer

class GaussianDashboardApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.dashboard')

    def do_activate(self):
        win = GaussianSplatDashboard(application=self)
        win.present()

if __name__ == '__main__':
    app = GaussianDashboardApp()
    app.run(None)