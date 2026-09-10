# Linux Installation mit Praxis: POML Templates, SM und set_context

## Übersicht

Diese Dokumentation erklärt wie man in Praxis eine VM-gestützte Linux-Installation
mit POML-Templates, SM-Dateien (Statemachine) und `set_context` baut.

Das System besteht aus drei Schichten:

```
┌─────────────────────────────────────────────────────────┐
│  SM-Datei (State Machine)                               │
│  Definiert States, Transitions, Overrides               │
│  Wechselt das system_template je nach Context-Variable  │
├─────────────────────────────────────────────────────────┤
│  POML-Templates                                         │
│  Enthält System-Prompts, Task-Beschreibungen,           │
│  Screen-spezifische Anweisungen, Include-Logik          │
├─────────────────────────────────────────────────────────┤
│  set_context (Tool)                                     │
│  Agent setzt Context-Variablen (z.B. screen="disk")     │
│  Triggert SM-Transition → neues system_template         │
└─────────────────────────────────────────────────────────┘
```

---

## 1. Dateistruktur

```
praxis/
├── contexts/
│   └── linux-install.cl          # SM State Machine
├── templates/
│   ├── system.poml               # Existierendes Default-Template
│   ├── linux/
│   │   └── void-linux.poml       # Haupt-System-Prompt für Linux-Installation
│   ├── installer/
│   │   └── linux-installers.poml # Task-Template (Schritt-für-Schritt)
│   └── applications/
│       ├── boot.poml             # Screen: Boot-Menü
│       ├── cf-disk.poml          # Screen: cfdisk Partitionierung
│       ├── f-disk.poml           # Screen: fdisk Partitionierung
│       ├── filesystem.poml       # Screen: Filesystem erstellen
│       ├── packages.poml         # Screen: Pakete installieren
│       ├── network.poml          # Screen: Netzwerk konfigurieren
│       └── bootloader.poml       # Screen: GRUB installieren
```

---

## 2. SM-Datei: `contexts/linux-install.cl`

### Was ist SM?

SM (Statemachine) ist eine State Machine die in Praxis eingebaut ist.
Sie wird in `src/sm.rs` geparsed. Eine SM-Datei definiert:

- **States** mit Variablen (z.B. welches Template zu verwenden ist)
- **Transitions** zwischen States basierend auf Context-Bedingungen
- **Overrides** die Context-Variablen ändern wenn Bedingungen erfüllt sind

### SM-Syntax (Referenz aus `src/sm.rs`)

```cl
# Metadata
@name "Workflow Name"
@version "1.0"
@steps [state1, state2, state3]

# Default-Variablen (gelten für alle States wenn nicht überschrieben)
key = value

# State-Definition
[state state_name]
key = value
system_template = template/pfad

# Transitions (State-Wechsel)
[transitions]
from_state -> to_state : when bedingung

# Auto-Regeln (automatische State-Bestimmung)
[auto]
bedingung -> use ziel_state

# Overrides (Context-Variablen ändern)
[overrides]
if bedingung -> key = value
```

### Vollständige SM-Datei

```cl
# Linux Installation Workflow fuer Void Linux
# Der Agent navigiert Screens via set_context(screen, "...")
# Die SM wechselt dann automatisch das system_template

@name "linux-install"
@version "1.0"
@steps [boot, disk, filesystem, packages, network, bootloader, done]

# Default-Werte
system_template = linux/void-linux

[state boot]
system_template = linux/void-linux

[state disk]
system_template = applications/cf-disk

[state filesystem]
system_template = applications/filesystem

[state packages]
system_template = applications/packages

[state network]
system_template = applications/network

[state bootloader]
system_template = applications/bootloader

[state done]
system_template = system

# Transitions: Wechselt den State wenn die Context-Variable "screen"
# den entsprechenden Wert hat. Der Agent setzt "screen" via set_context.

[transitions]
boot -> disk : when screen == "disk"
boot -> filesystem : when screen == "filesystem"
boot -> packages : when screen == "packages"
boot -> network : when screen == "network"
boot -> bootloader : when screen == "bootloader"
boot -> done : when screen == "done"
disk -> filesystem : when screen == "filesystem"
disk -> packages : when screen == "packages"
disk -> network : when screen == "network"
disk -> bootloader : when screen == "bootloader"
disk -> done : when screen == "done"
filesystem -> packages : when screen == "packages"
filesystem -> network : when screen == "network"
filesystem -> bootloader : when screen == "bootloader"
filesystem -> done : when screen == "done"
packages -> network : when screen == "network"
packages -> bootloader : when screen == "bootloader"
packages -> done : when screen == "done"
network -> bootloader : when screen == "bootloader"
network -> done : when screen == "done"
bootloader -> done : when screen == "done"

# Auto-Regel: Fallback auf boot wenn kein screen gesetzt ist
[auto]
screen == "" -> use boot
```

