#!/usr/bin/env python3
"""Pure Gaussian Splatting Display System - EVERYTHING is rendered with Gaussians"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('cairo', '1.0')
from gi.repository import Gtk, GLib, Gdk
import cairo
import math
import os
import psutil
import numpy as np
from collections import deque
from datetime import datetime

class GaussianTextRenderer:
    """Render text using Gaussian splats"""

    # Simplified character maps - each char defined by Gaussian positions
    CHAR_MAPS = {
        'A': [(0.5, 0.0), (0.25, 0.5), (0.75, 0.5), (0.0, 1.0), (1.0, 1.0), (0.25, 0.5), (0.75, 0.5)],
        'B': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.5, 0.0), (0.5, 0.5), (0.5, 1.0), (0.7, 0.25), (0.7, 0.75)],
        'C': [(0.7, 0.0), (0.0, 0.0), (0.0, 1.0), (0.7, 1.0)],
        'D': [(0.0, 0.0), (0.0, 1.0), (0.6, 0.2), (0.6, 0.8), (0.8, 0.5)],
        'E': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.7, 0.0), (0.5, 0.5), (0.7, 1.0)],
        'F': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.7, 0.0), (0.5, 0.5)],
        'G': [(0.7, 0.0), (0.0, 0.0), (0.0, 1.0), (0.7, 1.0), (0.7, 0.6), (0.4, 0.6)],
        'H': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0), (0.5, 0.5)],
        'I': [(0.5, 0.0), (0.5, 0.5), (0.5, 1.0)],
        'K': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.8, 0.0), (0.4, 0.5), (0.8, 1.0)],
        'L': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.7, 1.0)],
        'M': [(0.0, 1.0), (0.0, 0.5), (0.0, 0.0), (0.5, 0.3), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0)],
        'N': [(0.0, 1.0), (0.0, 0.0), (1.0, 1.0), (1.0, 0.0)],
        'O': [(0.5, 0.0), (0.0, 0.5), (0.5, 1.0), (1.0, 0.5)],
        'P': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.7, 0.0), (0.7, 0.4), (0.4, 0.5)],
        'R': [(0.0, 0.0), (0.0, 0.5), (0.0, 1.0), (0.7, 0.0), (0.7, 0.4), (0.4, 0.5), (0.8, 1.0)],
        'S': [(0.7, 0.0), (0.0, 0.2), (0.3, 0.5), (0.7, 0.8), (0.0, 1.0)],
        'T': [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (0.5, 0.5), (0.5, 1.0)],
        'U': [(0.0, 0.0), (0.0, 0.5), (0.0, 0.8), (0.3, 1.0), (0.7, 1.0), (1.0, 0.8), (1.0, 0.5), (1.0, 0.0)],
        'V': [(0.0, 0.0), (0.25, 0.5), (0.5, 1.0), (0.75, 0.5), (1.0, 0.0)],
        'W': [(0.0, 0.0), (0.2, 1.0), (0.5, 0.5), (0.8, 1.0), (1.0, 0.0)],
        'X': [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (1.0, 0.0), (0.0, 1.0)],
        'Y': [(0.0, 0.0), (0.5, 0.5), (1.0, 0.0), (0.5, 1.0)],
        ' ': [],
        '.': [(0.5, 0.9)],
        ':': [(0.5, 0.3), (0.5, 0.7)],
        '/': [(0.8, 0.0), (0.2, 1.0)],
        '-': [(0.2, 0.5), (0.8, 0.5)],
        '0': [(0.5, 0.0), (0.0, 0.5), (0.5, 1.0), (1.0, 0.5)],
        '1': [(0.3, 0.2), (0.5, 0.0), (0.5, 0.5), (0.5, 1.0)],
        '2': [(0.0, 0.2), (0.5, 0.0), (0.8, 0.3), (0.0, 0.7), (0.8, 1.0)],
        '3': [(0.0, 0.0), (0.8, 0.2), (0.4, 0.5), (0.8, 0.8), (0.0, 1.0)],
        '4': [(0.7, 0.0), (0.7, 0.5), (0.7, 1.0), (0.0, 0.6), (1.0, 0.6)],
        '5': [(0.8, 0.0), (0.0, 0.0), (0.0, 0.4), (0.8, 0.5), (0.8, 0.8), (0.0, 1.0)],
        '6': [(0.7, 0.0), (0.0, 0.3), (0.0, 0.7), (0.7, 1.0), (0.7, 0.6), (0.3, 0.5)],
        '7': [(0.0, 0.0), (0.8, 0.0), (0.4, 0.5), (0.2, 1.0)],
        '8': [(0.5, 0.0), (0.0, 0.25), (0.5, 0.5), (0.0, 0.75), (0.5, 1.0), (1.0, 0.75), (1.0, 0.25)],
        '9': [(0.3, 1.0), (0.8, 0.7), (0.8, 0.3), (0.3, 0.0), (0.3, 0.4), (0.7, 0.5)],
        '%': [(0.2, 0.2), (0.8, 0.8), (0.8, 0.0), (0.2, 1.0)],
    }

    @staticmethod
    def render_text(renderer, text, x, y, size=0.015, color=(1, 1, 1), intensity=0.8):
        """Render text string using Gaussian splats"""
        char_spacing = size * 2.5

        for i, char in enumerate(text.upper()):
            if char in GaussianTextRenderer.CHAR_MAPS:
                char_x = x + i * char_spacing
                points = GaussianTextRenderer.CHAR_MAPS[char]

                for px, py in points:
                    renderer.add_gaussian(
                        char_x + px * size * 1.5,
                        y + py * size * 2,
                        size * 0.4,
                        size * 0.4,
                        0,
                        intensity,
                        color
                    )

class PureGaussianRenderer:
    """Core Gaussian renderer for EVERYTHING"""

    def __init__(self, width, height):
        self.width = width
        self.height = height
        self.gaussians = []

    def clear(self):
        self.gaussians = []

    def add_gaussian(self, x, y, sx, sy, rotation, intensity, color):
        """Add a single Gaussian splat"""
        self.gaussians.append({
            'x': x, 'y': y,
            'sx': sx, 'sy': sy,
            'rotation': rotation,
            'intensity': intensity,
            'color': color
        })

    def draw_tab(self, x, y, width, height, label, active=False):
        """Draw a tab using Gaussians"""
        # Tab background - dense grid of Gaussians
        intensity = 0.9 if active else 0.5
        color = (0.3, 0.4, 0.8) if active else (0.2, 0.2, 0.3)

        # Fill with Gaussians
        for gx in np.linspace(x, x + width, int(width * 100)):
            for gy in np.linspace(y, y + height, int(height * 100)):
                self.add_gaussian(gx, gy, 0.01, 0.01, 0, intensity * 0.5, color)

        # Border
        for gx in np.linspace(x, x + width, int(width * 50)):
            self.add_gaussian(gx, y, 0.005, 0.005, 0, intensity, (0.8, 0.8, 1.0))
            self.add_gaussian(gx, y + height, 0.005, 0.005, 0, intensity, (0.8, 0.8, 1.0))

        for gy in np.linspace(y, y + height, int(height * 50)):
            self.add_gaussian(x, gy, 0.005, 0.005, 0, intensity, (0.8, 0.8, 1.0))
            self.add_gaussian(x + width, gy, 0.005, 0.005, 0, intensity, (0.8, 0.8, 1.0))

        # Text
        text_color = (1, 1, 1) if active else (0.7, 0.7, 0.7)
        GaussianTextRenderer.render_text(self, label, x + 0.02, y + 0.01, 0.012, text_color, intensity)

    def draw_frame(self, x, y, width, height, thickness=0.002):
        """Draw a frame/border using Gaussians"""
        # Top and bottom
        for gx in np.linspace(x, x + width, int(width * 200)):
            self.add_gaussian(gx, y, thickness, thickness, 0, 0.7, (0.5, 0.5, 0.6))
            self.add_gaussian(gx, y + height, thickness, thickness, 0, 0.7, (0.5, 0.5, 0.6))

        # Left and right
        for gy in np.linspace(y, y + height, int(height * 200)):
            self.add_gaussian(x, gy, thickness, thickness, 0, 0.7, (0.5, 0.5, 0.6))
            self.add_gaussian(x + width, gy, thickness, thickness, 0, 0.7, (0.5, 0.5, 0.6))

    def draw_line_chart(self, x, y, width, height, data, max_val, color):
        """Draw a line chart using Gaussians"""
        if not data:
            return

        # Grid lines
        for i in range(5):
            gy = y + (i / 4) * height
            for gx in np.linspace(x, x + width, 50):
                self.add_gaussian(gx, gy, 0.003, 0.003, 0, 0.2, (0.3, 0.3, 0.4))

        # Data points and connecting lines
        for i, value in enumerate(data):
            if max_val > 0:
                px = x + (i / max(1, len(data) - 1)) * width
                py = y + height - (value / max_val) * height

                # Data point
                self.add_gaussian(px, py, 0.008, 0.008, 0, 0.9, color)

                # Connect to previous point
                if i > 0:
                    prev_val = data[i-1]
                    prev_px = x + ((i-1) / max(1, len(data) - 1)) * width
                    prev_py = y + height - (prev_val / max_val) * height

                    # Interpolate
                    steps = 20
                    for j in range(steps):
                        t = j / steps
                        ipx = prev_px + (px - prev_px) * t
                        ipy = prev_py + (py - prev_py) * t
                        self.add_gaussian(ipx, ipy, 0.005, 0.005, 0, 0.7, color)

    def draw_process_row(self, x, y, width, proc, row_num):
        """Draw a process list row using Gaussians"""
        # Alternating background
        if row_num % 2 == 0:
            for gx in np.linspace(x, x + width, 30):
                for gy in np.linspace(y, y + 0.025, 3):
                    self.add_gaussian(gx, gy, 0.02, 0.01, 0, 0.1, (0.2, 0.2, 0.3))

        # Process data
        GaussianTextRenderer.render_text(self, str(proc['pid']), x + 0.01, y + 0.005, 0.008, (0.7, 0.7, 0.9))
        GaussianTextRenderer.render_text(self, proc['name'][:15], x + 0.08, y + 0.005, 0.008, (0.9, 0.9, 0.9))
        GaussianTextRenderer.render_text(self, f"{proc['cpu']:.1f}", x + 0.25, y + 0.005, 0.008, (0.7, 1.0, 0.7))
        GaussianTextRenderer.render_text(self, f"{proc['memory']:.1f}", x + 0.32, y + 0.005, 0.008, (1.0, 0.7, 1.0))

    def render(self, cr):
        """Render all Gaussians"""
        for g in self.gaussians:
            cr.save()

            px = g['x'] * self.width
            py = g['y'] * self.height
            sx = g['sx'] * self.width
            sy = g['sy'] * self.height

            cr.translate(px, py)
            cr.rotate(g['rotation'])

            # Gaussian gradient
            gradient = cairo.RadialGradient(0, 0, 0, 0, 0, max(sx, sy))
            r, g_val, b = g['color']
            i = g['intensity']

            gradient.add_color_stop_rgba(0, r*i, g_val*i, b*i, 0.9)
            gradient.add_color_stop_rgba(0.368, r*i*0.6, g_val*i*0.6, b*i*0.6, 0.6)  # exp(-0.5) ≈ 0.6
            gradient.add_color_stop_rgba(0.606, r*i*0.3, g_val*i*0.3, b*i*0.3, 0.3)  # exp(-2) ≈ 0.135
            gradient.add_color_stop_rgba(1.0, 0, 0, 0, 0)

            cr.set_source(gradient)
            cr.scale(sx, sy)
            cr.arc(0, 0, 1, 0, 2 * math.pi)
            cr.fill()

            cr.restore()

class SystemMonitor:
    """Real system data provider"""

    def __init__(self):
        self.cpu_history = deque(maxlen=60)
        self.mem_history = deque(maxlen=60)
        self.net_history = deque(maxlen=60)
        self.last_net = psutil.net_io_counters()
        self.boot_time = psutil.boot_time()

    def update(self):
        # CPU
        self.cpu_history.append(psutil.cpu_percent(interval=0))

        # Memory
        mem = psutil.virtual_memory()
        self.mem_history.append(mem.percent)

        # Network
        net = psutil.net_io_counters()
        bytes_sec = (net.bytes_sent + net.bytes_recv -
                     self.last_net.bytes_sent - self.last_net.bytes_recv) / 1024
        self.net_history.append(min(bytes_sec, 10000))  # Cap at 10MB/s
        self.last_net = net

        # Processes
        procs = []
        for p in psutil.process_iter(['pid', 'name', 'cpu_percent', 'memory_info']):
            try:
                info = p.info
                procs.append({
                    'pid': info['pid'],
                    'name': info['name'][:20],
                    'cpu': info['cpu_percent'] or 0,
                    'memory': info['memory_info'].rss / 1024 / 1024 if info['memory_info'] else 0
                })
            except:
                continue

        return {
            'cpu': list(self.cpu_history),
            'memory': list(self.mem_history),
            'network': list(self.net_history),
            'processes': sorted(procs, key=lambda x: x['cpu'], reverse=True),
            'uptime': self.get_uptime(),
            'load': os.getloadavg(),
            'mem_info': mem
        }

    def get_uptime(self):
        seconds = datetime.now().timestamp() - self.boot_time
        days = int(seconds // 86400)
        hours = int((seconds % 86400) // 3600)
        mins = int((seconds % 3600) // 60)
        return f"{days}D {hours}H {mins}M"

class PureGaussianDashboard(Gtk.ApplicationWindow):
    """Everything rendered with Gaussian splatting"""

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("Pure Gaussian Splatting System Monitor")
        self.set_default_size(1600, 900)

        self.monitor = SystemMonitor()
        self.active_tab = 0
        self.process_scroll = 0

        # Single drawing area for EVERYTHING
        self.drawing_area = Gtk.DrawingArea()
        self.drawing_area.set_draw_func(self.draw_everything)
        self.set_child(self.drawing_area)

        # Handle clicks for tabs
        gesture = Gtk.GestureClick()
        gesture.connect("pressed", self.on_click)
        self.drawing_area.add_controller(gesture)

        # Keyboard for scrolling
        key_controller = Gtk.EventControllerKey()
        key_controller.connect("key-pressed", self.on_key_press)
        self.add_controller(key_controller)

        # Update timer
        GLib.timeout_add(1000, self.update_data)
        self.update_data()

    def on_click(self, gesture, n_press, x, y):
        """Handle tab clicks"""
        # Check if click is in tab area (top 40 pixels)
        if y < 40:
            tab_width = 150
            tab_index = int(x / tab_width)
            if tab_index < 4:
                self.active_tab = tab_index
                self.drawing_area.queue_draw()

    def on_key_press(self, controller, keyval, keycode, state):
        """Handle keyboard for scrolling"""
        if keyval == Gdk.KEY_Up:
            self.process_scroll = max(0, self.process_scroll - 1)
            self.drawing_area.queue_draw()
        elif keyval == Gdk.KEY_Down:
            self.process_scroll += 1
            self.drawing_area.queue_draw()

    def update_data(self):
        self.data = self.monitor.update()
        self.drawing_area.queue_draw()
        return True

    def draw_everything(self, area, cr, width, height):
        """Draw EVERYTHING using only Gaussian splatting"""

        # Create renderer
        renderer = PureGaussianRenderer(width, height)

        # Black background - even this is Gaussians!
        for x in np.linspace(0, 1, 20):
            for y in np.linspace(0, 1, 20):
                renderer.add_gaussian(x, y, 0.1, 0.1, 0, 0.3, (0.05, 0.05, 0.1))

        # Draw tabs at top
        tab_labels = ["DASHBOARD", "PROCESSES", "NETWORK", "SYSTEM"]
        tab_y = 0.005
        tab_height = 0.04
        tab_width = 0.15

        for i, label in enumerate(tab_labels):
            renderer.draw_tab(
                0.01 + i * (tab_width + 0.01),
                tab_y,
                tab_width,
                tab_height,
                label,
                active=(i == self.active_tab)
            )

        # Draw content based on active tab
        content_y = 0.06

        if self.active_tab == 0:  # Dashboard
            self.draw_dashboard(renderer, content_y)
        elif self.active_tab == 1:  # Processes
            self.draw_processes(renderer, content_y)
        elif self.active_tab == 2:  # Network
            self.draw_network(renderer, content_y)
        elif self.active_tab == 3:  # System
            self.draw_system(renderer, content_y)

        # Render everything
        renderer.render(cr)

    def draw_dashboard(self, renderer, y_start):
        """Draw dashboard tab - all Gaussians"""
        if not hasattr(self, 'data'):
            return

        # Title
        GaussianTextRenderer.render_text(renderer, "SYSTEM DASHBOARD", 0.35, y_start, 0.02, (1, 1, 1))

        # CPU Chart
        renderer.draw_frame(0.05, y_start + 0.08, 0.42, 0.25)
        GaussianTextRenderer.render_text(renderer, "CPU USAGE", 0.06, y_start + 0.09, 0.012, (0.7, 1, 0.7))
        renderer.draw_line_chart(0.06, y_start + 0.12, 0.4, 0.2, self.data['cpu'], 100, (0.2, 1, 0.2))
        if self.data['cpu']:
            GaussianTextRenderer.render_text(renderer, f"{self.data['cpu'][-1]:.1f}%", 0.38, y_start + 0.11, 0.015, (0.5, 1, 0.5))

        # Memory Chart
        renderer.draw_frame(0.52, y_start + 0.08, 0.42, 0.25)
        GaussianTextRenderer.render_text(renderer, "MEMORY USAGE", 0.53, y_start + 0.09, 0.012, (1, 0.7, 1))
        renderer.draw_line_chart(0.53, y_start + 0.12, 0.4, 0.2, self.data['memory'], 100, (1, 0.3, 1))
        if self.data['memory']:
            GaussianTextRenderer.render_text(renderer, f"{self.data['memory'][-1]:.1f}%", 0.85, y_start + 0.11, 0.015, (1, 0.5, 1))

        # Network Chart
        renderer.draw_frame(0.05, y_start + 0.35, 0.42, 0.25)
        GaussianTextRenderer.render_text(renderer, "NETWORK KB/S", 0.06, y_start + 0.36, 0.012, (0.7, 0.7, 1))
        max_net = max(self.data['network']) if self.data['network'] else 1000
        renderer.draw_line_chart(0.06, y_start + 0.39, 0.4, 0.2, self.data['network'], max_net, (0.3, 0.6, 1))
        if self.data['network']:
            GaussianTextRenderer.render_text(renderer, f"{self.data['network'][-1]:.0f}", 0.38, y_start + 0.38, 0.015, (0.5, 0.7, 1))

        # System Info
        renderer.draw_frame(0.52, y_start + 0.35, 0.42, 0.25)
        GaussianTextRenderer.render_text(renderer, "SYSTEM INFO", 0.53, y_start + 0.36, 0.012, (1, 1, 0.7))

        info_y = y_start + 0.40
        GaussianTextRenderer.render_text(renderer, f"UPTIME: {self.data['uptime']}", 0.54, info_y, 0.010, (0.9, 0.9, 0.9))
        info_y += 0.03
        GaussianTextRenderer.render_text(renderer, f"LOAD: {self.data['load'][0]:.2f} {self.data['load'][1]:.2f} {self.data['load'][2]:.2f}",
                                         0.54, info_y, 0.010, (0.9, 0.9, 0.9))
        info_y += 0.03
        GaussianTextRenderer.render_text(renderer, f"PROCESSES: {len(self.data['processes'])}", 0.54, info_y, 0.010, (0.9, 0.9, 0.9))
        info_y += 0.03
        mem_gb = self.data['mem_info'].used / 1024 / 1024 / 1024
        total_gb = self.data['mem_info'].total / 1024 / 1024 / 1024
        GaussianTextRenderer.render_text(renderer, f"MEM: {mem_gb:.1f}/{total_gb:.1f} GB", 0.54, info_y, 0.010, (0.9, 0.9, 0.9))

    def draw_processes(self, renderer, y_start):
        """Draw process list - all Gaussians"""
        if not hasattr(self, 'data'):
            return

        # Title
        GaussianTextRenderer.render_text(renderer, "ALL PROCESSES", 0.38, y_start, 0.02, (1, 1, 1))

        # Headers
        header_y = y_start + 0.05
        GaussianTextRenderer.render_text(renderer, "PID", 0.05, header_y, 0.012, (0.6, 0.6, 1))
        GaussianTextRenderer.render_text(renderer, "NAME", 0.15, header_y, 0.012, (0.6, 0.6, 1))
        GaussianTextRenderer.render_text(renderer, "CPU%", 0.45, header_y, 0.012, (0.6, 0.6, 1))
        GaussianTextRenderer.render_text(renderer, "MEM MB", 0.55, header_y, 0.012, (0.6, 0.6, 1))

        # Process rows
        row_y = header_y + 0.03
        visible_procs = self.data['processes'][self.process_scroll:self.process_scroll + 25]

        for i, proc in enumerate(visible_procs):
            renderer.draw_process_row(0.05, row_y + i * 0.025, 0.9, proc, i)

        # Scroll indicator
        if len(self.data['processes']) > 25:
            GaussianTextRenderer.render_text(renderer,
                f"SHOWING {self.process_scroll+1}-{min(self.process_scroll+25, len(self.data['processes']))} OF {len(self.data['processes'])}",
                0.7, 0.85, 0.010, (0.5, 0.5, 0.7))

    def draw_network(self, renderer, y_start):
        """Draw network details - all Gaussians"""
        if not hasattr(self, 'data'):
            return

        GaussianTextRenderer.render_text(renderer, "NETWORK MONITOR", 0.35, y_start, 0.02, (1, 1, 1))

        # Large network chart
        renderer.draw_frame(0.05, y_start + 0.08, 0.9, 0.5)
        max_net = max(self.data['network']) if self.data['network'] else 1000
        renderer.draw_line_chart(0.06, y_start + 0.12, 0.88, 0.45, self.data['network'], max_net, (0.3, 0.6, 1))

        # Current speed
        if self.data['network']:
            speed = self.data['network'][-1]
            GaussianTextRenderer.render_text(renderer, f"CURRENT: {speed:.1f} KB/S", 0.35, y_start + 0.62, 0.018, (0.5, 0.8, 1))

            # Peak
            peak = max(self.data['network'])
            GaussianTextRenderer.render_text(renderer, f"PEAK: {peak:.1f} KB/S", 0.35, y_start + 0.67, 0.015, (0.7, 0.5, 1))

    def draw_system(self, renderer, y_start):
        """Draw system details - all Gaussians"""
        if not hasattr(self, 'data'):
            return

        GaussianTextRenderer.render_text(renderer, "SYSTEM INFORMATION", 0.33, y_start, 0.02, (1, 1, 1))

        # System details
        info_y = y_start + 0.08
        spacing = 0.04

        kernel = os.uname()
        GaussianTextRenderer.render_text(renderer, f"KERNEL: {kernel.sysname} {kernel.release}", 0.1, info_y, 0.012, (0.8, 0.8, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"HOSTNAME: {kernel.nodename}", 0.1, info_y, 0.012, (0.8, 0.8, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"ARCHITECTURE: {kernel.machine}", 0.1, info_y, 0.012, (0.8, 0.8, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"UPTIME: {self.data['uptime']}", 0.1, info_y, 0.012, (0.8, 0.8, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"LOAD AVERAGE: {self.data['load'][0]:.2f} {self.data['load'][1]:.2f} {self.data['load'][2]:.2f}",
                                         0.1, info_y, 0.012, (0.8, 0.8, 0.9))
        info_y += spacing

        # Memory details
        mem = self.data['mem_info']
        GaussianTextRenderer.render_text(renderer, f"TOTAL MEMORY: {mem.total//1024//1024//1024} GB", 0.1, info_y, 0.012, (0.9, 0.7, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"USED MEMORY: {mem.used//1024//1024//1024} GB ({mem.percent:.1f}%)",
                                         0.1, info_y, 0.012, (0.9, 0.7, 0.9))
        info_y += spacing

        GaussianTextRenderer.render_text(renderer, f"AVAILABLE: {mem.available//1024//1024//1024} GB", 0.1, info_y, 0.012, (0.9, 0.7, 0.9))
        info_y += spacing

        # CPU info
        cpu_count = psutil.cpu_count()
        cpu_freq = psutil.cpu_freq()
        GaussianTextRenderer.render_text(renderer, f"CPU CORES: {cpu_count}", 0.1, info_y, 0.012, (0.7, 0.9, 0.7))
        info_y += spacing

        if cpu_freq:
            GaussianTextRenderer.render_text(renderer, f"CPU FREQ: {cpu_freq.current:.0f} MHZ", 0.1, info_y, 0.012, (0.7, 0.9, 0.7))

class GaussianApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.pure_gaussian')

    def do_activate(self):
        win = PureGaussianDashboard(application=self)
        win.present()

if __name__ == '__main__':
    app = GaussianApp()
    app.run(None)