//! Modern LibDraw - Graphics system served through 9P.e synthetic files
//!
//! Philosophy: Everything is a file, including graphics primitives
//! Access pattern: echo "params" > /draw/primitive, cat /draw/framebuffer

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use crate::synthetic::SyntheticGenerator;

/// Modern color representation with HDR support
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,  // 0.0-1.0, can exceed for HDR
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const RED: Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE: Color = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    pub fn to_rgba8(&self) -> (u8, u8, u8, u8) {
        (
            (self.r.clamp(0.0, 1.0) * 255.0) as u8,
            (self.g.clamp(0.0, 1.0) * 255.0) as u8,
            (self.b.clamp(0.0, 1.0) * 255.0) as u8,
            (self.a.clamp(0.0, 1.0) * 255.0) as u8,
        )
    }
}

/// 2D Point
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

/// Rectangle
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Rect { x, y, width, height }
    }
}

/// Drawing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawCommand {
    /// Clear the surface with color
    Clear { color: Color },

    /// Draw a line
    Line { start: Point, end: Point, color: Color, width: f32 },

    /// Draw a rectangle
    Rectangle { rect: Rect, color: Color, filled: bool },

    /// Draw a circle
    Circle { center: Point, radius: f32, color: Color, filled: bool },

    /// Draw text
    Text { position: Point, text: String, color: Color, size: f32, font: String },

    /// Composite image
    Image { position: Point, image_id: String, opacity: f32 },

    /// Custom shader operation
    Shader { shader_id: String, params: HashMap<String, f32> },
}

/// Canvas state for rendering
#[derive(Debug, Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub commands: Vec<DrawCommand>,
    pub background: Color,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Canvas {
            width,
            height,
            commands: Vec::new(),
            background: Color::WHITE,
        }
    }

    pub fn add_command(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// Render to HTML5 Canvas
    pub fn to_html5_canvas(&self) -> String {
        let mut html = format!(
            r#"<canvas id="canvas" width="{}" height="{}"></canvas>
<script>
const canvas = document.getElementById('canvas');
const ctx = canvas.getContext('2d');

// Clear with background
ctx.fillStyle = 'rgba({}, {}, {}, {})';
ctx.fillRect(0, 0, {}, {});
"#,
            self.width, self.height,
            (self.background.r * 255.0) as u8,
            (self.background.g * 255.0) as u8,
            (self.background.b * 255.0) as u8,
            self.background.a,
            self.width, self.height
        );

        for cmd in &self.commands {
            match cmd {
                DrawCommand::Clear { color } => {
                    html.push_str(&format!(
                        "ctx.fillStyle = 'rgba({}, {}, {}, {})';\nctx.fillRect(0, 0, {}, {});\n",
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        color.a,
                        self.width, self.height
                    ));
                }
                DrawCommand::Line { start, end, color, width } => {
                    html.push_str(&format!(
                        "ctx.strokeStyle = 'rgba({}, {}, {}, {})';\nctx.lineWidth = {};\nctx.beginPath();\nctx.moveTo({}, {});\nctx.lineTo({}, {});\nctx.stroke();\n",
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        color.a,
                        width,
                        start.x, start.y,
                        end.x, end.y
                    ));
                }
                DrawCommand::Rectangle { rect, color, filled } => {
                    if *filled {
                        html.push_str(&format!(
                            "ctx.fillStyle = 'rgba({}, {}, {}, {})';\nctx.fillRect({}, {}, {}, {});\n",
                            (color.r * 255.0) as u8,
                            (color.g * 255.0) as u8,
                            (color.b * 255.0) as u8,
                            color.a,
                            rect.x, rect.y, rect.width, rect.height
                        ));
                    } else {
                        html.push_str(&format!(
                            "ctx.strokeStyle = 'rgba({}, {}, {}, {})';\nctx.strokeRect({}, {}, {}, {});\n",
                            (color.r * 255.0) as u8,
                            (color.g * 255.0) as u8,
                            (color.b * 255.0) as u8,
                            color.a,
                            rect.x, rect.y, rect.width, rect.height
                        ));
                    }
                }
                DrawCommand::Circle { center, radius, color, filled } => {
                    html.push_str(&format!(
                        "ctx.{}Style = 'rgba({}, {}, {}, {})';\nctx.beginPath();\nctx.arc({}, {}, {}, 0, 2 * Math.PI);\nctx.{}();\n",
                        if *filled { "fill" } else { "stroke" },
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        color.a,
                        center.x, center.y, radius,
                        if *filled { "fill" } else { "stroke" }
                    ));
                }
                DrawCommand::Text { position, text, color, size, .. } => {
                    html.push_str(&format!(
                        "ctx.fillStyle = 'rgba({}, {}, {}, {})';\nctx.font = '{}px sans-serif';\nctx.fillText('{}', {}, {});\n",
                        (color.r * 255.0) as u8,
                        (color.g * 255.0) as u8,
                        (color.b * 255.0) as u8,
                        color.a,
                        size,
                        text.replace("'", "\\'"),
                        position.x, position.y
                    ));
                }
                _ => {
                    // TODO: Implement other commands
                }
            }
        }

        html.push_str("</script>");
        html
    }
}