### Wie die SM funktioniert (Code-Flow)

In `src/gateway/agent_loop.rs` passiert folgendes pro Turn:

```
JEDER TURN (in der Loop, line 293+):

1. Context wird frisch aus DB geladen (line 322-325)
   → pickt set_context Änderungen vom vorigen Turn auf
2. SM wird angewandt (line 328-337):
   a. sm::apply_to_context() liest den frischen Context
   b. Evaluiert Transitions (z.B. screen == "disk" → wechselt State zu disk)
   c. Wendet State-Variablen an (z.B. system_template = "applications/cf-disk")
   d. Wendet Overrides an
3. Context wird in DB gespeichert (line 339)
4. System-Prompt wird gebaut (line 342):
   a. Liest ctx.settings.system_template (z.B. "applications/cf-disk")
   b. Rendert templates/applications/cf-disk.poml
5. LLM wird aufgerufen mit dem gerenderten System-Prompt
6. LLM führt Tools aus (z.B. set_context(screen="filesystem"))
   → wird in DB gespeichert
7. Nächster Turn → zurück zu Schritt 1
```

Der entscheidende Punkt: Die SM wird **pro Turn** angewandt, nicht einmalig.
Das bedeutet `set_context` Änderungen werden im **nächsten Turn sofort**
berücksichtigt — der System-Prompt wechselt ohne Verzögerung.

---

## 3. POML-Templates

### Was ist POML?

POML (Prompt Orchestration Markup Language) ist eine XML-ähnliche Sprache
für strukturierte LLM-Prompts. Dokumentation:
https://microsoft.github.io/poml/latest/

Wichtigste Features:
- `{{variable}}` - Variablen-Substitution
- `<section if="bedingung">` - Conditional Rendering
- `<include src="datei.poml" />` - Datei einbinden
- `<let name="var" value="..." />` - Variablen definieren
- `<for="item in list">` - Schleifen

### 3.1 Haupt-Template: `templates/linux/void-linux.poml`

Das ist das **System-Template** das als system_prompt an das LLM gesendet wird.
Es wird gewählt wenn `system_template = "linux/void-linux"` in der SM steht.

```xml
<poml>
  <role>You are a Linux installation expert guiding through a Void Linux
  installation inside a QEMU virtual machine. You can see the VM screen
  via screenshots and control it via VM tools.</role>

  <task>
    <p>Current screen: <b>{{screen}}</b>. Turn: <b>{{turn}}</b>.
    User: <b>{{user_name}}</b>.</p>
    <p>You are guiding a Void Linux installation step by step.
    After each action, you MUST update the screen context so the
    correct instructions are shown next turn.</p>
  </task>

  <cp caption="Screen Navigation — MANDATORY">
    <p>After EVERY action, call <code inline="true">set_context</code>
    with key="screen" and one of these values:</p>
    <list>
      <item><code inline="true">boot</code> — Initial boot / menu screen</item>
      <item><code inline="true">disk</code> — Disk partitioning (cfdisk/fdisk)</item>
      <item><code inline="true">filesystem</code> — Filesystem creation (mkfs)</item>
      <item><code inline="true">packages</code> — Package installation (xbps-install)</item>
      <item><code inline="true">network</code> — Network configuration</item>
      <item><code inline="true">bootloader</code> — GRUB bootloader installation</item>
      <item><code inline="true">done</code> — Installation complete</item>
    </list>
    <p>This changes the system prompt for the next turn to show the
    correct instructions for that screen.</p>
  </cp>

  <cp caption="Screenshot Analysis">
    <p>When a VM screenshot is provided:</p>
    <list>
      <item>Check the BOTTOM BAR of the screen to identify which
      installer step you are on</item>
      <item>State exactly what is displayed on screen</item>
      <item>Determine what input or action is needed next</item>
      <item>Execute the action using vm_keys, vm_shell, or other VM tools</item>
      <item>Call set_context with the correct screen value</item>
    </list>
  </cp>

  <cp caption="Available Tools" if="mode == 'agent'">
    <p>{{tools}}</p>
  </cp>

  <cp caption="Rules">
    <list>
      <item>Always take a screenshot FIRST to see the current state</item>
      <item>After each action, set the screen context</item>
      <item>Use vm_keys for keyboard navigation, vm_shell for commands</item>
      <item>Wait for the screen to update before proceeding</item>
      <item>If unsure which screen you are on, check the bottom bar</item>
    </list>
  </cp>
</poml>
```

