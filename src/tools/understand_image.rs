use crate::gateway::llm::provider::ContentPart;

/// Result of the understand_image tool
pub struct ImageToolResult {
    pub text: String,
    pub content_parts: Vec<ContentPart>,
}

/// Read an image file, convert to PNG if needed, and return as content_part
pub async fn run(args: &serde_json::Value) -> ImageToolResult {
    let path = args["path"].as_str().unwrap_or("");
    let prompt = args["prompt"].as_str().unwrap_or("");

    if path.is_empty() || prompt.is_empty() {
        return ImageToolResult {
            text: "Error: path and prompt are required".to_string(),
            content_parts: Vec::new(),
        };
    }

    if !std::path::Path::new(path).exists() {
        return ImageToolResult {
            text: format!("Error: File not found: {}", path),
            content_parts: Vec::new(),
        };
    }

    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // For PPM files, convert to PNG since most vision APIs don't support PPM
    let (png_bytes, mime) = if ext == "ppm" || ext == "pgm" || ext == "pbm" {
        match image::open(path) {
            Ok(img) => {
                let mut buf = Vec::new();
                let cursor = std::io::Cursor::new(&mut buf);
                match img.write_to(cursor, image::ImageFormat::Png) {
                    Ok(_) => (buf, "image/png"),
                    Err(e) => {
                        return ImageToolResult {
                            text: format!("Error converting PPM to PNG: {}", e),
                            content_parts: Vec::new(),
                        };
                    }
                }
            }
            Err(e) => {
                return ImageToolResult {
                    text: format!("Error reading PPM file: {}", e),
                    content_parts: Vec::new(),
                };
            }
        }
    } else {
        // For other formats, read as-is
        match std::fs::read(path) {
            Ok(bytes) => {
                let mime = match ext.as_str() {
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    "webp" => "image/webp",
                    _ => "image/png",
                };
                (bytes, mime)
            }
            Err(e) => {
                return ImageToolResult {
                    text: format!("Error reading image: {}", e),
                    content_parts: Vec::new(),
                };
            }
        }
    };

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);

    let text = format!("Image loaded: {}. {}", path, prompt);

    ImageToolResult {
        text,
        content_parts: vec![ContentPart::ImageUrl {
            image_url: crate::gateway::llm::provider::ImageUrlDetail {
                url: data_url,
                detail: Some("high".to_string()),
            },
        }],
    }
}
