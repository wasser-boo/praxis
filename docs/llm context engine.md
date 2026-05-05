# LLM Context Engine

## Overview

The context engine manages per-user state that drives template rendering, workflow progression, and knowledge injection. It consists of:

- **Context** - stored in SQLite, persists across turns
- **set_context** - LLM tool for updating context variables
- **POML templates** - rendered with context variables injected
- **CL system** - state machine for workflow automation
- **RAG system** - document ingestion and retrieval with neural embeddings
- **File handling** - Discord attachment download with VM shared folder support

---

## Context Structure

```rust
struct Context {
    user_id: String,
    turn: i32,
    mode: String,              // "agent" or "chat"
    user_name: Option<String>,
    cl_file: Option<String>,   // Path to CL workflow file
    active_state: Option<String>,
    active_templates: Vec<String>,
    settings: ContextSettings,
    custom_data: Value,        // Arbitrary user data (device, style, etc.)
    cl_data: Value,            // Workflow-specific data (application, screen, etc.)
}
```

### custom_data vs cl_data

| Field | Purpose | Merge Behavior |
|-------|---------|----------------|
| `custom_data` | General user preferences (device, style, theme) | Deep merge via dot-notation |
| `cl_data` | Workflow/application context (application, screen, step) | Deep merge via dot-notation |

Both fields support **dot-notation** for nested updates without overwriting existing data.

---

## set_context Tool

The `set_context` tool writes to context variables. It supports dot-notation for nested keys.

### Tool Schema

```json
{
  "name": "set_context",
  "parameters": {
    "key": "string (required)",
    "value": "any (required)"
  }
}
```

### Examples

**Simple key (top-level):**
```
set_context(key="mode", value="coding")
```

**Nested key with dot-notation:**
```
set_context(key="custom_data.device", value="main")
set_context(key="custom_data.style", value="analytical")
```

This sets `custom_data.device = "main"` and `custom_data.style = "analytical"` without overwriting each other.

**Deeply nested:**
```
set_context(key="cl_data.app.settings.theme", value="dark")
```

This creates: `cl_data.app.settings.theme = "dark"`

### How Dot-Notation Works

When a key contains dots (e.g., `custom_data.device`):
1. Splits key by `.` into parts: `["custom_data", "device"]`
2. Navigates into `data["custom_data"]`
3. If intermediate object doesn't exist, creates it
4. Sets `device` at the final level

This means you can set individual keys within `custom_data` or `cl_data` without overwriting the entire object.

---

## Template Rendering

POML templates are rendered with context variables injected. The template can access:

- `{{user_name}}` - User's name
- `{{mode}}` - Current mode
- `{{turn}}` - Turn counter
- `{{custom_data}}` - Full custom_data object
- `{{cl_data}}` - Full cl_data object
- `{{memory}}` - Learned facts, topics, preferences

### Accessing Nested Values

In POML templates, you can access nested values:

```xml
<p>Device: {{custom_data.device}}</p>
<p>Application: {{cl_data.application}}</p>
```

Or with conditional sections:

```xml
<section if="cl_data.application == 'fdisk'">
  <p>You are working with fdisk. Here's how it works...</p>
</section>
```

---

## Driving Templates with cl_data

The pattern: set `cl_data` variables, then let POML templates render content based on those variables.

### Example: Application-Specific Knowledge

**In your template (system.poml):**
```xml
<poml speaker="system">
  <task>
    <p>MANDATORY: Before calling agent_next, you MUST first call 
    <code>set_context</code> with key="cl_data.application" and value="fdisk".</p>
    <p>Then call <code>agent_next</code> to continue.</p>
  </task>

  <section if="cl_data.application == 'fdisk'">
    <p>You are working with fdisk. Key commands:</p>
    <list>
      <item>fdisk -l: List partitions</item>
      <item>fdisk /dev/sda: Open disk for editing</item>
      <item>p: Print partition table</item>
      <item>n: New partition</item>
      <item>w: Write changes</item>
    </list>
  </section>

  <section if="cl_data.application == 'parted'">
    <p>You are working with parted. Key commands:</p>
    <list>
      <item>parted /dev/sda: Open disk</item>
      <item>print: Show partitions</item>
      <item>mkpart: Create partition</item>
    </list>
  </section>
</poml>
```

**LLM call flow:**
1. LLM reads template, sees instruction
2. Calls `set_context(key="cl_data.application", value="fdisk")`
3. Calls `agent_next`
4. Next turn: template renders with fdisk-specific content

### Example: Multi-Screen Workflow