### 3.2 Screen-Template Beispiel: `templates/applications/cf-disk.poml`

Dieses Template wird geladen wenn `system_template = "applications/cf-disk"`
in der SM steht (also wenn `screen == "disk"`).

```xml
<poml>
  <cp caption="Screen: Disk Partitioning (cfdisk)">
    <p>You are at the disk partitioning screen. The tool <b>cfdisk</b>
    is a graphical partition manager.</p>
  </cp>

  <cp caption="Step-by-Step Instructions">
    <stepwiseinstructions>
      <step>Select the target disk. Usually <code inline="true">/dev/sda</code>
      or <code inline="true">/dev/vda</code> in a VM.</step>
      <step>If prompted, create a new <b>GPT</b> partition table.</step>
      <step>Create an <b>EFI System Partition</b>:
        <list>
          <item>Select "New" → Size: 512M → Type: EFI System</item>
        </list>
      </step>
      <step>Create a <b>Swap Partition</b>:
        <list>
          <item>Select "New" → Size: 2G → Type: Linux swap</item>
        </list>
      </step>
      <step>Create the <b>Root Partition</b>:
        <list>
          <item>Select "New" → Size: remaining space → Type: Linux filesystem</item>
        </list>
      </step>
      <step>Select <b>"Write"</b> to write the partition table.
      Type "yes" to confirm.</step>
      <step>Select <b>"Quit"</b> to exit cfdisk.</step>
    </stepwiseinstructions>
  </cp>

  <cp caption="Navigation Keys for cfdisk">
    <list>
      <item><b>Arrow Up/Down</b> — Select partition</item>
      <item><b>Arrow Left/Right</b> — Select action (New, Delete, Quit, etc.)</item>
      <item><b>Enter</b> — Confirm selection</item>
    </list>
  </cp>

  <cp caption="After Completing This Screen">
    <p>After partitioning is done, call
    <code inline="true">set_context(key="screen", value="filesystem")</code>
    to proceed to filesystem creation.</p>
  </cp>
</poml>
```

### 3.3 Task-Template: `templates/installer/linux-installers.poml`

Das Task-Template wird über `task_template = installer/linux-installers`
in der SM geladen. Es bindet das passende Screen-Template ein.

```xml
<poml>
  <task>
    <p>Install Void Linux on the VM. Current step: <b>{{active_state}}</b>.</p>
    <p>Follow the instructions for the current screen below.</p>
  </task>

  <cp caption="Current Screen Instructions">
    <include src="applications/{{active_state}}.poml"
             if="active_state" />
    <p if="!active_state">No specific instructions loaded.
    Use the system prompt guidance.</p>
  </cp>

  <cp caption="General Installation Flow">
    <list>
      <item><b>boot</b> — Boot from ISO, select installation mode</item>
      <item><b>disk</b> — Partition the disk (cfdisk or fdisk)</item>
      <item><b>filesystem</b> — Create filesystems (mkfs.vfat, mkfs.ext4)</item>
      <item><b>packages</b> — Mount filesystems and install base system</item>
      <item><b>network</b> — Configure network (DHCP or static)</item>
      <item><b>bootloader</b> — Install and configure GRUB</item>
      <item><b>done</b> — Reboot into installed system</item>
    </list>
  </cp>
</poml>
```

### 3.4 Weitere Screen-Templates

Jedes `applications/*.poml` folgt dem gleichen Muster wie `cf-disk.poml`:

