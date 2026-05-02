use crate::gateway::llm::provider::ContentPart;

/// Result of the understand_image tool
pub struct ImageToolResult {
    pub text: String,
    pub content_parts: Vec<ContentPart>,
}

/// Read an image file and return it as a content_part for LLM vision
pub async fn run(args: &serde_json::Value) -> ImageToolResult {
    let path = args["path"].as_str().unwrap_or("");

    if path.is_empty() {
        return ImageToolResult {
            text: "Error: path is required".to_string(),
            content_parts: Vec::new(),
        };
    }

    if !std::path::Path::new(path).exists() {
        return ImageToolResult {
            text: format!("Error: File not found: {}", path),
            content_parts: Vec::new(),
        };
    }

    match std::fs::read(path) {
        Ok(bytes) => {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

            // Determine MIME type from extension
            let mime = match std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase()
                .as_str()
            {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "ppm" => "image/x-portable-pixmap",
                _ => "image/png",
            };

            let data_url = format!("data:{};base64,{}", mime, b64);

            ImageToolResult {
                text: format!("Image loaded: {}", path),
                content_parts: vec![ContentPart::ImageUrl {
                    image_url: crate::gateway::llm::provider::ImageUrlDetail {
                        url: data_url,
                        detail: Some("high".to_string()),
                    },
                }],
            }
        }
        Err(e) => ImageToolResult {
            text: format!("Error reading image: {}", e),
            content_parts: Vec::new(),
        },
    }
}
