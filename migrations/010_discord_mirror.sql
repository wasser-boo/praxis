-- Discord chat mirror: persist mirrored Discord messages in the messages
-- table so the dashboard chat shows them after a reload (previously they were
-- only pushed as transient `discord_message` SSE events).
-- Meta (direction/author/channel) is stored as JSON; role is
-- 'discord_user' or 'discord_bot'. LLM history queries filter these roles out.
ALTER TABLE messages ADD COLUMN discord_meta TEXT;