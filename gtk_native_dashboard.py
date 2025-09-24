#!/usr/bin/env python3
"""Native GTK Dashboard with Cairo-based charts and real system monitoring"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, GLib, Gdk, Adw
import cairo
import math
import os
import psutil
import socket
from collections import deque
from datetime import datetime
import subprocess

class CairoChart(Gtk.DrawingArea):
    """Native Cairo-based chart rendering"""

    def __init__(self, chart_type="line"):
        super().__init__()
        self.chart_type = chart_type
        self.data = deque(maxlen=60)
        self.max_value = 100
        self.color = (0.2, 0.6, 1.0)
        self.set_size_request(400, 200)
        self.set_draw_func(self.draw)

    def update_data(self, value):
        """Add new data point"""
        self.data.append(value)
        if self.chart_type != "cpu" and self.chart_type != "memory":
            if self.data:
                self.max_value = max(self.data) * 1.2 + 1
        self.queue_draw()

    def draw(self, area, cr, width, height):
        """Render chart using Cairo"""
        # Background
        cr.set_source_rgb(0.95, 0.95, 0.95)
        cr.rectangle(0, 0, width, height)
        cr.fill()

        # Draw grid
        cr.set_source_rgba(0.7, 0.7, 0.7, 0.3)
        cr.set_line_width(1)

        # Horizontal grid lines
        for i in range(5):
            y = (height / 4) * i
            cr.move_to(0, y)
            cr.line_to(width, y)
            cr.stroke()

        # Vertical grid lines
        for i in range(6):
            x = (width / 5) * i
            cr.move_to(x, 0)
            cr.line_to(x, height)
            cr.stroke()

        if not self.data:
            return

        # Draw chart line
        cr.set_source_rgb(*self.color)
        cr.set_line_width(2)

        for i, value in enumerate(self.data):
            x = (i / (60 - 1)) * width if len(self.data) < 60 else (i / (len(self.data) - 1)) * width
            y = height - (value / self.max_value) * height

            if i == 0:
                cr.move_to(x, y)
            else:
                cr.line_to(x, y)

        cr.stroke()

        # Fill area under line
        cr.set_source_rgba(*self.color, 0.2)
        for i, value in enumerate(self.data):
            x = (i / (60 - 1)) * width if len(self.data) < 60 else (i / (len(self.data) - 1)) * width
            y = height - (value / self.max_value) * height

            if i == 0:
                cr.move_to(x, height)
                cr.line_to(x, y)
            else:
                cr.line_to(x, y)

        if self.data:
            cr.line_to(x, height)
            cr.close_path()
            cr.fill()

        # Draw current value
        if self.data:
            cr.set_source_rgb(0.2, 0.2, 0.2)
            cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
            cr.set_font_size(16)
            cr.move_to(10, 20)

            if self.chart_type == "cpu":
                text = f"CPU: {self.data[-1]:.1f}%"
            elif self.chart_type == "memory":
                text = f"Memory: {self.data[-1]:.1f}%"
            elif self.chart_type == "network":
                text = f"Network: {self.data[-1]:.1f} KB/s"
            else:
                text = f"Value: {self.data[-1]:.1f}"

            cr.show_text(text)

class ProcessListView(Gtk.ScrolledWindow):
    """Full process list with all processes"""

    def __init__(self):
        super().__init__()
        self.set_vexpand(True)
        self.set_hexpand(True)
        self.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        self.set_min_content_height(400)

        # Create list store - PID, Name, CPU%, Memory MB
        self.list_store = Gtk.ListStore(int, str, float, float)

        # Create tree view
        self.tree_view = Gtk.TreeView(model=self.list_store)

        # Add columns
        for i, (title, width) in enumerate([
            ("PID", 80),
            ("Process", 200),
            ("CPU %", 80),
            ("Memory (MB)", 120)
        ]):
            renderer = Gtk.CellRendererText()
            column = Gtk.TreeViewColumn(title, renderer, text=i)
            column.set_sort_column_id(i)
            column.set_resizable(True)
            column.set_min_width(width)
            self.tree_view.append_column(column)

        self.set_child(self.tree_view)

    def update_processes(self):
        """Update with ALL system processes"""
        self.list_store.clear()

        processes = []
        for proc in psutil.process_iter(['pid', 'name', 'cpu_percent', 'memory_info']):
            try:
                pinfo = proc.info
                processes.append({
                    'pid': pinfo['pid'],
                    'name': pinfo['name'][:30],  # Limit name length
                    'cpu': pinfo['cpu_percent'] or 0,
                    'memory': pinfo['memory_info'].rss / 1024 / 1024 if pinfo['memory_info'] else 0
                })
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                continue

        # Sort by CPU usage
        processes.sort(key=lambda x: x['cpu'], reverse=True)

        # Add all processes to list
        for proc in processes:
            self.list_store.append([
                proc['pid'],
                proc['name'],
                round(proc['cpu'], 1),
                round(proc['memory'], 1)
            ])

class SystemMonitor:
    """Real system monitoring"""

    def __init__(self):
        self.boot_time = psutil.boot_time()
        self.last_net_io = psutil.net_io_counters()
        self.last_disk_io = psutil.disk_io_counters()

    def get_cpu_usage(self):
        """Get current CPU usage"""
        return psutil.cpu_percent(interval=0.1)

    def get_memory_usage(self):
        """Get memory usage percentage"""
        return psutil.virtual_memory().percent

    def get_network_speed(self):
        """Get network speed in KB/s"""
        current = psutil.net_io_counters()
        bytes_sent = current.bytes_sent - self.last_net_io.bytes_sent
        bytes_recv = current.bytes_recv - self.last_net_io.bytes_recv
        self.last_net_io = current
        return (bytes_sent + bytes_recv) / 1024.0  # KB/s

    def get_disk_io(self):
        """Get disk I/O in KB/s"""
        current = psutil.disk_io_counters()
        if current and self.last_disk_io:
            read_bytes = current.read_bytes - self.last_disk_io.read_bytes
            write_bytes = current.write_bytes - self.last_disk_io.write_bytes
            self.last_disk_io = current
            return (read_bytes + write_bytes) / 1024.0  # KB/s
        return 0

    def get_uptime(self):
        """Get system uptime as string"""
        uptime_seconds = datetime.now().timestamp() - self.boot_time
        days = int(uptime_seconds // 86400)
        hours = int((uptime_seconds % 86400) // 3600)
        minutes = int((uptime_seconds % 3600) // 60)
        return f"{days}d {hours}h {minutes}m"

    def get_system_info(self):
        """Get comprehensive system information"""
        mem = psutil.virtual_memory()
        swap = psutil.swap_memory()
        disk = psutil.disk_usage('/')
        load_avg = os.getloadavg()
        cpu_count = psutil.cpu_count()
        kernel = os.uname()

        return {
            'uptime': self.get_uptime(),
            'kernel': f"{kernel.sysname} {kernel.release}",
            'hostname': socket.gethostname(),
            'cpu_count': f"{cpu_count} cores",
            'load_avg': f"{load_avg[0]:.2f}, {load_avg[1]:.2f}, {load_avg[2]:.2f}",
            'memory_used': f"{mem.used//1024//1024} MB",
            'memory_total': f"{mem.total//1024//1024} MB",
            'memory_percent': f"{mem.percent:.1f}%",
            'swap_used': f"{swap.used//1024//1024} MB",
            'swap_total': f"{swap.total//1024//1024} MB",
            'disk_used': f"{disk.used//1024//1024//1024} GB",
            'disk_total': f"{disk.total//1024//1024//1024} GB",
            'disk_percent': f"{disk.percent:.1f}%",
            'process_count': len(psutil.pids()),
            'connections': len(psutil.net_connections()),
        }

class NativeDashboard(Adw.ApplicationWindow):
    """Main dashboard window with tabs"""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("9P.e Native System Monitor")
        self.set_default_size(1400, 900)

        self.monitor = SystemMonitor()

        # Main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_content(main_box)

        # Create notebook for tabs
        self.notebook = Gtk.Notebook()
        self.notebook.set_scrollable(True)
        main_box.append(self.notebook)

        # Tab 1: Overview
        self.create_overview_tab()

        # Tab 2: Performance
        self.create_performance_tab()

        # Tab 3: Processes
        self.create_processes_tab()

        # Tab 4: System Info
        self.create_system_info_tab()

        # Start update timer
        GLib.timeout_add(1000, self.update_all)
        self.update_all()  # Initial update

    def create_overview_tab(self):
        """Create overview tab with main charts"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>System Overview</span>")
        box.append(title)

        # Charts grid
        grid = Gtk.Grid()
        grid.set_row_spacing(10)
        grid.set_column_spacing(10)
        grid.set_hexpand(True)
        box.append(grid)

        # CPU Chart
        cpu_frame = Gtk.Frame()
        cpu_frame.set_label("CPU Usage")
        self.cpu_chart = CairoChart("cpu")
        self.cpu_chart.color = (0.2, 0.6, 1.0)
        cpu_frame.set_child(self.cpu_chart)
        grid.attach(cpu_frame, 0, 0, 1, 1)

        # Memory Chart
        mem_frame = Gtk.Frame()
        mem_frame.set_label("Memory Usage")
        self.mem_chart = CairoChart("memory")
        self.mem_chart.color = (0.8, 0.2, 0.8)
        mem_frame.set_child(self.mem_chart)
        grid.attach(mem_frame, 1, 0, 1, 1)

        # Network Chart
        net_frame = Gtk.Frame()
        net_frame.set_label("Network Activity")
        self.net_chart = CairoChart("network")
        self.net_chart.color = (0.2, 0.8, 0.2)
        net_frame.set_child(self.net_chart)
        grid.attach(net_frame, 0, 1, 1, 1)

        # Disk I/O Chart
        disk_frame = Gtk.Frame()
        disk_frame.set_label("Disk I/O")
        self.disk_chart = CairoChart("disk")
        self.disk_chart.color = (0.8, 0.6, 0.2)
        disk_frame.set_child(self.disk_chart)
        grid.attach(disk_frame, 1, 1, 1, 1)

        # Quick stats
        stats_frame = Gtk.Frame()
        stats_frame.set_label("Quick Stats")
        self.stats_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        self.stats_box.set_margin_start(10)
        self.stats_box.set_margin_end(10)
        self.stats_box.set_margin_top(10)
        self.stats_box.set_margin_bottom(10)
        stats_frame.set_child(self.stats_box)
        box.append(stats_frame)

        self.stats_labels = {}
        for key in ['uptime', 'processes', 'connections', 'load']:
            label = Gtk.Label()
            label.set_xalign(0)
            self.stats_labels[key] = label
            self.stats_box.append(label)

        self.notebook.append_page(box, Gtk.Label(label="Overview"))

    def create_performance_tab(self):
        """Create detailed performance tab"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # CPU details
        cpu_frame = Gtk.Frame()
        cpu_frame.set_label("CPU Details")
        cpu_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        cpu_box.set_margin_start(10)
        cpu_box.set_margin_end(10)
        cpu_box.set_margin_top(10)
        cpu_box.set_margin_bottom(10)

        # Per-CPU usage
        self.cpu_bars = []
        for i in range(psutil.cpu_count()):
            hbox = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            label = Gtk.Label(label=f"CPU {i}:")
            label.set_size_request(60, -1)
            label.set_xalign(0)
            hbox.append(label)

            bar = Gtk.ProgressBar()
            bar.set_hexpand(True)
            bar.set_show_text(True)
            self.cpu_bars.append(bar)
            hbox.append(bar)

            cpu_box.append(hbox)

        cpu_frame.set_child(cpu_box)
        box.append(cpu_frame)

        # Memory details
        mem_frame = Gtk.Frame()
        mem_frame.set_label("Memory Details")
        self.mem_details = Gtk.Label()
        self.mem_details.set_xalign(0)
        self.mem_details.set_margin_start(10)
        self.mem_details.set_margin_end(10)
        self.mem_details.set_margin_top(10)
        self.mem_details.set_margin_bottom(10)
        mem_frame.set_child(self.mem_details)
        box.append(mem_frame)

        self.notebook.append_page(box, Gtk.Label(label="Performance"))

    def create_processes_tab(self):
        """Create processes tab with full list"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Process count
        self.proc_count_label = Gtk.Label()
        self.proc_count_label.set_xalign(0)
        box.append(self.proc_count_label)

        # Process list
        self.process_list = ProcessListView()
        box.append(self.process_list)

        self.notebook.append_page(box, Gtk.Label(label="Processes"))

    def create_system_info_tab(self):
        """Create system information tab"""
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # System info display
        self.system_info_label = Gtk.Label()
        self.system_info_label.set_xalign(0)
        self.system_info_label.set_selectable(True)
        box.append(self.system_info_label)

        scroll.set_child(box)
        self.notebook.append_page(scroll, Gtk.Label(label="System Info"))

    def update_all(self):
        """Update all displays with real data"""
        # Update charts
        cpu = self.monitor.get_cpu_usage()
        mem = self.monitor.get_memory_usage()
        net = self.monitor.get_network_speed()
        disk = self.monitor.get_disk_io()

        self.cpu_chart.update_data(cpu)
        self.mem_chart.update_data(mem)
        self.net_chart.update_data(net)
        self.disk_chart.update_data(disk)

        # Update system info
        info = self.monitor.get_system_info()

        # Update overview stats
        self.stats_labels['uptime'].set_markup(f"<b>Uptime:</b> {info['uptime']}")
        self.stats_labels['processes'].set_markup(f"<b>Processes:</b> {info['process_count']}")
        self.stats_labels['connections'].set_markup(f"<b>Network Connections:</b> {info['connections']}")
        self.stats_labels['load'].set_markup(f"<b>Load Average:</b> {info['load_avg']}")

        # Update per-CPU bars
        cpu_percents = psutil.cpu_percent(percpu=True, interval=0)
        for i, bar in enumerate(self.cpu_bars):
            if i < len(cpu_percents):
                bar.set_fraction(cpu_percents[i] / 100.0)
                bar.set_text(f"{cpu_percents[i]:.1f}%")

        # Update memory details
        mem = psutil.virtual_memory()
        swap = psutil.swap_memory()
        self.mem_details.set_markup(
            f"<b>Physical Memory:</b>\n"
            f"  Used: {mem.used//1024//1024} MB / {mem.total//1024//1024} MB ({mem.percent:.1f}%)\n"
            f"  Available: {mem.available//1024//1024} MB\n"
            f"  Cached: {mem.cached//1024//1024 if hasattr(mem, 'cached') else 0} MB\n\n"
            f"<b>Swap Memory:</b>\n"
            f"  Used: {swap.used//1024//1024} MB / {swap.total//1024//1024} MB ({swap.percent:.1f}%)\n"
            f"  Free: {swap.free//1024//1024} MB"
        )

        # Update process list
        self.process_list.update_processes()
        self.proc_count_label.set_markup(f"<b>Total Processes: {info['process_count']}</b>")

        # Update system info tab
        self.system_info_label.set_markup(
            f"<b>System Information</b>\n\n"
            f"<b>Hostname:</b> {info['hostname']}\n"
            f"<b>Kernel:</b> {info['kernel']}\n"
            f"<b>Uptime:</b> {info['uptime']}\n"
            f"<b>CPU:</b> {info['cpu_count']}\n"
            f"<b>Load Average:</b> {info['load_avg']}\n\n"
            f"<b>Memory:</b>\n"
            f"  Physical: {info['memory_used']} / {info['memory_total']} ({info['memory_percent']})\n"
            f"  Swap: {info['swap_used']} / {info['swap_total']}\n\n"
            f"<b>Disk Usage (/):</b>\n"
            f"  Used: {info['disk_used']} / {info['disk_total']} ({info['disk_percent']})\n\n"
            f"<b>Network:</b>\n"
            f"  Active Connections: {info['connections']}\n\n"
            f"<b>Processes:</b> {info['process_count']}"
        )

        return True  # Continue timer

class NativeApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.native_monitor')

    def do_activate(self):
        win = NativeDashboard(application=self)
        win.present()

if __name__ == '__main__':
    app = NativeApp()
    app.run(None)