/// Display manager - holds canvases and handles rendering
pub struct ModernDisplay {
    canvases: Arc<RwLock<HashMap<String, Canvas>>>,
    default_canvas: String,
}

impl ModernDisplay {
    pub fn new() -> Self {
        let mut canvases = HashMap::new();
        let default_canvas = "main".to_string();
        canvases.insert(default_canvas.clone(), Canvas::new(800, 600));

        ModernDisplay {
            canvases: Arc::new(RwLock::new(canvases)),
            default_canvas,
        }
    }

    pub async fn get_canvas(&self, name: &str) -> Option<Canvas> {
        let canvases = self.canvases.read().await;
        canvases.get(name).cloned()
    }

    pub async fn add_command(&self, canvas_name: &str, command: DrawCommand) -> Result<()> {
        let mut canvases = self.canvases.write().await;
        if let Some(canvas) = canvases.get_mut(canvas_name) {
            canvas.add_command(command);
        }
        Ok(())
    }

    pub async fn create_canvas(&self, name: String, width: u32, height: u32) -> Result<()> {
        let mut canvases = self.canvases.write().await;
        canvases.insert(name, Canvas::new(width, height));
        Ok(())
    }
}

/// Synthetic file generator for canvas HTML output
pub struct CanvasHtmlGenerator {
    display: Arc<ModernDisplay>,
    canvas_name: String,
}

impl CanvasHtmlGenerator {
    pub fn new(display: Arc<ModernDisplay>, canvas_name: String) -> Self {
        CanvasHtmlGenerator { display, canvas_name }
    }
}

