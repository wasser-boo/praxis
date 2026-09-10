# LLM-Laufzeit: Routing, Limits und Wiederholungen

## Befund und Geltungsbereich

Bei der Untersuchung am 10.09.2026 lief Praxis aus `/workspace/release`, der Quellcode aus `/workspace/praxis`. Im untersuchten Tageslog standen 13 Fehlerketten:

1. Standardprovider `openai`, aber nur `ollama` und `llamacpp` registriert.
2. Ollama-Transportfehler wiederholt nach ungefähr 30 Sekunden.
3. Ungefragter Fallback auf den nicht laufenden Llama.cpp-Endpunkt.
4. Nur `All LLM providers failed` als abschließende Meldung.

Das war **kein nachgewiesenes HTTP-429-Rate-Limit**. Im Ollama-Log gab es teilweise HTTP 400. Außerdem lief Ollama aus einem gelöschten Arbeitsverzeichnis; ein lokaler Embedding-Runner brach nachweislich mit `cannot get current path` ab. Diese Prozess-/Deployment-Probleme lassen sich nicht durch beliebige Retries reparieren.

Die Änderungen hier sind Quellcodeänderungen. Sie ersetzen weder die laufende Binary noch `.env`, Secrets oder Datenbanken und starten keine Dienste neu.

## Verhalten

- `USE_PROVIDER` bleibt standardmäßig `openai`. Ein nicht registrierter ausgewählter Provider ist jetzt ein **klarer Konfigurationsfehler**, bereits vor dem Start von VM/Discord. Es wird kein anderer Provider stillschweigend ausgewählt. Lokale Adapter können registriert sein, ohne dass ihr Server läuft; Registrierung ist keine Erreichbarkeitsgarantie.
- Fallbacks sind nur über `LLM_FALLBACK_PROVIDERS` ausdrücklich erlaubt. Diese Auswahl kann Kosten und die Empfänger der Unterhaltung ändern. Keine automatischen Fallbacks auf Ollama oder Llama.cpp mehr.
- Chat, native Ollama-Streams, Tool-Folgeaufrufe und Zusammenfassungen durchlaufen dieselbe Retry-Steuerung. Andere Adapter liefern weiterhin vollständige Antworten statt nativer Token-Streams.
- Das Gesamtbudget gilt **pro logischem LLM-Aufruf**, nicht für die gesamte mehrstündige Aufgabe. Wiederholungen erhöhen keine Agent-Turn-/Tool-Zähler. Fallbacks teilen sich dasselbe Zeit- und Versuchslimit; je verbleibendem Fallback wird nach Möglichkeit ein Versuch reserviert. Ein Fallback benutzt sein eigenes konfiguriertes Standardmodell, nicht den Modellnamen des vorherigen Providers. Vision-Routing wird auch im Streaming-Pfad berücksichtigt.
- Wiederholt werden Transportabbrüche, Timeouts, 408, echte 429 sowie 500/502/503/504/529. Permanente Request-/Modellfehler, Authentifizierungsfehler, Guthaben-/Quota-Erschöpfung und ungültige/leere Antworten werden nicht wiederholt und lösen keinen automatischen Providerwechsel aus. Bekannte Fehler in HTTP-200-JSON-Antworten werden erkannt, statt als leerer Erfolg durchzugehen.
- Exponentielle Wartezeit mit Equal Jitter (halber bis voller berechneter Abstand). `Retry-After` wird als Sekunden oder HTTP-Datum gelesen und ist eine **Mindestwartezeit**, auch wenn sie über dem normalen Backoff-Maximum liegt. Reicht das Gesamtbudget nicht, endet der Aufruf ohne vorzeitigen Retry.
- Ein gemeinsamer Cooldown wird auch nach dem letzten temporären Fehler aktualisiert. Neue Aufgaben müssen diesen ebenfalls respektieren. Aktive Netzwerk-Permits werden vor Retry-Wartezeiten freigegeben.
- Limiter gelten pro Router/Prozess und Endpoint-Origin/Zugang; konfigurierte Alias-Provider mit gleichem Origin und Key teilen denselben Gate. Alle Modelle dieses Zugangs teilen die Limits. Andere Praxis-Prozesse oder direkte Ollama-Clients werden **nicht** mitgezählt. Der bestehende eingehende HTTP-Limiter bleibt davon unabhängig.
- Request-Limits verteilen Starts gleichmäßig, ohne große Bursts. Optionale TPM-Limits reservieren geschätzte Eingabe- plus maximale Ausgabetokens für jeden Versuch und gleichen mit tatsächlicher Usage ab, soweit vorhanden. Das ist eine Näherung, kein Modell-Tokenizer; Bilder werden pauschal geschätzt. Fehlgeschlagene Versuche behalten vorsichtshalber ihre Reservierung. Ein einzelner zu großer Request wird abgewiesen, statt endlos auf Tokenkapazität zu warten.

