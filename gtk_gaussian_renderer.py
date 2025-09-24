#!/usr/bin/env python3
"""GTK Dashboard with Gaussian Splatting as the PRIMARY rendering system - Real System Data"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('cairo', '1.0')
from gi.repository import Gtk, GLib, Gdk
import cairo
import math
import os
import psutil
import socket
from collections import deque
from datetime import datetime
import subprocess

class GaussianSplatRenderer:
    """Core Gaussian Splatting renderer - this IS the display system"""

    def __init__(self, width, height):
        self.width = width
        self.height = height
        self.gaussians = []

    def clear(self):
        """Clear all gaussians"""
        self.gaussians = []

    def add_gaussian_for_point(self, x, y, value, max_value, color=(0, 1, 0)):
        """Add a Gaussian splat to represent a data point"""
        # Each data point becomes a Gaussian
        # Size based on value magnitude
        scale = (value / max_value) * 0.05 + 0.02

        gaussian = {
            'x': x,
            'y': y,
            'sx': scale,
            'sy': scale * 0.8,  # Slightly elliptical
            'rotation': 0,
            'intensity': min(1.0, value / max_value),
            'color': color
        }
        self.gaussians.append(gaussian)

    def add_bar_gaussians(self, x, height, width, value, max_value, color):
        """Create a bar using multiple Gaussians"""
        # Use multiple Gaussians to form a bar
        num_gaussians = max(1, int(height * 10))
        for i in range(num_gaussians):
            y_pos = 1.0 - (i / num_gaussians) * height
            self.add_gaussian_for_point(
                x + width/2,
                y_pos,
                value * (1.0 - i/num_gaussians),  # Fade towards top
                max_value,
                color
            )

    def render_text_with_gaussians(self, cr, text, x, y, size=0.02):
        """Approximate text rendering using Gaussian points"""
        # This is simplified - in real implementation would use proper text metrics
        for i, char in enumerate(text):
            self.add_gaussian_for_point(
                x + i * size * 2,
                y,
                1.0,
                1.0,
                (1, 1, 1)
            )

    def render(self, cr):
        """Render all Gaussians to create the display"""
        # Render each Gaussian
        for g in self.gaussians:
            self._render_single_gaussian(cr, g)

    def _render_single_gaussian(self, cr, g):
        """Render a single 2D Gaussian splat"""
        cr.save()

        # Position
        px = g['x'] * self.width
        py = g['y'] * self.height

        # Scale
        sx = g['sx'] * self.width
        sy = g['sy'] * self.height

        # Create Gaussian falloff
        cr.translate(px, py)
        cr.rotate(g['rotation'])

        # Gaussian gradient with proper falloff
        gradient = cairo.RadialGradient(0, 0, 0, 0, 0, max(sx, sy))

        r, g_val, b = g['color']
        intensity = g['intensity']

        # Gaussian falloff: exp(-x²/2σ²)
        # Approximate with gradient stops
        gradient.add_color_stop_rgba(0, r*intensity, g_val*intensity, b*intensity, 0.9)
        gradient.add_color_stop_rgba(0.3, r*intensity*0.7, g_val*intensity*0.7, b*intensity*0.7, 0.6)
        gradient.add_color_stop_rgba(0.6, r*intensity*0.3, g_val*intensity*0.3, b*intensity*0.3, 0.3)
        gradient.add_color_stop_rgba(1.0, 0, 0, 0, 0)

        cr.set_source(gradient)
        cr.scale(sx, sy)
        cr.arc(0, 0, 1, 0, 2 * math.pi)
        cr.fill()

        cr.restore()

class SystemMonitor:
    """Real system monitoring data"""

    def __init__(self):
        self.cpu_history = deque(maxlen=60)
        self.mem_history = deque(maxlen=60)
        self.net_history = deque(maxlen=60)
        self.last_net_io = psutil.net_io_counters()
        self.boot_time = psutil.boot_time()

    def update(self):
        """Update all metrics with real data"""
        # CPU
        self.cpu_history.append(psutil.cpu_percent(interval=0))

        # Memory
        mem = psutil.virtual_memory()
        self.mem_history.append(mem.percent)

        # Network (bytes per second)
        net_io = psutil.net_io_counters()
        bytes_sent = net_io.bytes_sent - self.last_net_io.bytes_sent
        bytes_recv = net_io.bytes_recv - self.last_net_io.bytes_recv
        self.net_history.append((bytes_sent + bytes_recv) / 1024.0)  # KB/s
        self.last_net_io = net_io

        return {
            'cpu': list(self.cpu_history),
            'memory': list(self.mem_history),
            'network': list(self.net_history),
            'mem_info': mem,
            'uptime': self.get_uptime(),
            'load_avg': os.getloadavg(),
            'processes': self.get_processes()
        }

    def get_uptime(self):
        """Get real system uptime"""
        uptime_seconds = datetime.now().timestamp() - self.boot_time
        days = int(uptime_seconds // 86400)
        hours = int((uptime_seconds % 86400) // 3600)
        minutes = int((uptime_seconds % 3600) // 60)
        return f"{days}d {hours}h {minutes}m"

    def get_processes(self):
        """Get ALL processes with real data"""
        processes = []
        for proc in psutil.process_iter(['pid', 'name', 'cpu_percent', 'memory_info']):
            try:
                pinfo = proc.info
                processes.append({
                    'pid': pinfo['pid'],
                    'name': pinfo['name'][:20],  # Truncate long names
                    'cpu': pinfo['cpu_percent'] or 0,
                    'memory': pinfo['memory_info'].rss / 1024 / 1024 if pinfo['memory_info'] else 0
                })
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue
        return sorted(processes, key=lambda x: x['cpu'], reverse=True)

class GaussianChart(Gtk.DrawingArea):
    """Chart rendered entirely with Gaussian splatting"""

    def __init__(self, chart_type="cpu"):
        super().__init__()
        self.chart_type = chart_type
        self.data = []
        self.set_size_request(500, 300)
        self.renderer = None
        self.set_draw_func(self.draw)

    def update_data(self, data):
        """Update with real system data"""
        self.data = data
        self.queue_draw()

    def draw(self, area, cr, width, height):
        """Render chart using Gaussian splatting"""
        # Black background
        cr.set_source_rgb(0.05, 0.05, 0.1)
        cr.rectangle(0, 0, width, height)
        cr.fill()

        if not self.data:
            return

        # Create renderer
        self.renderer = GaussianSplatRenderer(width, height)

        # Determine scale
        if self.chart_type == "cpu" or self.chart_type == "memory":
            max_value = 100
        else:  # network
            max_value = max(self.data) if self.data else 1000

        # Color based on type
        colors = {
            'cpu': (0.2, 0.8, 0.2),    # Green
            'memory': (0.8, 0.2, 0.8),  # Purple
            'network': (0.2, 0.6, 1.0)  # Blue
        }
        color = colors.get(self.chart_type, (0.5, 0.5, 0.5))

        # Create line chart with Gaussians
        if len(self.data) > 1:
            for i, value in enumerate(self.data):
                x = i / (len(self.data) - 1)
                y = 1.0 - (value / max_value)

                # Add Gaussian for each data point
                self.renderer.add_gaussian_for_point(x, y, value, max_value, color)

                # Add connecting Gaussians for smooth line
                if i > 0:
                    prev_value = self.data[i-1]
                    prev_y = 1.0 - (prev_value / max_value)

                    # Interpolate between points
                    steps = 5
                    for j in range(1, steps):
                        interp = j / steps
                        inter_x = (i-1 + interp) / (len(self.data) - 1)
                        inter_y = prev_y + (y - prev_y) * interp
                        inter_val = prev_value + (value - prev_value) * interp
                        self.renderer.add_gaussian_for_point(
                            inter_x, inter_y, inter_val, max_value, color
                        )

        # Render grid lines with dim Gaussians
        for i in range(5):
            y = i / 4
            for x in range(0, 100, 2):
                self.renderer.add_gaussian_for_point(
                    x/100, y, 0.2, 1.0, (0.2, 0.2, 0.3)
                )

        # Render all Gaussians
        self.renderer.render(cr)

        # Add labels with Cairo (until we implement text via Gaussians)
        cr.set_source_rgb(1, 1, 1)
        cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        cr.set_font_size(14)
        cr.move_to(10, 20)

        if self.chart_type == "cpu":
            label = f"CPU: {self.data[-1]:.1f}%" if self.data else "CPU: --"
        elif self.chart_type == "memory":
            label = f"Memory: {self.data[-1]:.1f}%" if self.data else "Memory: --"
        else:
            label = f"Network: {self.data[-1]:.1f} KB/s" if self.data else "Network: --"

        cr.show_text(label)

class ProcessListView(Gtk.ScrolledWindow):
    """Full scrollable process list with ALL processes"""

    def __init__(self):
        super().__init__()
        self.set_vexpand(True)
        self.set_hexpand(True)
        self.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)

        # Create list store
        self.list_store = Gtk.ListStore(int, str, float, float)

        # Create tree view
        self.tree_view = Gtk.TreeView(model=self.list_store)

        # Add columns
        for i, title in enumerate(["PID", "Process", "CPU %", "Memory (MB)"]):
            renderer = Gtk.CellRendererText()
            column = Gtk.TreeViewColumn(title, renderer, text=i)
            column.set_sort_column_id(i)
            column.set_resizable(True)
            self.tree_view.append_column(column)

        self.set_child(self.tree_view)

    def update_processes(self, processes):
        """Update with real process data"""
        self.list_store.clear()
        for proc in processes:
            self.list_store.append([
                proc['pid'],
                proc['name'],
                round(proc['cpu'], 1),
                round(proc['memory'], 1)
            ])

class GaussianDashboard(Gtk.ApplicationWindow):
    """Main dashboard - everything rendered with Gaussian splatting"""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("🔥 9P.e Gaussian Splatting System Monitor")
        self.set_default_size(1600, 900)

        self.monitor = SystemMonitor()

        # Main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        self.set_child(main_box)

        # Header
        self.create_header(main_box)

        # Main content area
        content_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=5)
        main_box.append(content_box)

        # Left side - Charts
        charts_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        content_box.append(charts_box)

        # CPU Chart
        cpu_frame = Gtk.Frame()
        cpu_frame.set_label("CPU Usage")
        self.cpu_chart = GaussianChart("cpu")
        cpu_frame.set_child(self.cpu_chart)
        charts_box.append(cpu_frame)

        # Memory Chart
        mem_frame = Gtk.Frame()
        mem_frame.set_label("Memory Usage")
        self.mem_chart = GaussianChart("memory")
        mem_frame.set_child(self.mem_chart)
        charts_box.append(mem_frame)

        # Network Chart
        net_frame = Gtk.Frame()
        net_frame.set_label("Network Activity")
        self.net_chart = GaussianChart("network")
        net_frame.set_child(self.net_chart)
        charts_box.append(net_frame)

        # Right side - Info and Processes
        right_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        right_box.set_size_request(600, -1)
        content_box.append(right_box)

        # System info
        info_frame = Gtk.Frame()
        info_frame.set_label("System Information")
        self.info_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        self.info_box.set_margin_start(10)
        self.info_box.set_margin_end(10)
        self.info_box.set_margin_top(10)
        self.info_box.set_margin_bottom(10)
        info_frame.set_child(self.info_box)
        right_box.append(info_frame)

        # Create info labels
        self.info_labels = {}
        for key in ['uptime', 'load', 'kernel', 'processes', 'memory']:
            label = Gtk.Label()
            label.set_xalign(0)
            self.info_labels[key] = label
            self.info_box.append(label)

        # Process list
        proc_frame = Gtk.Frame()
        proc_frame.set_label("All Processes (sorted by CPU usage)")
        self.process_list = ProcessListView()
        proc_frame.set_child(self.process_list)
        right_box.append(proc_frame)

        # Start updates
        GLib.timeout_add(1000, self.update_display)
        self.update_display()  # Initial update

    def create_header(self, parent):
        """Create header bar"""
        header = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
        header.set_margin_start(10)
        header.set_margin_end(10)
        header.set_margin_top(5)
        header.set_margin_bottom(5)

        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>Gaussian Splatting Renderer - Real System Monitor</span>")
        header.append(title)

        # Spacer
        header.append(Gtk.Box())

        self.time_label = Gtk.Label()
        header.append(self.time_label)

        parent.append(header)

    def update_display(self):
        """Update all displays with real data"""
        # Get real system data
        data = self.monitor.update()

        # Update time
        self.time_label.set_text(datetime.now().strftime("%Y-%m-%d %H:%M:%S"))

        # Update charts with real data
        self.cpu_chart.update_data(data['cpu'])
        self.mem_chart.update_data(data['memory'])
        self.net_chart.update_data(data['network'])

        # Update system info with REAL values
        kernel = os.uname()
        self.info_labels['uptime'].set_markup(f"<b>Uptime:</b> {data['uptime']}")
        self.info_labels['load'].set_markup(f"<b>Load Average:</b> {data['load_avg'][0]:.2f}, {data['load_avg'][1]:.2f}, {data['load_avg'][2]:.2f}")
        self.info_labels['kernel'].set_markup(f"<b>Kernel:</b> {kernel.sysname} {kernel.release}")
        self.info_labels['processes'].set_markup(f"<b>Total Processes:</b> {len(data['processes'])}")
        self.info_labels['memory'].set_markup(f"<b>Memory:</b> {data['mem_info'].used//1024//1024} MB / {data['mem_info'].total//1024//1024} MB ({data['mem_info'].percent:.1f}%)")

        # Update process list with ALL processes
        self.process_list.update_processes(data['processes'])

        return True  # Continue timer

class GaussianApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.gaussian_renderer')

    def do_activate(self):
        win = GaussianDashboard(application=self)
        win.present()

if __name__ == '__main__':
    app = GaussianApp()
    app.run(None)