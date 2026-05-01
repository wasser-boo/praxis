use crate::event_channel::{DiscordEmbed, EmbedField};

pub async fn send_embed(
    user_id: &str,
    channel_id: &str,
    title: Option<&str>,
    description: Option<&str>,
    url: Option<&str>,
    color: Option<u32>,
    footer: Option<&str>,
    author: Option<&str>,
    thumbnail: Option<&str>,
    image: Option<&str>,
    fields: Vec<EmbedField>,
) -> anyhow::Result<String> {
    if channel_id.is_empty() {
        return Err(anyhow::anyhow!("channel_id is required"));
    }

    let embed = DiscordEmbed {
        title: title.map(|s| s.to_string()),
        description: description.map(|s| s.to_string()),
        url: url.map(|s| s.to_string()),
        color,
        footer: footer.map(|s| s.to_string()),
        author: author.map(|s| s.to_string()),
        thumbnail: thumbnail.map(|s| s.to_string()),
        image: image.map(|s| s.to_string()),
        fields,
    };

    crate::event_channel::broadcast_channel_embed(user_id, channel_id, embed);
    tracing::info!("Broadcasted embed to channel {}", channel_id);
    Ok(format!("embed_sent_to_{}", channel_id))
}

pub fn parse_color(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::Number(n) => n.as_u64().map(|v| v as u32),
        serde_json::Value::String(s) => {
            let s = s.trim().trim_start_matches('#').trim_start_matches("0x");
            u32::from_str_radix(s, 16).ok()
        }
        _ => None,
    }
}

pub fn parse_fields(value: &serde_json::Value) -> Vec<EmbedField> {
    match value {
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                Some(EmbedField {
                    name: obj.get("name")?.as_str()?.to_string(),
                    value: obj.get("value")?.as_str()?.to_string(),
                    inline: obj.get("inline").and_then(|v| v.as_bool()).unwrap_or(false),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_send_embed_basic() {
        let _tx = crate::event_channel::init();
        let result = send_embed(
            "user1",
            "123456",
            Some("Title"),
            Some("Desc"),
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
        )
        .await
        .unwrap();
        assert!(result.contains("123456"));
    }

    #[tokio::test]
    async fn test_send_embed_no_channel() {
        let _tx = crate::event_channel::init();
        let result = send_embed(
            "user1",
            "",
            Some("Title"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_color_hex_string() {
        assert_eq!(parse_color(&serde_json::json!("6C5CE7")), Some(0x6C5CE7));
        assert_eq!(parse_color(&serde_json::json!("#FF0000")), Some(0xFF0000));
        assert_eq!(parse_color(&serde_json::json!("0x00FF00")), Some(0x00FF00));
    }

    #[test]
    fn test_parse_color_number() {
        assert_eq!(parse_color(&serde_json::json!(0x6C5CE7)), Some(0x6C5CE7));
        assert_eq!(parse_color(&serde_json::json!(255)), Some(255));
    }

    #[test]
    fn test_parse_fields() {
        let fields_json = serde_json::json!([
            {"name": "Field 1", "value": "Value 1", "inline": true},
            {"name": "Field 2", "value": "Value 2"}
        ]);
        let fields = parse_fields(&fields_json);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "Field 1");
        assert!(fields[0].inline);
        assert!(!fields[1].inline);
    }

    #[test]
    fn test_parse_fields_empty() {
        let fields = parse_fields(&serde_json::json!(null));
        assert!(fields.is_empty());
    }
}