### Streaming und Nebenwirkungen

Automatische Wiederholung findet ausschließlich **unterhalb der Tool-Ausführung** statt. Bereits ausgeführte Terminalbefehle, Dateiänderungen und Uploads werden durch einen LLM-Retry nicht erneut ausgeführt. Im Gateway bleiben Assistant-Toolcalls und Tool-Ergebnisse in der vorhandenen SQLite-Historie; auch wegen Abbruch/Limit/Validierung übersprungene Agent-Tools erhalten einen gespeicherten Ergebnisdatensatz.

Wurden bereits Textdeltas an das Dashboard ausgegeben, wird ein unterbrochener Stream **nicht automatisch neu generiert oder durch einen Fallback ersetzt**. `stream_abort` entfernt die vorläufige Antwortblase; es wird keine endgültige Assistant-Antwort und kein Tool aus dieser unvollständigen Antwort übernommen. Ein späterer Aufruf beginnt mit leerem Stream-Puffer. Server und aktualisiertes Dashboard deshalb zusammen ausrollen.

Es gibt keine Garantie für Exactly-once-Ausführung über Prozessabstürze hinweg: Ein Absturz zwischen externem Tool-Effekt und DB-Speicherung bleibt ein Sonderfall. Die öffentliche ältere `chat_with_tools`-API hält ihre übergebene Unterhaltung weiterhin im Speicher; sie ist kein dauerhafter Job-Checkpoint. Nach ausgeschöpftem Retry-Budget erfolgt **kein automatischer Neustart der ganzen Aufgabe**.

### Tool-only-Antworten sind keine leeren Endantworten

Ein Modell kann ausschließlich `tool_calls` zurückgeben, ohne Text. Das ist eine gültige Fortsetzung, kein Abschluss und kein Grund für TTS. Die Logs zeigen deshalb Textbytes **und** Tool-Anzahl. Eine Antwort ohne Text **und ohne Tools** bleibt ein expliziter `InvalidResponse`-Fehler; sie wird nicht blind wiederholt.

Der Chat-Pfad (`settings.max_llm_turns` null/≤1) führt auch mehrere aufeinanderfolgende Tool-Runden aus, etwa `use_skill` → `read_file` → Dateioperation → Textantwort. Tool-Definitionen und Kontext werden für jede Fortsetzung aktualisiert, die Ergebnisse dauerhaft gespeichert. `settings.max_tool_calls` begrenzt dabei die gesamten angeforderten Calls pro Nachricht (Standard 5, ≤0 deaktiviert Ausführung); ungültige Calls verbrauchen ebenfalls Budget. Die Obergrenze wird am Nachrichtenbeginn festgehalten und kann nicht durch ein Tool für die laufende Nachricht erhöht werden. Nach Budgetverbrauch ist höchstens eine textuelle Abschlussanfrage ohne Tools erlaubt. Fordert das Modell dennoch weitere Tools an, erhalten diese gespeicherte Ablehnungen und der Nutzer einen Limitfehler statt einer leeren Erfolgsmeldung.