| Datei | Screen-Wert | Inhalt |
|---|---|---|
| `boot.poml` | `"boot"` | ISO-Boot-Menü, Installationsmodus wählen |
| `cf-disk.poml` | `"disk"` | cfdisk Partitionierung |
| `f-disk.poml` | `"disk"` | fdisk Partitionierung (Alternative) |
| `filesystem.poml` | `"filesystem"` | mkfs.vfat, mkfs.ext4, mkswap |
| `packages.poml` | `"packages"` | mount, xbps-install base-system |
| `network.poml` | `"network"` | dhcpcd, /etc/resolv.conf |
| `bootloader.poml` | `"bootloader"` | grub-install, grub-mkconfig |

---

## 4. set_context: Wie der System-Prompt gewechselt wird

### 4.1 Was ist set_context?

`set_context` ist ein eingebautes Tool in Praxis (definiert in `src/db/tools.rs:147`).

Tool-Definition:
```json
{
  "name": "set_context",
  "description": "Set context variable",
  "parameters": {
    "type": "object",
    "properties": {
      "key": {"type": "string"},
      "value": {}
    },
    "required": ["key", "value"]
  }
}
```

### 4.2 Was passiert beim Aufruf?

Wenn der Agent `set_context(key="screen", value="disk")` aufruft:

```
TURN N:
1. Agent ruft set_context auf (src/gateway/agent_loop.rs:1282)
   ↓
2. db.merge_context() speichert {"screen": "disk"} in SQLite
   ↓
3. Turn N endet (LLM fertig)

TURN N+1:
4. Context wird frisch aus DB geladen (line 322)
   → screen = "disk" ist jetzt im Context
   ↓
5. SM wird angewandt (line 328):
   - sm::apply_to_context() liest den frischen Context
   - Transition: boot → disk (screen == "disk")
   - State-Vars: system_template = "applications/cf-disk"
   ↓
6. Context wird in DB gespeichert (line 339)
   ↓
7. build_system_prompt() wird aufgerufen (line 342)
   - Liest ctx.settings.system_template → "applications/cf-disk"
   - Rendert templates/applications/cf-disk.poml
   ↓
8. LLM sieht den gerenderten cf-disk Prompt als System-Prompt
```

### 4.3 Vollständiger Turn-Flow

```
User: "Installiere Void Linux"
         │
         ▼
┌─ Turn 1 ──────────────────────────────────────────────┐
│ 1. Context laden (default active_state = "boot")      │
│ 2. SM anwenden → system_template = "linux/void-linux" │
│ 3. System-Prompt: linux/void-linux.poml               │
│ 4. LLM sieht: "Du bist ein Linux Experte..."          │
│ 5. LLM nimmt Screenshot → sieht Boot-Menü             │
│ 6. LLM navigiert Boot-Menü mit vm_keys                │
│ 7. LLM ruft auf: set_context(screen="disk")           │
│    → Response: "Ich navigiere zum Partitionierer..."   │
└───────────────────────────────────────────────────────┘
         │
         ▼
┌─ Turn 2 ──────────────────────────────────────────────┐
│ 1. Context laden (screen = "disk")                    │
│ 2. SM anwenden:                                       │
│    - Transition: boot → disk (screen == "disk")       │
│    - State-Vars: system_template = "applications/     │
│      cf-disk"                                         │
│ 3. System-Prompt: applications/cf-disk.poml           │
│ 4. LLM sieht: "Du bist am Disk Partitioning Screen"   │
│    + Schritt-für-Schritt Anleitung für cfdisk         │
│ 5. LLM nimmt Screenshot → verifiziert Screen          │
│ 6. LLM führt cfdisk-Navigation durch mit vm_keys     │
│ 7. LLM ruft auf: set_context(screen="filesystem")    │
└───────────────────────────────────────────────────────┘
         │
         ▼
┌─ Turn 3 ──────────────────────────────────────────────┐
│ 1. Context laden (screen = "filesystem")              │
│ 2. SM anwenden:                                       │
│    - Transition: disk → filesystem                    │
│    - system_template = "applications/filesystem"      │
│ 3. System-Prompt: applications/filesystem.poml        │
│ 4. LLM sieht Filesystem-Erstellungs-Anleitung        │
│ 5. ...                                                │
└───────────────────────────────────────────────────────┘
         │
         ▼
       (... weiter bis bootloader → done ...)
```

### 4.4 Alternativer Ansatz: Ohne SM Transitions

Falls du die SM-Transitions NICHT nutzen willst, kannst du auch
direkt `system_template` über `set_context` setzen:

```
# Agent ruft auf:
set_context(key="system_template", value="applications/cf-disk")

# build_system_prompt() liest dann direkt:
# ctx.settings.system_template → "applications/cf-disk"
```