**Template with screen-based sections:**
```xml
<poml>
  <section if="cl_data.screen == 'boot'">
    <p>Boot screen: Select installation type</p>
  </section>
  
  <section if="cl_data.screen == 'disk'">
    <p>Disk partitioning: Use fdisk or parted</p>
  </section>
  
  <section if="cl_data.screen == 'filesystem'">
    <p>Create filesystems: mkfs.ext4, mkswap</p>
  </section>
  
  <task>
    After completing this screen, call:
    set_context(key="cl_data.screen", value="next_screen_name")
    Then call agent_next.
  </task>
</poml>
```

---

## RAG System

### How RAG Works

1. **Ingestion**: Documents are chunked (500 words, 50 overlap) and stored in SQLite
2. **Embedding**: Neural embeddings via OpenAI or Ollama (falls back to hash-based if not configured)
3. **Retrieval**: Cosine similarity search over all chunks for a user
4. **Injection**: Retrieved chunks can be injected into prompts

### RAG Tools (Registered)

| Tool | Description | Parameters |
|------|-------------|------------|
| `rag_search` | Search knowledge base using vector similarity | `query` (string), `limit` (int, default 5) |
| `rag_ingest` | Add document to knowledge base | `filename` (string), `content` (string), `file_type` (string) |
| `rag_list` | List all documents in knowledge base | none |
| `rag_delete` | Delete document by ID | `document_id` (string) |

### Embedding Configuration

Configure neural embeddings via environment variables:

```bash
# Provider: "openai" or "ollama"
EMBEDDING_PROVIDER=ollama

# Model name
EMBEDDING_MODEL=nomic-embed-text

# For OpenAI:
OPENAI_API_KEY=sk-...
OPENAI_BASE_URL=https://api.openai.com/v1

# For Ollama:
OLLAMA_BASE_URL=http://localhost:11434
```

**Supported Models:**
- OpenAI: `text-embedding-3-small`, `text-embedding-3-large`
- Ollama: `nomic-embed-text`, `mxbai-embed-large`, `all-minilm`

If no embedding provider is configured, falls back to hash-based 256-dim vectors.

---

## File Handling

### Discord Attachment Download

When a paired user sends a file attachment in Discord:

1. **Downloads** the file to the appropriate folder
2. **Appends** the file path to the message content
3. **LLM decides** what to do (read, ingest, etc.)

**Path Resolution:**
- VM enabled: `{data_dir}/shared/downloads/{filename}` → VM sees `/mnt/shared/downloads/{filename}`
- VM disabled: `{data_dir}/downloads/{filename}`

**Example:**
```
User: [attaches report.pdf] Here's the report
LLM sees: Here's the report
          [Attached file saved to: /mnt/shared/downloads/report.pdf]
```

### Reading Files with POML

POML's `<Document>` component can read PDF, DOCX, and TXT files directly:

```xml
<!-- Read entire PDF -->
<Document src="/mnt/shared/downloads/report.pdf" />

<!-- Read specific pages (0-indexed) -->
<Document src="/mnt/shared/downloads/report.pdf" selectedPages="0:5" />

<!-- Read DOCX -->
<Document src="/mnt/shared/downloads/document.docx" />

<!-- Read as text (no multimedia) -->
<Document src="/mnt/shared/downloads/data.txt" multimedia="false" />
```

**Supported Formats:**
- PDF (with page selection)
- DOCX
- TXT

### Complete File Flow

```
1. User sends attachment in Discord
2. File downloaded to shared/downloads/
3. LLM sees: [Attached file saved to: /mnt/shared/downloads/report.pdf]
4. LLM reads file:
   - Option A: vm_shell(command="cat /mnt/shared/downloads/report.pdf")
   - Option B: Use POML <Document> in template
5. LLM ingests to RAG (optional):
   rag_ingest(filename="report.pdf", content="extracted text...")
```

---

## Knowledge Getter Pattern

Create a template that automatically retrieves and injects RAG knowledge based on `cl_data`.

### Template: knowledge.poml

```xml
<poml>
  <task>
    You are a knowledge assistant. Use the retrieved context to answer questions.
    
    <p>Current application context: {{cl_data.application}}</p>
    <p>Retrieved knowledge:</p>
    <p>{{rag_context}}</p>
  </task>
</poml>
```

### Implementation: Auto-Retrieval

In `build_system_prompt()`, add RAG retrieval:

```rust
// After setting cl_data
if let Some(app) = ctx.cl_data.get("application").and_then(|v| v.as_str()) {
    // Search for application-specific knowledge
    let results = vector_search(&db, &ctx.user_id, app, 5).await?;
    let rag_context: Vec<String> = results.iter()
        .map(|r| r.content.clone())
        .collect();
    context_json["rag_context"] = serde_json::json!(rag_context.join("\n---\n"));
}
```

