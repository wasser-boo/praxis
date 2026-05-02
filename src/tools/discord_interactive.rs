use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, RwLock};

lazy_static::lazy_static! {
    pub static ref PENDING_QUESTIONS: Arc<RwLock<HashMap<String, PendingQuestion>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

pub struct PendingQuestion {
    pub tx: oneshot::Sender<String>,
}

/// Send a VM screenshot to Discord
pub async fn send_screenshot_to_discord(
    channel_id: &str,
    caption: Option<&str>,
    vm_name: &str,
) -> anyhow::Result<String> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let screenshot_path = format!("{}/vm/{}/screenshot.ppm", data_dir, vm_name);

    // Take a fresh screenshot first
    if let Some(manager) = crate::tools::vm_tools::get_vm_manager().await {
        let _ = manager.screenshot(vm_name).await;
    }

    if !std::path::Path::new(&screenshot_path).exists() {
        return Ok("No screenshot available. Is the VM running?".to_string());
    }

    let msg = caption.unwrap_or("VM Screenshot");
    match crate::tools::discord_upload::upload_file(
        channel_id,
        &screenshot_path,
        "vm_screenshot.ppm",
        Some(msg),
    )
    .await
    {
        Ok(result) => Ok(format!(
            "Screenshot sent to Discord channel {}: {}",
            channel_id, result
        )),
        Err(e) => Ok(format!("Failed to send screenshot: {}", e)),
    }
}

/// Send a screenshot with feedback text to Discord as a rich embed
pub async fn screenshot_with_feedback(
    channel_id: &str,
    feedback: &str,
    vm_name: &str,
) -> anyhow::Result<String> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());
    let screenshot_path = format!("{}/vm/{}/screenshot.ppm", data_dir, vm_name);

    // Take a fresh screenshot
    if let Some(manager) = crate::tools::vm_tools::get_vm_manager().await {
        let _ = manager.screenshot(vm_name).await;
    }

    if !std::path::Path::new(&screenshot_path).exists() {
        // No screenshot, just send text
        crate::tools::discord_send_message::send_message(channel_id, feedback).await?;
        return Ok("No screenshot available, sent text only.".to_string());
    }

    // Upload screenshot with feedback as caption
    match crate::tools::discord_upload::upload_file(
        channel_id,
        &screenshot_path,
        "vm_screenshot.ppm",
        Some(feedback),
    )
    .await
    {
        Ok(result) => Ok(format!(
            "Screenshot with feedback sent to {}: {}",
            channel_id, result
        )),
        Err(e) => Ok(format!("Failed to send: {}", e)),
    }
}

/// Ask the user a question via Discord with optional reaction-based suggestions.
/// The tool blocks until the user responds (timeout after timeout_secs).
pub async fn ask_question(
    channel_id: &str,
    question: &str,
    suggestions: &[String],
    timeout_secs: u64,
) -> anyhow::Result<String> {
    let question_id = uuid::Uuid::new_v4().to_string();

    // Build the message
    let mut message = format!("**Question from AI:**\n{}", question);

    if !suggestions.is_empty() {
        message.push_str("\n\n**Quick replies:**");
        let emojis = ["1️⃣", "2️⃣", "3️⃣", "4️⃣", "5️⃣", "6️⃣", "7️⃣", "8️⃣", "9️⃣"];
        for (i, suggestion) in suggestions.iter().enumerate() {
            if i < emojis.len() {
                message.push_str(&format!("\n{} {}", emojis[i], suggestion));
            }
        }
        message.push_str("\n\nReact with an emoji or type your answer below.");
    } else {
        message.push_str("\n\nType your answer below.");
    }

    // Send the question message
    crate::tools::discord_send_message::send_message(channel_id, &message).await?;

    // Create a channel to wait for the response
    let (tx, rx) = oneshot::channel::<String>();

    // Store the pending question
    {
        let mut pending = PENDING_QUESTIONS.write().await;
        pending.insert(question_id.clone(), PendingQuestion { tx });
    }

    tracing::info!(question_id = %question_id, channel = %channel_id, "Waiting for user response...");

    // Wait for response with timeout
    let timeout = std::time::Duration::from_secs(timeout_secs);
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(response)) => {
            tracing::info!(question_id = %question_id, response = %response, "Got user response");
            Ok(format!("User responded: {}", response))
        }
        Ok(Err(_)) => {
            // Channel was dropped (question cancelled)
            let mut pending = PENDING_QUESTIONS.write().await;
            pending.remove(&question_id);
            Ok("Question was cancelled.".to_string())
        }
        Err(_) => {
            // Timeout
            let mut pending = PENDING_QUESTIONS.write().await;
            pending.remove(&question_id);
            let timeout_msg = format!(
                "Question timed out after {}s. No response received.",
                timeout_secs
            );
            let _ =
                crate::tools::discord_send_message::send_message(channel_id, &timeout_msg).await;
            Ok(timeout_msg)
        }
    }
}

/// Called by the Discord handler when a user reacts to a question message
pub async fn handle_reaction(channel_id: &str, emoji: &str, _user_id: &str) {
    let emoji_to_text: HashMap<&str, &str> = [
        ("1️⃣", "1"),
        ("2️⃣", "2"),
        ("3️⃣", "3"),
        ("4️⃣", "4"),
        ("5️⃣", "5"),
        ("6️⃣", "6"),
        ("7️⃣", "7"),
        ("8️⃣", "8"),
        ("9️⃣", "9"),
        ("✅", "yes"),
        ("❌", "no"),
        ("👍", "yes"),
        ("👎", "no"),
    ]
    .iter()
    .cloned()
    .collect();

    let text = emoji_to_text.get(emoji).unwrap_or(&emoji).to_string();

    // Respond to the first pending question in this channel
    let mut pending = PENDING_QUESTIONS.write().await;
    let keys: Vec<String> = pending.keys().cloned().collect();
    for key in keys {
        if let Some(q) = pending.remove(&key) {
            let _ = q.tx.send(text);
            return;
        }
    }
}

/// Called by the Discord handler when a user sends a message that's a reply to a question
pub async fn handle_message_reply(channel_id: &str, content: &str, _user_id: &str) {
    let mut pending = PENDING_QUESTIONS.write().await;
    let keys: Vec<String> = pending.keys().cloned().collect();
    for key in keys {
        if let Some(q) = pending.remove(&key) {
            let _ = q.tx.send(content.to_string());
            return;
        }
    }
}