#[async_trait]
impl SyntheticGenerator for CanvasHtmlGenerator {
    async fn generate(&self, offset: u64, count: u32) -> Result<Vec<u8>> {
        let canvas = self.display.get_canvas(&self.canvas_name).await
            .unwrap_or_else(|| Canvas::new(800, 600));

        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>9P.e Graphics Canvas</title>
    <style>
        body {{ margin: 0; padding: 20px; font-family: sans-serif; }}
        canvas {{ border: 1px solid #ddd; }}
    </style>
</head>
<body>
    <h1>9P.e Modern LibDraw</h1>
    {}
</body>
</html>"#,
            canvas.to_html5_canvas()
        );

        let bytes = html.as_bytes();
        let start = offset.min(bytes.len() as u64) as usize;
        let end = (start + count as usize).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    async fn size(&self) -> u64 {
        32768 // Large enough for most HTML canvases
    }

    fn refresh_rate_ms(&self) -> u64 {
        100 // Refresh 10 times per second for animations
    }
}

/// Command processor - parses drawing commands from file writes
pub struct DrawCommandProcessor {
    display: Arc<ModernDisplay>,
}

impl DrawCommandProcessor {
    pub fn new(display: Arc<ModernDisplay>) -> Self {
        DrawCommandProcessor { display }
    }

    pub async fn process_command(&self, canvas_name: &str, command_text: &str) -> Result<()> {
        let parts: Vec<&str> = command_text.trim().split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        let command = match parts[0] {
            "clear" => {
                let color = if parts.len() >= 4 {
                    Color::from_rgba8(
                        parts[1].parse().unwrap_or(255),
                        parts[2].parse().unwrap_or(255),
                        parts[3].parse().unwrap_or(255),
                        parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(255),
                    )
                } else {
                    Color::WHITE
                };
                DrawCommand::Clear { color }
            }
            "line" if parts.len() >= 9 => {
                DrawCommand::Line {
                    start: Point::new(parts[1].parse()?, parts[2].parse()?),
                    end: Point::new(parts[3].parse()?, parts[4].parse()?),
                    color: Color::from_rgba8(
                        parts[5].parse()?,
                        parts[6].parse()?,
                        parts[7].parse()?,
                        255,
                    ),
                    width: parts[8].parse()?,
                }
            }
            "rect" if parts.len() >= 9 => {
                DrawCommand::Rectangle {
                    rect: Rect::new(
                        parts[1].parse()?,
                        parts[2].parse()?,
                        parts[3].parse()?,
                        parts[4].parse()?,
                    ),
                    color: Color::from_rgba8(
                        parts[5].parse()?,
                        parts[6].parse()?,
                        parts[7].parse()?,
                        255,
                    ),
                    filled: parts.get(8).map(|s| *s == "filled").unwrap_or(false),
                }
            }
            "circle" if parts.len() >= 8 => {
                DrawCommand::Circle {
                    center: Point::new(parts[1].parse()?, parts[2].parse()?),
                    radius: parts[3].parse()?,
                    color: Color::from_rgba8(
                        parts[4].parse()?,
                        parts[5].parse()?,
                        parts[6].parse()?,
                        255,
                    ),
                    filled: parts.get(7).map(|s| *s == "filled").unwrap_or(false),
                }
            }
            "text" if parts.len() >= 7 => {
                let text = parts[6..].join(" ");
                DrawCommand::Text {
                    position: Point::new(parts[1].parse()?, parts[2].parse()?),
                    text,
                    color: Color::from_rgba8(
                        parts[3].parse()?,
                        parts[4].parse()?,
                        parts[5].parse()?,
                        255,
                    ),
                    size: 16.0,
                    font: "sans-serif".to_string(),
                }
            }
            _ => return Ok(()), // Ignore unknown commands
        };

        self.display.add_command(canvas_name, command).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_conversion() {
        let color = Color::from_rgba8(255, 128, 0, 255);
        assert!((color.r - 1.0).abs() < 0.001);
        assert!((color.g - 0.502).abs() < 0.01);
        assert!((color.b - 0.0).abs() < 0.001);

        let (r, g, b, a) = color.to_rgba8();
        assert_eq!(r, 255);
        assert_eq!(g, 128);
        assert_eq!(b, 0);
        assert_eq!(a, 255);
    }

    #[tokio::test]
    async fn test_canvas_operations() {
        let display = Arc::new(ModernDisplay::new());

        // Add some drawing commands
        let line_cmd = DrawCommand::Line {
            start: Point::new(10.0, 10.0),
            end: Point::new(100.0, 100.0),
            color: Color::RED,
            width: 2.0,
        };

        display.add_command("main", line_cmd).await.unwrap();

        let canvas = display.get_canvas("main").await.unwrap();
        assert_eq!(canvas.commands.len(), 1);

        // Test HTML generation
        let html = canvas.to_html5_canvas();
        assert!(html.contains("canvas"));
        assert!(html.contains("strokeStyle"));
    }
}