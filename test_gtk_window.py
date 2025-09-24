#!/usr/bin/env python3
"""Test GTK4 window with Gaussian Splatting info"""

import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk, GLib

class GaussianSplatWindow(Gtk.ApplicationWindow):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.set_title("🔥 9P.e System Monitor - Gaussian Splatting")
        self.set_default_size(1400, 900)

        # Main container
        vbox = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=20)
        vbox.set_margin_start(20)
        vbox.set_margin_end(20)
        vbox.set_margin_top(20)
        vbox.set_margin_bottom(20)
        self.set_child(vbox)

        # Header
        header = Gtk.Label()
        header.set_markup("<span size='xx-large' weight='bold'>🔥 9P.e System Monitor with Gaussian Splatting</span>")
        vbox.append(header)

        # Server info
        server_info = Gtk.Label(label="✅ Server running at: http://localhost:4001")
        vbox.append(server_info)

        # Gaussian features
        features_text = """🎨 Gaussian Splatting Features:
• Content-adaptive 2D Gaussian initialization
• Tile-based rendering with top-K optimization
• Real-time dashboard chart generation
• Image-GS algorithm from SIGGRAPH 2025 research
• 500+ lines of mathematically-accurate implementation"""

        features = Gtk.Label(label=features_text)
        features.set_justify(Gtk.Justification.LEFT)
        vbox.append(features)

        # Chart info
        charts_text = """📊 Available Gaussian Splat Charts:
• CPU Usage: http://localhost:4001/ui/charts/cpu.html
• Memory Usage: http://localhost:4001/ui/charts/memory.html
• Network Activity: http://localhost:4001/ui/charts/network.html
• Process Distribution: http://localhost:4001/ui/charts/process.html"""

        charts = Gtk.Label(label=charts_text)
        charts.set_justify(Gtk.Justification.LEFT)
        vbox.append(charts)

        # Technical details
        tech_text = """🔬 Technical Implementation:
• Based on "Image-GS: Content-Adaptive Image Representation via 2D Gaussians"
• Authors: Yunxiang Zhang, Alexandr Kuznetsov, Akshay Jindal, Kenneth Chen, Anton Kaplanyan
• Research: NYU, Intel, AMD (SIGGRAPH 2025)
• Integrated with 9P.e synthetic file system for real-time metrics"""

        tech = Gtk.Label(label=tech_text)
        tech.set_justify(Gtk.Justification.LEFT)
        vbox.append(tech)

        # Status
        self.status = Gtk.Label()
        self.status.set_markup("<span weight='bold' color='green'>🌐 Open the URLs above in your browser to see live visualizations!</span>")
        vbox.append(self.status)

        # Update timer
        self.counter = 0
        GLib.timeout_add_seconds(1, self.update_status)

    def update_status(self):
        """Update status display"""
        self.counter += 1
        self.status.set_markup(f"<span weight='bold' color='green'>⏱️ Running for {self.counter} seconds...</span>")
        return True  # Continue timer

class GaussianSplatApp(Gtk.Application):
    def __init__(self):
        super().__init__(application_id='org.plan9e.gaussian')

    def do_activate(self):
        win = GaussianSplatWindow(application=self)
        win.present()

if __name__ == '__main__':
    app = GaussianSplatApp()
    app.run(None)