Für längere Aufgaben bleibt der explizite Multi-Turn-Modus verfügbar: dort begrenzt `max_llm_turns` die LLM-Runden und `max_tool_calls` die Ausführungen je Runde. Tool-Historie der **laufenden** Aufgabe bleibt auch bei deaktivierter älterer Tool-Historie erhalten. Ein Turn-Limit ohne Endantwort wird gemeldet; frühere Antworten anderer Aufgaben werden nicht als Ergebnis ausgegeben. Kein automatisches Erhöhen der gespeicherten Limits oder Wiederanlaufen einer abgebrochenen Aufgabe. Leere/Whitespace-Texte starten keine automatische TTS-Aufgabe.

### Fortschritt und Abbrechen

Dashboard/SSE und Gateway-WebSocket zeigen Queue-/Cooldown-/Retry-Meldungen. WebSocket-Pings und `agent_input` bleiben während einer Aufgabe bedienbar; `{"type":"stop","user_id":"…"}` bricht LLM-Aufrufe und Wartezeiten ab. Die bisherigen Dashboard-/Discord-Stop-Aktionen verwenden dieselbe Cancellation.

Pro Benutzer ist eine Aufgabe gleichzeitig erlaubt. Stoppen gibt diese Belegung nicht vorzeitig frei: Ein bereits laufendes Tool darf noch fertig werden und sein Ergebnis speichern. Neue Tools werden danach nicht gestartet. Auch nach einem WS-Verbindungsabbruch wird eine bereits laufende Tool-Ausführung nicht einfach verworfen. **Tool-eigene Laufzeiten/Prozessbeendigung sind kein Teil des LLM-Timeouts.**

## Konfiguration

Alle Einstellungen werden beim Start gelesen; Änderungen benötigen einen Neustart. Ungültige Zahlen führen zu einem Konfigurationsfehler, nicht zu stillen Defaults.

| Variable | Default | Bedeutung / zulässiger Bereich |
|---|---:|---|
| `LLM_FALLBACK_PROVIDERS` | leer | Kommagetrennte explizite Provider-Auswahl; alle müssen registriert sein |
| `LLM_MAX_ATTEMPTS` | `5` | Gesamtzahl der Versuche einschließlich erstem Versuch und Fallbacks; 1–20 |
| `LLM_MAX_CONCURRENT` | `1` | Gleichzeitige ausgehende Versuche pro Zugang; 1–64 |
| `LLM_MAX_QUEUE` | `32` | Zusätzliche zugelassene Aufgaben pro Zugang; 0–10000; Wartende/Retry-Aufgaben belegen Queue-Kapazität |
| `LLM_REQUESTS_PER_MINUTE` | `0` | Gleichmäßige Request-Pacing-Rate; 0 = kein festes RPM-Limit; maximal 1000000 |
| `LLM_TOKENS_PER_MINUTE` | `0` | Geschätztes rollendes 60-Sekunden-Tokenbudget; 0 = deaktiviert; maximal 1000000000 |
| `LLM_RETRY_BASE_MS` | `1000` | Initialer Backoff; 1–300000 ms |
| `LLM_RETRY_MAX_MS` | `30000` | Maximaler exponentieller Backoff; mindestens Base, maximal 300000 ms; begrenzt nicht `Retry-After` |
| `LLM_REQUEST_TIMEOUT_MS` | `180000` | Pro Versuch inklusive HTTP-Upload, Antwort/Stream und Parsing; 1–86400000 ms |
| `LLM_TOTAL_TIMEOUT_MS` | `300000` | Queue, alle Versuche und Wartezeiten zusammen; 1–86400000 ms |

TCP-Verbindungsaufbau ist zusätzlich auf 10 Sekunden begrenzt. Die HTTP-Bibliothek führt keine verdeckten eigenen Retries aus. Unter Linux/Android/Fuchsia wird ihr standardmäßiger 30-Sekunden-`TCP_USER_TIMEOUT` entfernt; stattdessen begrenzt das explizite Aufrufbudget die Operation. Dieser Socket-Timer betrifft unbestätigte Upload-Daten, **nicht** generell langsame Modellgenerierung. Dass er die beobachteten Abbrüche verursacht hat, ist damit noch nicht bewiesen.