This automatically retrieves relevant documents based on the application context.

---

## CL System Integration

The CL (Context Language) system can automatically set `cl_data` variables based on state transitions.

### CL File Example

```cl
_default:
  cl_data.screen = "boot"

state boot:
  cl_data.screen = "boot"
  system_template = "workflow/boot"

  -> disk : when custom_data.device == "main"

state disk:
  cl_data.screen = "disk"
  system_template = "workflow/disk"

  -> filesystem : when cl_data.disk_done == "true"

state filesystem:
  cl_data.screen = "filesystem"
  system_template = "workflow/filesystem"
```

### How CL Uses cl_data

In `agent_control.rs`, when `agent_next` is called:
1. Loads CL file
2. Evaluates transitions based on `cl_data` and `custom_data`
3. Applies new state variables (including `cl_data` updates)
4. Saves context

This means you can use `cl_data` in CL transition conditions:

```cl
-> next_state : when cl_data.application == "fdisk" && cl_data.step == "done"
```

---

## Complete Flow Example

### 1. Template Setup

```xml
<!-- templates/fdisk.poml -->
<poml>
  <task>
    <p>You are helping with disk partitioning using fdisk.</p>
    <p>Current step: {{cl_data.step}}</p>
  </task>
  
  <section if="cl_data.step == 'select_disk'">
    <p>Select a disk to partition. Run: fdisk -l</p>
    <p>After selecting, call set_context(key="cl_data.step", value="partition")</p>
  </section>
  
  <section if="cl_data.step == 'partition'">
    <p>Create partitions using fdisk commands.</p>
    <p>After done, call set_context(key="cl_data.step", value="format")</p>
  </section>
  
  <section if="cl_data.step == 'format'">
    <p>Format partitions with mkfs.</p>
    <p>After done, call set_context(key="cl_data.step", value="done")</p>
  </section>
  
  <task>
    Always call set_context before agent_next.
  </task>
</poml>
```

### 2. LLM Interaction

```
User: Help me partition /dev/sda

LLM: I'll help you partition /dev/sda. Let me set up the workflow.
     [calls set_context(key="cl_data.application", value="fdisk")]
     [calls set_context(key="cl_data.step", value="select_disk")]
     [calls set_context(key="system_template", value="fdisk")]
     [calls agent_next]

[Next turn: template renders with cl_data.step == "select_disk"]

LLM: Let me list the available disks.
     [runs: fdisk -l]
     [calls set_context(key="cl_data.step", value="partition")]
     [calls agent_next]

[Next turn: template renders with cl_data.step == "partition"]
...
```

### 3. File Ingestion Flow

```
User: [attaches fdisk-guide.pdf] Here's the fdisk documentation

LLM: I'll read the PDF and add it to the knowledge base.
     [reads /mnt/shared/downloads/fdisk-guide.pdf via POML or vm_shell]
     [calls rag_ingest(filename="fdisk-guide.pdf", content="...")]

Later:
User: How do I create a partition?

LLM: [calls rag_search(query="create partition fdisk")]
     Based on the documentation, here's how to create a partition...
```

---

## Available Tools

### Context Tools (Registered)

| Tool | Description |
|------|-------------|
| `set_context` | Set context variable (supports dot-notation) |
| `get_context` | Read context variable |
| `delete_context` | Delete context variable |
| `agent_next` | Advance to next step |
| `agent_complete` | Mark task as done |
| `agent_set_path` | Set working directory |
| `agent_feedback` | Send progress update |

### Knowledge Tools (Registered)

| Tool | Description |
|------|-------------|
| `learn_fact` | Store a fact in memory |
| `learn_preference` | Store user preference |
| `learn_topic` | Track conversation topic |

### RAG Tools (Registered)

| Tool | Description |
|------|-------------|
| `rag_search` | Search knowledge base using vector similarity |
| `rag_ingest` | Add document to knowledge base |
| `rag_list` | List all documents in knowledge base |
| `rag_delete` | Delete document from knowledge base |

---

## Summary

1. **Use dot-notation** with `set_context` to set individual keys without overwriting
2. **Use `cl_data`** for workflow-specific data that drives template selection
3. **Use `custom_data`** for general user preferences
4. **Create POML templates** with `<section if="cl_data.x == 'y'">` for conditional content
5. **Use `rag_search`** to search knowledge base from templates
6. **Use `rag_ingest`** to add documents to knowledge base
7. **Use CL system** for automated state transitions based on context variables
8. **Configure embeddings** via `EMBEDDING_PROVIDER` env var for better search quality
9. **Discord attachments** are auto-downloaded to shared/downloads/ for VM access
10. **Use POML `<Document>`** to read PDF, DOCX, and TXT files directly in templates