Das ist einfacher aber weniger strukturiert — der Agent muss dann
selbst wissen welches Template zu welchem Screen gehört.

---

## 5. POML-Template Referenz

### 5.1 Variablen die im Context verfügbar sind

Diese Variablen werden in `build_system_prompt()` (line 1011-1069)
aus dem Context in das POML-Template injiziert:

| Variable | Typ | Beschreibung |
|---|---|---|
| `{{user_id}}` | string | User-ID |
| `{{turn}}` | number | Aktuelle Turn-Nummer |
| `{{mode}}` | string | "agent" oder "chat" |
| `{{system_info}}` | string | "Praxis v0.3.0" |
| `{{user_message}}` | string | Letzte User-Nachricht |
| `{{user_prompt}}` | string | Letzte User-Nachricht |
| `{{time}}` | string | Aktuelle Zeit |
| `{{user_name}}` | string | Username |
| `{{path}}` | string | Working Directory |
| `{{active_state}}` | string | Aktiver SM-State |
| `{{screen}}` | string | Screen-Wert (von set_context) |
| `{{skills}}` | array | Verfügbare Skills |
| `{{tools}}` | array | Verfügbare Tools |
| `{{memory}}` | object | Gespeicherte Fakten/Preferences |
| `{{custom_data}}` | object | Custom Context Data |

Zusätzlich werden alle State-Variablen aus der SM injiziert
(z.B. `system_template`, `task_template`).

### 5.2 Conditional Sections

```xml
<!-- Wird nur gerendert wenn screen == "disk" ist -->
<section if="screen == 'disk'">
  <p>Du bist am Disk Partitioning Screen.</p>
</section>

<!-- Wird nur gerendert wenn screen NICHT "disk" ist -->
<section if="screen != 'disk'">
  <p>Du bist nicht am Disk Screen.</p>
</section>

<!-- Wird nur gerendert wenn screen existiert und nicht leer ist -->
<section if="screen">
  <p>Screen ist gesetzt: {{screen}}</p>
</section>
```

### 5.3 Includes

```xml
<!-- Bindet eine andere POML-Datei ein -->
<include src="applications/cf-disk.poml" />

<!-- Dynamisches Include basierend auf Variable -->
<include src="applications/{{active_state}}.poml" if="active_state" />

<!-- Conditional Include -->
<include src="applications/cf-disk.poml" if="screen == 'disk'" />
```

### 5.4 Schritt-für-Schritt Anweisungen

```xml
<stepwiseinstructions>
  <step>Erster Schritt</step>
  <step>Zweiter Schritt</step>
  <step>Dritter Schritt</step>
</stepwiseinstructions>
```

### 5.5 Listen

```xml
<list>
  <item>Erstes Item</item>
  <item>Zweites Item</item>
  <item for="item in myList">{{item}}</item>
</list>
```

### 5.6 Code

```xml
<!-- Inline Code -->
<p>Benutze den Befehl <code inline="true">xbps-install -Su</code></p>

<!-- Code Block -->
<code lang="bash">
  xbps-install -Su
  xbps-install base-system
</code>
```

---

## 6. VM-Tools die der Agent nutzen kann

Für die Linux-Installation relevante Tools (definiert in `src/db/tools.rs`):

| Tool | Beschreibung |
|---|---|
| `vm_start` | VM starten |
| `vm_stop` | VM stoppen |
| `vm_screenshot` | Screenshot des VM-Bildschirms |
| `vm_keys` | Tastatureingaben senden (für Installer-Navigation) |
| `vm_shell` | Shell-Befehl in der VM ausführen |
| `vm_input` | Vim-ähnliche Navigation |
| `vm_shortcut` | Keyboard-Shortcuts senden |
| `vm_key_combo` | Tastenkombinationen |
| `vm_type_fast` | Schnellen Text eingeben |
| `vm_wait_for_text` | Warten bis Text auf dem Screen erscheint |
| `vm_install` | Von ISO booten |
| `vm_file_read` | Datei in der VM lesen |
| `vm_package_install` | Paket installieren |

### Auto-Screenshot

Wenn `vm_screenshot_enabled = true` im Context, wird nach jedem
VM-Tool-Call automatisch ein Screenshot gemacht und als Bild
an das LLM gesendet (line 362-386 in agent_loop.rs).

