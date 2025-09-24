#!/usr/bin/env python3
"""GTK Dashboard focused on 9P.e Filesystem Monitoring"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, GLib, Gdk, Adw
import cairo
import math
import os
import psutil
import socket
from collections import deque, defaultdict
from datetime import datetime
import subprocess
import json
import time
from pathlib import Path

class FilesystemChart(Gtk.DrawingArea):
    """Filesystem-specific charts"""

    def __init__(self, chart_type="ops"):
        super().__init__()
        self.chart_type = chart_type
        self.data = deque(maxlen=60)
        self.max_value = 100
        self.color = (0.2, 0.8, 0.4)
        self.set_size_request(400, 200)
        self.set_draw_func(self.draw)

    def update_data(self, value):
        """Add new data point"""
        self.data.append(value)
        if self.data:
            self.max_value = max(max(self.data) * 1.2, 10)
        self.queue_draw()

    def draw(self, area, cr, width, height):
        """Render chart using Cairo"""
        # Background
        cr.set_source_rgb(0.05, 0.05, 0.1)
        cr.rectangle(0, 0, width, height)
        cr.fill()

        # Draw grid
        cr.set_source_rgba(0.3, 0.3, 0.4, 0.2)
        cr.set_line_width(1)

        # Grid lines
        for i in range(5):
            y = (height / 4) * i
            cr.move_to(0, y)
            cr.line_to(width, y)
            cr.stroke()

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

        # Fill area
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

        # Draw label
        if self.data:
            cr.set_source_rgb(1, 1, 1)
            cr.select_font_face("Sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
            cr.set_font_size(14)
            cr.move_to(10, 20)

            labels = {
                'ops': f"Operations/s: {self.data[-1]:.0f}",
                'reads': f"Reads/s: {self.data[-1]:.0f}",
                'writes': f"Writes/s: {self.data[-1]:.0f}",
                'connections': f"Active Connections: {self.data[-1]:.0f}",
                'bandwidth': f"Bandwidth: {self.data[-1]:.1f} KB/s",
                'cache': f"Cache Hit Rate: {self.data[-1]:.1f}%",
            }
            cr.show_text(labels.get(self.chart_type, f"Value: {self.data[-1]:.1f}"))

class FilesystemTreeView(Gtk.ScrolledWindow):
    """Tree view for filesystem structure"""

    def __init__(self):
        super().__init__()
        self.set_vexpand(True)
        self.set_hexpand(True)
        self.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        self.set_min_content_height(400)

        # Create tree store - Path, Type, Size, Permissions
        self.tree_store = Gtk.TreeStore(str, str, str, str)

        # Create tree view
        self.tree_view = Gtk.TreeView(model=self.tree_store)
        self.tree_view.set_enable_tree_lines(True)

        # Add columns
        for i, (title, width) in enumerate([
            ("Path", 300),
            ("Type", 100),
            ("Size", 100),
            ("Permissions", 100)
        ]):
            renderer = Gtk.CellRendererText()
            column = Gtk.TreeViewColumn(title, renderer, text=i)
            column.set_resizable(True)
            column.set_min_width(width)
            self.tree_view.append_column(column)

        self.set_child(self.tree_view)

    def update_filesystem(self, root_path="/tmp"):
        """Update with filesystem structure"""
        self.tree_store.clear()

        # Add synthetic filesystem entries
        synthetic_root = self.tree_store.append(None, ["/synthetic", "directory", "-", "dr-xr-xr-x"])

        # System monitoring synthetic files
        sys_folder = self.tree_store.append(synthetic_root, ["/synthetic/sys", "directory", "-", "dr-xr-xr-x"])
        self.tree_store.append(sys_folder, ["cpu", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(sys_folder, ["memory", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(sys_folder, ["network", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(sys_folder, ["processes", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(sys_folder, ["uptime", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(sys_folder, ["load", "file", "dynamic", "-r--r--r--"])

        # AI synthetic files
        ai_folder = self.tree_store.append(synthetic_root, ["/synthetic/ai", "directory", "-", "dr-xr-xr-x"])
        self.tree_store.append(ai_folder, ["chat", "file", "dynamic", "-rw-rw-rw-"])
        self.tree_store.append(ai_folder, ["summarize", "file", "dynamic", "-rw-rw-rw-"])
        self.tree_store.append(ai_folder, ["translate", "file", "dynamic", "-rw-rw-rw-"])

        # WASM synthetic files
        wasm_folder = self.tree_store.append(synthetic_root, ["/synthetic/wasm", "directory", "-", "dr-xr-xr-x"])
        self.tree_store.append(wasm_folder, ["execute", "file", "dynamic", "-rwxrwxrwx"])
        self.tree_store.append(wasm_folder, ["compile", "file", "dynamic", "-rwxrwxrwx"])

        # Stats synthetic files
        stats_folder = self.tree_store.append(synthetic_root, ["/synthetic/stats", "directory", "-", "dr-xr-xr-x"])
        self.tree_store.append(stats_folder, ["reads", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(stats_folder, ["writes", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(stats_folder, ["operations", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(stats_folder, ["connections", "file", "dynamic", "-r--r--r--"])
        self.tree_store.append(stats_folder, ["errors", "file", "dynamic", "-r--r--r--"])

        # Add real filesystem entries
        real_root = self.tree_store.append(None, [root_path, "directory", "-", "drwxrwxrwx"])
        self._add_directory_contents(real_root, root_path, max_depth=2)

    def _add_directory_contents(self, parent, path, depth=0, max_depth=2):
        """Recursively add directory contents"""
        if depth >= max_depth:
            return

        try:
            path_obj = Path(path)
            for item in sorted(path_obj.iterdir())[:20]:  # Limit entries
                try:
                    stat = item.stat()
                    size = f"{stat.st_size}" if item.is_file() else "-"
                    perms = oct(stat.st_mode)[-3:]

                    if item.is_dir():
                        node = self.tree_store.append(parent, [item.name, "directory", size, f"d{perms}"])
                        # Don't recurse into hidden directories
                        if not item.name.startswith('.'):
                            self._add_directory_contents(node, str(item), depth + 1, max_depth)
                    else:
                        self.tree_store.append(parent, [item.name, "file", size, f"-{perms}"])
                except (PermissionError, OSError):
                    continue
        except (PermissionError, OSError):
            pass

class FilesystemMonitor:
    """Monitor filesystem operations and metrics"""

    def __init__(self):
        self.operation_counts = defaultdict(int)
        self.last_disk_io = psutil.disk_io_counters()
        self.last_check = time.time()
        self.synthetic_stats = {
            'reads': 0,
            'writes': 0,
            'operations': 0,
            'cache_hits': 0,
            'cache_misses': 0,
        }
        self.connections = []

    def update_stats(self):
        """Update filesystem statistics"""
        current_time = time.time()
        time_delta = current_time - self.last_check
        self.last_check = current_time

        # Get disk I/O stats
        current_io = psutil.disk_io_counters()
        if self.last_disk_io and time_delta > 0:
            reads_per_sec = (current_io.read_count - self.last_disk_io.read_count) / time_delta
            writes_per_sec = (current_io.write_count - self.last_disk_io.write_count) / time_delta
            read_bytes_per_sec = (current_io.read_bytes - self.last_disk_io.read_bytes) / time_delta
            write_bytes_per_sec = (current_io.write_bytes - self.last_disk_io.write_bytes) / time_delta
        else:
            reads_per_sec = writes_per_sec = read_bytes_per_sec = write_bytes_per_sec = 0

        self.last_disk_io = current_io

        # Simulate synthetic file operations (would be real metrics in production)
        self.synthetic_stats['reads'] += reads_per_sec
        self.synthetic_stats['writes'] += writes_per_sec
        self.synthetic_stats['operations'] = reads_per_sec + writes_per_sec

        # Calculate cache hit rate (simulated)
        total_ops = self.synthetic_stats['reads'] + self.synthetic_stats['writes']
        if total_ops > 0:
            cache_hit_rate = min(85 + (total_ops % 10), 95)  # 85-95% hit rate
        else:
            cache_hit_rate = 0

        # Check for 9P connections on known ports
        active_connections = 0
        try:
            for conn in psutil.net_connections():
                if conn.laddr and conn.laddr.port in [5640, 5641, 5645, 5646, 5647, 9641, 9999]:
                    if conn.status == 'ESTABLISHED':
                        active_connections += 1
        except (psutil.AccessDenied, AttributeError):
            pass

        return {
            'reads_per_sec': reads_per_sec,
            'writes_per_sec': writes_per_sec,
            'ops_per_sec': reads_per_sec + writes_per_sec,
            'read_bandwidth': read_bytes_per_sec / 1024,  # KB/s
            'write_bandwidth': write_bytes_per_sec / 1024,  # KB/s
            'total_bandwidth': (read_bytes_per_sec + write_bytes_per_sec) / 1024,
            'cache_hit_rate': cache_hit_rate,
            'active_connections': active_connections,
            'disk_usage': psutil.disk_usage('/'),
        }

    def get_mount_points(self):
        """Get all mount points and their usage"""
        mounts = []
        for partition in psutil.disk_partitions():
            try:
                usage = psutil.disk_usage(partition.mountpoint)
                mounts.append({
                    'device': partition.device,
                    'mountpoint': partition.mountpoint,
                    'fstype': partition.fstype,
                    'total': usage.total,
                    'used': usage.used,
                    'free': usage.free,
                    'percent': usage.percent,
                })
            except (PermissionError, OSError):
                continue
        return mounts

class FilesystemDashboard(Adw.ApplicationWindow):
    """Main filesystem monitoring dashboard"""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("9P.e Filesystem Monitor")
        self.set_default_size(1400, 900)

        self.monitor = FilesystemMonitor()

        # Main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_content(main_box)

        # Create notebook for tabs
        self.notebook = Gtk.Notebook()
        self.notebook.set_scrollable(True)
        main_box.append(self.notebook)

        # Tab 1: Filesystem Overview
        self.create_overview_tab()

        # Tab 2: Operations
        self.create_operations_tab()

        # Tab 3: Filesystem Tree
        self.create_tree_tab()

        # Tab 4: Mount Points
        self.create_mounts_tab()

        # Tab 5: Synthetic Files
        self.create_synthetic_tab()

        # Start update timer
        GLib.timeout_add(1000, self.update_all)
        self.update_all()  # Initial update

    def create_overview_tab(self):
        """Create filesystem overview tab"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>9P.e Filesystem Overview</span>")
        box.append(title)

        # Charts grid
        grid = Gtk.Grid()
        grid.set_row_spacing(10)
        grid.set_column_spacing(10)
        grid.set_hexpand(True)
        box.append(grid)

        # Operations/s Chart
        ops_frame = Gtk.Frame()
        ops_frame.set_label("Operations per Second")
        self.ops_chart = FilesystemChart("ops")
        self.ops_chart.color = (0.2, 0.8, 0.4)
        ops_frame.set_child(self.ops_chart)
        grid.attach(ops_frame, 0, 0, 1, 1)

        # Bandwidth Chart
        bw_frame = Gtk.Frame()
        bw_frame.set_label("I/O Bandwidth")
        self.bandwidth_chart = FilesystemChart("bandwidth")
        self.bandwidth_chart.color = (0.8, 0.4, 0.2)
        bw_frame.set_child(self.bandwidth_chart)
        grid.attach(bw_frame, 1, 0, 1, 1)

        # Connections Chart
        conn_frame = Gtk.Frame()
        conn_frame.set_label("9P Connections")
        self.connections_chart = FilesystemChart("connections")
        self.connections_chart.color = (0.2, 0.6, 1.0)
        conn_frame.set_child(self.connections_chart)
        grid.attach(conn_frame, 0, 1, 1, 1)

        # Cache Hit Rate Chart
        cache_frame = Gtk.Frame()
        cache_frame.set_label("Cache Performance")
        self.cache_chart = FilesystemChart("cache")
        self.cache_chart.color = (0.8, 0.2, 0.8)
        cache_frame.set_child(self.cache_chart)
        grid.attach(cache_frame, 1, 1, 1, 1)

        # Stats box
        stats_frame = Gtk.Frame()
        stats_frame.set_label("Filesystem Statistics")
        self.stats_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
        self.stats_box.set_margin_start(10)
        self.stats_box.set_margin_end(10)
        self.stats_box.set_margin_top(10)
        self.stats_box.set_margin_bottom(10)
        stats_frame.set_child(self.stats_box)
        box.append(stats_frame)

        self.stats_labels = {}
        for key in ['total_ops', 'disk_usage', 'inode_usage', 'largest_file']:
            label = Gtk.Label()
            label.set_xalign(0)
            self.stats_labels[key] = label
            self.stats_box.append(label)

        self.notebook.append_page(box, Gtk.Label(label="Overview"))

    def create_operations_tab(self):
        """Create detailed operations tab"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>Filesystem Operations</span>")
        box.append(title)

        # Operation charts
        grid = Gtk.Grid()
        grid.set_row_spacing(10)
        grid.set_column_spacing(10)
        grid.set_hexpand(True)
        box.append(grid)

        # Reads Chart
        reads_frame = Gtk.Frame()
        reads_frame.set_label("Read Operations")
        self.reads_chart = FilesystemChart("reads")
        self.reads_chart.color = (0.2, 0.8, 0.2)
        reads_frame.set_child(self.reads_chart)
        grid.attach(reads_frame, 0, 0, 1, 1)

        # Writes Chart
        writes_frame = Gtk.Frame()
        writes_frame.set_label("Write Operations")
        self.writes_chart = FilesystemChart("writes")
        self.writes_chart.color = (0.8, 0.2, 0.2)
        writes_frame.set_child(self.writes_chart)
        grid.attach(writes_frame, 1, 0, 1, 1)

        # Operation details
        details_frame = Gtk.Frame()
        details_frame.set_label("Operation Details")
        self.ops_details = Gtk.Label()
        self.ops_details.set_xalign(0)
        self.ops_details.set_margin_start(10)
        self.ops_details.set_margin_end(10)
        self.ops_details.set_margin_top(10)
        self.ops_details.set_margin_bottom(10)
        details_frame.set_child(self.ops_details)
        box.append(details_frame)

        self.notebook.append_page(box, Gtk.Label(label="Operations"))

    def create_tree_tab(self):
        """Create filesystem tree tab"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>Filesystem Structure</span>")
        box.append(title)

        # Tree view
        self.tree_view = FilesystemTreeView()
        box.append(self.tree_view)

        self.notebook.append_page(box, Gtk.Label(label="Filesystem"))

    def create_mounts_tab(self):
        """Create mount points tab"""
        scroll = Gtk.ScrolledWindow()
        scroll.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)

        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>Mount Points</span>")
        box.append(title)

        # Mount points list
        self.mounts_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.append(self.mounts_box)

        scroll.set_child(box)
        self.notebook.append_page(scroll, Gtk.Label(label="Mounts"))

    def create_synthetic_tab(self):
        """Create synthetic files tab"""
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        box.set_margin_start(10)
        box.set_margin_end(10)
        box.set_margin_top(10)
        box.set_margin_bottom(10)

        # Title
        title = Gtk.Label()
        title.set_markup("<span size='large' weight='bold'>9P.e Synthetic Filesystem</span>")
        box.append(title)

        # Synthetic file categories
        categories = [
            ("System Monitoring", [
                "/synthetic/sys/cpu - Current CPU usage",
                "/synthetic/sys/memory - Memory statistics",
                "/synthetic/sys/network - Network activity",
                "/synthetic/sys/processes - Process list",
                "/synthetic/sys/uptime - System uptime",
                "/synthetic/sys/load - Load average",
            ]),
            ("AI Services", [
                "/synthetic/ai/chat - Interactive chat interface",
                "/synthetic/ai/summarize - Text summarization",
                "/synthetic/ai/translate - Language translation",
            ]),
            ("WASM Runtime", [
                "/synthetic/wasm/execute - Execute WebAssembly modules",
                "/synthetic/wasm/compile - Compile to WebAssembly",
            ]),
            ("Statistics", [
                "/synthetic/stats/reads - Read operation count",
                "/synthetic/stats/writes - Write operation count",
                "/synthetic/stats/operations - Total operations",
                "/synthetic/stats/connections - Active connections",
                "/synthetic/stats/errors - Error count",
            ]),
        ]

        for category_name, files in categories:
            frame = Gtk.Frame()
            frame.set_label(category_name)

            list_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
            list_box.set_margin_start(10)
            list_box.set_margin_end(10)
            list_box.set_margin_top(10)
            list_box.set_margin_bottom(10)

            for file_desc in files:
                label = Gtk.Label(label=file_desc)
                label.set_xalign(0)
                list_box.append(label)

            frame.set_child(list_box)
            box.append(frame)

        self.notebook.append_page(box, Gtk.Label(label="Synthetic"))

    def update_all(self):
        """Update all displays"""
        # Get filesystem stats
        stats = self.monitor.update_stats()

        # Update charts
        self.ops_chart.update_data(stats['ops_per_sec'])
        self.bandwidth_chart.update_data(stats['total_bandwidth'])
        self.connections_chart.update_data(stats['active_connections'])
        self.cache_chart.update_data(stats['cache_hit_rate'])
        self.reads_chart.update_data(stats['reads_per_sec'])
        self.writes_chart.update_data(stats['writes_per_sec'])

        # Update stats
        disk = stats['disk_usage']
        self.stats_labels['total_ops'].set_markup(
            f"<b>Total Operations:</b> {self.monitor.synthetic_stats['operations']:.0f}"
        )
        self.stats_labels['disk_usage'].set_markup(
            f"<b>Disk Usage:</b> {disk.used//1024//1024//1024} GB / {disk.total//1024//1024//1024} GB ({disk.percent:.1f}%)"
        )

        # Get inode usage
        try:
            statvfs = os.statvfs('/')
            inode_total = statvfs.f_files
            inode_free = statvfs.f_favail
            inode_used = inode_total - inode_free
            inode_percent = (inode_used / inode_total * 100) if inode_total > 0 else 0
            self.stats_labels['inode_usage'].set_markup(
                f"<b>Inode Usage:</b> {inode_used:,} / {inode_total:,} ({inode_percent:.1f}%)"
            )
        except:
            self.stats_labels['inode_usage'].set_markup("<b>Inode Usage:</b> N/A")

        # Find largest file (simplified)
        self.stats_labels['largest_file'].set_markup(
            f"<b>Active 9P.e Connections:</b> {stats['active_connections']}"
        )

        # Update operation details
        self.ops_details.set_markup(
            f"<b>Read Operations:</b> {stats['reads_per_sec']:.1f} ops/sec\n"
            f"<b>Write Operations:</b> {stats['writes_per_sec']:.1f} ops/sec\n"
            f"<b>Read Bandwidth:</b> {stats['read_bandwidth']:.1f} KB/s\n"
            f"<b>Write Bandwidth:</b> {stats['write_bandwidth']:.1f} KB/s\n"
            f"<b>Total I/O:</b> {stats['total_bandwidth']:.1f} KB/s\n"
            f"<b>Cache Hit Rate:</b> {stats['cache_hit_rate']:.1f}%\n"
            f"<b>Active Connections:</b> {stats['active_connections']}"
        )

        # Update filesystem tree
        if hasattr(self, '_tree_updated'):
            self._tree_updated += 1
        else:
            self._tree_updated = 0

        # Update tree every 10 seconds
        if self._tree_updated % 10 == 0:
            self.tree_view.update_filesystem("/tmp")

        # Update mount points
        self._update_mounts()

        return True  # Continue timer

    def _update_mounts(self):
        """Update mount points display"""
        # Clear existing
        for child in list(self.mounts_box):
            self.mounts_box.remove(child)

        # Add mount points
        for mount in self.monitor.get_mount_points():
            frame = Gtk.Frame()
            frame.set_label(mount['mountpoint'])

            info_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=5)
            info_box.set_margin_start(10)
            info_box.set_margin_end(10)
            info_box.set_margin_top(10)
            info_box.set_margin_bottom(10)

            # Device info
            device_label = Gtk.Label()
            device_label.set_markup(f"<b>Device:</b> {mount['device']}")
            device_label.set_xalign(0)
            info_box.append(device_label)

            # Filesystem type
            fs_label = Gtk.Label()
            fs_label.set_markup(f"<b>Type:</b> {mount['fstype']}")
            fs_label.set_xalign(0)
            info_box.append(fs_label)

            # Usage bar
            usage_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=10)
            usage_label = Gtk.Label(label="Usage:")
            usage_label.set_size_request(60, -1)
            usage_box.append(usage_label)

            usage_bar = Gtk.ProgressBar()
            usage_bar.set_fraction(mount['percent'] / 100.0)
            usage_bar.set_text(f"{mount['percent']:.1f}%")
            usage_bar.set_show_text(True)
            usage_bar.set_hexpand(True)
            usage_box.append(usage_bar)
            info_box.append(usage_box)

            # Size info
            size_label = Gtk.Label()
            size_label.set_markup(
                f"<b>Space:</b> {mount['used']//1024//1024//1024} GB / {mount['total']//1024//1024//1024} GB "
                f"({mount['free']//1024//1024//1024} GB free)"
            )
            size_label.set_xalign(0)
            info_box.append(size_label)

            frame.set_child(info_box)
            self.mounts_box.append(frame)

class FilesystemApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.filesystem_monitor')

    def do_activate(self):
        win = FilesystemDashboard(application=self)
        win.present()

if __name__ == '__main__':
    app = FilesystemApp()
    app.run(None)