Providerfehler enthalten Kategorie, HTTP-Status, einen kontrollierten Transportgrund (z. B. Verbindung zurückgesetzt, DNS, TLS, Timeout), optional Request-ID und Retry-After. Rohe Provider-Fehlerbodies, URLs und Keys werden nicht in diese Fehler übernommen. Auch wiederholtes Loggen ganzer Ollama-/MiMo-Requests wurde entfernt. Bestehende Tool-/Aktivitätslogs sind dadurch nicht automatisch vollständig anonymisiert.

## Sichere Inbetriebnahme der untersuchten Installation

**Nicht automatisch ausgeführt. Erst nach erfolgreichem Build und geplanter Unterbrechung laufender Aufgaben:**

1. Vorhandene Binary, `.env` und Dashboard-Dateien sichern. Keine Secrets in Chat oder Kommandozeilenargumente kopieren.
2. In `/workspace/release/.env` die vorhandenen Modell-/Endpunktangaben erhalten und den tatsächlichen Provider ausdrücklich setzen:

   ```env
   USE_PROVIDER=ollama
   OLLAMA_API_BASE=http://localhost:11434
   OLLAMA_MODEL=glm-5.3-flash:cloud
   LLM_FALLBACK_PROVIDERS=
   LLM_MAX_CONCURRENT=1
   ```

   Der Modellname stammt aus der untersuchten Installation; auf anderen Installationen ein tatsächlich verfügbares Modell verwenden. RPM/TPM erst passend zum eigenen Tarif setzen, nicht willkürlich übernehmen.
3. Ollama nach Beendigung laufender Arbeit kontrolliert stoppen und aus einem dauerhaften Arbeitsverzeichnis neu starten, z. B. `cd /home/marvin` vor `ollama serve`. Nicht aus einem anschließend gelöschten/ersetzten Checkout starten und nicht parallel einen zweiten Server auf demselben Port starten.
4. Praxis mit den benötigten Features bauen, für diese Discord-Voice-Installation normalerweise `cargo build --release --locked --features songbird --jobs 1`. Aktualisierte Binary und Dashboard gemeinsam bereitstellen. `praxis repair-assets --directory /workspace/release --update-dashboard` kann Dashboard-Assets mit Backups aktualisieren; **kein erneutes Onboarding** für ein Upgrade.
5. Praxis kontrolliert neu starten und Browser aktualisieren. Mit einer kurzen Aufgabe beginnen, danach eine lange Tool-Aufgabe testen. Die neue Fehlerkategorie/Request-ID beobachten, insbesondere falls weiterhin HTTP 400 oder Transportabbrüche auftreten. Kein pauschales Wiederholen von HTTP 400.

Keys werden beim Start in den Router übernommen. Dashboard-Secret-Änderungen erfordern weiterhin einen Neustart. Beim Env-basierten Start wird jetzt auch `OPENROUTER_API_KEY` übernommen. Der interaktive Wizard speichert die ausgewählte `USE_PROVIDER`-Zeile zuverlässig; die alten Platzhalter-Anleitungen ohne funktionsfähigen ausgewählten Provider reichen nicht mehr für einen Service-Start.

## Lokale Tests (keine Live-LLM-Anfragen)

```bash
cargo test --locked --lib resilience_ -- --test-threads=1
POML_CLI=/path/to/Microsoft/POML/cli.js \
  cargo test --locked --lib resilience_ -- --include-ignored --test-threads=1
POML_CLI=/path/to/Microsoft/POML/cli.js \
  cargo test --locked --lib message_tool_loop_tests -- --include-ignored --test-threads=1
node scripts/test_llm_stream.js
node scripts/test_ui_static.js
python3 scripts/check_context_docs.py
```

Die nicht ignorierten Tests verwenden pausierte Tokio-Zeit, Mock-Provider, echte lokale HTTP-Endpunkte und temporäre Datenbanken/Dateien. Die optionalen Gateway-/WebSocket-Integrationstests benötigen nur Node/POML; ihre LLMs sind weiterhin Mocks. Sie prüfen insbesondere gespeicherte Tool-Ergebnisse nach Retry/Exhaustion, einmalige Terminaleffekte, Fortschrittsmeldungen und Ping/Stop während laufender Requests. Sie belegen weder tatsächliche Cloud-Quota noch die Behebung eines konkreten Live-Netzwerkfehlers.