Der LLM sieht dann:
- Den Text-Prompt (System + User Message)
- Das aktuelle VM-Screenshot als Bild

---

## 7. Setup-Anleitung

### 7.1 SM-Datei erstellen

```bash
# Erstelle die SM-Datei
cat > contexts/linux-install.cl << 'EOF'
# (Inhalt aus Abschnitt 2)
EOF
```

### 7.2 Templates erstellen

```bash
# Erstelle Verzeichnisse
mkdir -p templates/linux
mkdir -p templates/installer
mkdir -p templates/applications

# Erstelle Templates
# (Inhalt aus Abschnitt 3)
```

### 7.3 SM aktivieren

Über die Praxis CLI oder das Dashboard:

```bash
# Via CLI
praxis run
# Dann im Dashboard: Settings → SM File → "linux-install.cl"
```

Oder via set_context:
```
set_context(key="cl_file", value="contexts/linux-install.cl")
```

### 7.4 VM starten und ISO mounten

```bash
# ISO hinzufügen
praxis vm add-iso /path/to/void-linux.iso

# VM starten
praxis vm start

# Oder via Tool: vm_install(iso_name="void-linux")
```

### 7.5 Installation starten

```
User: "Installiere Void Linux auf der VM"
```

Der Agent beginnt automatisch mit dem Boot-Screen und navigiert
durch die Installation, wobei er bei jedem Schritt den Screen-Context
aktualisiert.

---

## 8. Debugging

### Context anzeigen

```
# Via Tool
get_context()

# Via CLI
praxis vm status
```

### SM-Datei validieren

Die SM wird beim Laden geparst. Fehler werden geloggt:
```
# Logs anzeigen
praxis service logs
```

### POML-Rendering debuggen

Das POML CLI rendert die Templates. Fehler werden als Warnung geloggt
und auf Fallback-Rendering (einfache {{variable}}-Substitution) umgestellt.

Logs zeigen:
```
WARN POML CLI error: ...
WARN POML CLI not found: ..., using simple template
```

### Template-Wechsel verfolgen

In den Logs siehst du:
```
INFO build_system_prompt called with user_message: ...
INFO context_json user_prompt: ...
DEBUG Rendered system prompt (first 2000 chars): ...
```

---

## 9. Zusammenfassung

```
┌──────────────────────────────────────────────────────────┐
│                    User-Nachricht                        │
│              "Installiere Void Linux"                    │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│               SM: linux-install.cl                       │
│  1. Lade Context (screen="", active_state="boot")        │
│  2. Auto-Regel → State "boot"                            │
│  3. State-Vars: system_template = "linux/void-linux"     │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│            POML: linux/void-linux.poml                   │
│  Rendert System-Prompt mit:                              │
│  - Rolle: Linux Installations-Experte                    │
│  - Screen Navigation Rules                               │
│  - Screenshot Analysis Anweisungen                       │
│  - Verfügbare Tools                                      │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│                    LLM Turn                              │
│  1. Screenshot analysieren                               │
│  2. Boot-Menü navigieren (vm_keys)                       │
│  3. set_context(screen="disk") aufrufen                  │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│               SM: Nächster Turn                          │
│  1. Context laden (screen="disk")                        │
│  2. Transition: boot → disk (screen == "disk")           │
│  3. State-Vars: system_template = "applications/cf-disk" │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────────────┐
│            POML: applications/cf-disk.poml               │
│  Rendert Screen-spezifischen Prompt mit:                 │
│  - cfdisk Schritt-für-Schritt Anleitung                  │
│  - Tasten-Navigation                                     │
│  - Nächster Screen-Hinweis                               │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
                  (... wiederholt ...)
```

### Kernprinzipien

1. **SM steuert das Template**: Die SM-Datei entscheidet welches
   POML-Template als System-Prompt gerendert wird.

2. **set_context triggert Wechsel**: Der Agent setzt die Context-Variable
   "screen" → die SM erkennt die Änderung → wechselt den State →
   ändert das system_template.

3. **POML enthält die Anweisungen**: Jedes Screen-Template enthält
   spezifische Schritt-für-Schritt-Anweisungen für genau einen
   Installer-Screen.

4. **Screenshots als Feedback**: Der Agent sieht via Screenshot was auf
   dem Bildschirm ist und kann so die korrekte Navigation bestätigen.
