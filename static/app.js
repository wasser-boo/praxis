const API_BASE = '';
let authToken = localStorage.getItem('praxis_token');
let chatPollInterval = null;
let chatEventSource = null;
// TTS-only side channels: receive chat_tts events from ALL paired Discord
// contexts so replies generated for Discord are also spoken in the dashboard
// while another session is active. No chat content flows through them.
let chatTtsSideSources = [];
let chatTtsSideUserIds = [];
// Per chat-session unique identifier. Each session has its OWN user_id at
// the storage layer (separate context + separate messages). Sessions are
// created by forking a parent context (see chatNewSession).
let chatUserId = localStorage.getItem('praxis_chat_active_session') || 'default';
// Display name for the human in this chat. Distinct from chatUserId
// (which is the session/storage key). Stored in ctx.username on the backend
// and shared across sessions belonging to the same human.
let chatUsername = localStorage.getItem('praxis_chat_username') || 'User';
let chatSessionId = chatUserId; // legacy alias
let chatAttachments = [];
let vmRefreshInterval = null;
let chatSeenIds = new Set();
let chatPollGen = 0;
let isAgentActive = false;
let chatSessions = [];
let chatBotName = 'Praxis';
let sidebarCollapsed = false;

// ═══ Auth ══════════════════════════════════════════════════════════════════════

async function login(password) {
    const res = await fetch(`${API_BASE}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password })
    });
    if (!res.ok) throw new Error('Invalid password');
    const data = await res.json();
    authToken = data.token;
    localStorage.setItem('praxis_token', authToken);
    return data;
}

function logout() {
    authToken = null;
    localStorage.removeItem('praxis_token');
    showScreen('login-screen');
}

function getAuthHeaders() {
    return { 'Content-Type': 'application/json', 'Authorization': `Bearer ${authToken}` };
}

async function apiFetch(path, options = {}) {
    // FormData/Blob bodies set their own Content-Type (multipart boundary /
    // blob type). A forced JSON Content-Type would clobber that boundary and
    // the server rejects the request ("Invalid `boundary` …"), so drop the
    // default header for those bodies.
    const isFormDataBody = typeof FormData !== 'undefined' && options.body instanceof FormData;
    const isBlobBody = typeof Blob !== 'undefined' && options.body instanceof Blob;
    const defaultHeaders = (isFormDataBody || isBlobBody)
        ? { 'Authorization': `Bearer ${authToken}` }
        : getAuthHeaders();
    const res = await fetch(`${API_BASE}${path}`, {
        ...options,
        headers: { ...defaultHeaders, ...options.headers }
    });
    if (res.status === 401) { logout(); throw new Error('Session expired'); }
    return res;
}

function apiPost(path, body) {
    return apiFetch(path, { method: 'POST', body: JSON.stringify(body) });
}
function apiGet(path) { return apiFetch(path); }

// ═══ Screen & Tab ══════════════════════════════════════════════════════════════

async function validateTokenAndLoad() {
    try {
        const res = await fetch(`${API_BASE}/api/contexts`, {
            headers: { 'Authorization': `Bearer ${authToken}` }
        });
        if (res.ok) {
            showScreen('dashboard-screen');
            loadOverview();
            initChatTab();
        } else { logout(); }
    } catch { logout(); }
}

function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(s => s.classList.remove('active'));
    document.getElementById(screenId).classList.add('active');
}

function showTab(tabId) {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelectorAll('.nav-links li').forEach(l => l.classList.remove('active'));
    document.getElementById(`tab-${tabId}`).classList.add('active');
    document.querySelector(`[data-tab="${tabId}"]`).classList.add('active');

    const content = document.querySelector('.content');
    const sidebar = document.getElementById('sidebar');
    document.body.classList.toggle('chat-tab-active', tabId === 'chat');
    document.body.classList.remove('chat-conversations-open');
    content.classList.toggle('chat-expanded', tabId === 'chat');
    sidebar.classList.toggle('collapsed', tabId === 'chat' || sidebarCollapsed);
    updateNavigationButtons();
    loadTabData(tabId);
}

function toggleSidebar() {
    const sidebar = document.getElementById('sidebar');
    const isChat = document.getElementById('tab-chat').classList.contains('active');
    if (isChat) {
        const open = sidebar.classList.contains('collapsed');
        sidebar.classList.toggle('collapsed', !open);
        document.body.classList.remove('chat-conversations-open');
    } else {
        sidebarCollapsed = !sidebarCollapsed;
        sidebar.classList.toggle('collapsed', sidebarCollapsed);
    }
    updateNavigationButtons();
}

function toggleChatConversations() {
    document.body.classList.toggle('chat-conversations-open');
    updateNavigationButtons();
}

function updateNavigationButtons() {
    const expanded = !document.getElementById('sidebar').classList.contains('collapsed');
    const menu = document.getElementById('sidebar-toggle');
    menu.setAttribute('aria-expanded', String(expanded));
    menu.setAttribute('aria-label', expanded ? 'Close dashboard menu' : 'Open dashboard menu');
    menu.title = expanded ? 'Close dashboard menu' : 'Open dashboard menu';
    const conversations = document.getElementById('chat-sidebar-toggle');
    const chatsExpanded = document.body.classList.contains('chat-conversations-open');
    conversations.setAttribute('aria-expanded', String(chatsExpanded));
    conversations.setAttribute('aria-label', chatsExpanded ? 'Close conversations' : 'Open conversations');
}

async function loadTabData(tab) {
    try {
        switch (tab) {
            case 'overview': await loadOverview(); break;
            case 'chat': await loadChatStatus(); break;
            case 'vm': await loadVM(); break;
            case 'contexts': await loadContexts(); break;
            case 'templates': await loadTemplates(); break;
            case 'tools': await loadTools(); break;
            case 'secrets': await loadSecrets(); break;
            case 'pairings': await loadPairings(); break;
            case 'sm-files': await loadSmFiles(); break;
            case 'cron-jobs': await loadCronJobs(); break;
            case 'messages': await populateUserDropdowns(); break;
            case 'memory': await populateUserDropdowns(); break;
        }
    } catch (err) { console.error(`Failed to load ${tab}:`, err); }
}

// ═══ Overview ═════════════════════════════════════════════════════════════════════

async function loadOverview() {
    try {
        const [statusRes, contextsRes, pairingsRes, pendingRes] = await Promise.all([
            apiGet('/api/status'), apiGet('/api/contexts'), apiGet('/api/pairings'), apiGet('/api/pairings/pending')
        ]);
        const status = await statusRes.json();
        const contexts = await contextsRes.json();
        const pairings = await pairingsRes.json();
        const pending = await pendingRes.json();
        document.getElementById('version').textContent = status.version || '-';
        document.getElementById('context-count').textContent = contexts.contexts?.length || 0;
        document.getElementById('pairing-count').textContent = pairings.pairings?.length || 0;
        document.getElementById('pending-count').textContent = pending.pending_pairings?.length || 0;

        const users = (contexts.contexts || []).map(c => c.user_id).filter(Boolean);
        const starters = document.getElementById('overview-agent-starters');
        starters.innerHTML = users.length
            ? users.map(uid => `<div class="data-item">
                <span class="name">${escapeHtml(uid)}</span>
                <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="quickStartAgent('${escapeHtml(uid)}')">Start Agent</button>
                </div></div>`).join('')
            : '<div class="data-item"><span class="name">No users yet. Pair a bot or create a context first.</span></div>';
    } catch (err) { console.error('Overview error:', err); }
}

function quickStartAgent(userId) {
    chatUserId = userId;
    document.getElementById('chat-user-name').textContent = userId;
    showTab('chat');
    setTimeout(() => chatStartAgent(), 300);
}

// ═══ Chat Tab ════════════════════════════════════════════════════════════════════

// Load the TTS switch state from the user's settings. Called on every
// session load/switch — otherwise chatTtsOn stays false after a page reload
// and chatPlayTts silently drops every incoming chat_tts event (audio sent
// by the server but never played).
async function loadTtsSwitchState() {
    try {
        const res = await apiGet(`/api/contexts/${encodeURIComponent(chatUserId)}`);
        const ctxData = await res.json();
        const on = !!(ctxData && ctxData.settings && ctxData.settings.web_chat_tts);
        chatTtsOn = on;
        const btn = document.getElementById('chat-tts-btn');
        if (btn) {
            btn.setAttribute('aria-pressed', on ? 'true' : 'false');
            btn.classList.toggle('tts-on', on);
            btn.textContent = on ? '🔊' : '🔉';
        }
    } catch (_) { /* switch state stays default */ }
}

async function initChatTab() {
    const input = document.getElementById('chat-input');
    input.addEventListener('input', () => {
        input.style.height = 'auto';
        input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    });
    loadChatSessions();
    // Activate the persisted session (or default) before loading info, so
    // chatUserId is correct for context/avatar lookups.
    const stored = localStorage.getItem('praxis_chat_active_session');
    if (stored && chatSessions.find(s => s.id === stored)) {
        chatUserId = stored;
        chatSessionId = stored;
    } else if (chatSessions.length > 0) {
        chatUserId = chatSessions[0].id;
        chatSessionId = chatSessions[0].id;
    }
    renderChatSessionList();
    await loadChatUserInfo();
    // Load existing chat history immediately on startup so the user sees
    // their previous conversation, not just an empty welcome banner.
    await loadChatHistory();
    await loadChatStatus();
    await loadTtsSwitchState();
    // Always open the SSE stream so we can react to agent_start events even
    // before any user interaction in this session.
    startChatStream();
    startTtsSideChannel();
    // Scroll to bottom when chat tab opens
    const container = document.getElementById('chat-messages');
    if (container) container.scrollTop = container.scrollHeight;
}

const SLASH_COMMANDS = [
    { name: '/clear', desc: 'Clear the chat view (history stays in Messages)', action: clearChatMessages },
    { name: '/deletemessages', desc: 'Delete ALL messages (chat + Messages tab; Discord untouched)', action: deleteAllMessages },
    { name: '/start', desc: 'Start the agent loop', action: chatStartAgent },
    { name: '/stop', desc: 'Stop the agent loop', action: chatStopAgent },
    { name: '/new', desc: 'Start a new chat session', action: chatNewSession },
    { name: '/sessions', desc: 'Open the conversations sidebar', action: () => {
        const sidebar = document.querySelector('.chat-sidebar');
        if (sidebar) sidebar.scrollIntoView({ behavior: 'smooth' });
        toggleChatConversations();
    } },
    { name: '/status', desc: 'Check agent status', action: async () => {
        const active = await checkAgentActive(chatUserId);
        addChatMessage('system', `Agent is ${active ? 'active' : 'inactive'} for ${chatUserId}`);
    } },
    {
        name: '/context',
        desc: 'Get/set context vars (e.g. set foo=bar) — supports dot keys',
        takesArgs: true,
        action: chatContextCommand,
    },
    { name: '/compact', desc: 'Summarize history, keep recent messages', action: chatCompactCommand },
    { name: '/skill', desc: 'List skills / activate: /skill NAME | off', takesArgs: true, action: chatSkillCommand },
    { name: '/thinking', desc: 'Thinking mode: /thinking on|off|auto', takesArgs: true, action: chatThinkingCommand },
    { name: '/delegations', desc: 'Show delegated subtasks (status + result)', action: chatDelegationsCommand },
    { name: '/avatar', desc: 'Change your avatar', action: () => showAvatarModal('user') },
    { name: '/botavatar', desc: 'Change bot avatar', action: () => showAvatarModal('bot') },
    { name: '/help', desc: 'Show available commands', action: showSlashHelp },
];

// /compact: summarize the history server-side, keep recent messages.
async function chatCompactCommand() {
    addChatMessage('system', '⏳ Compacting conversation…');
    try {
        const res = await apiFetch(`/api/messages/${encodeURIComponent(chatUserId)}/compact`, { method: 'POST' });
        const data = await res.json();
        if (data.error) { addChatMessage('feedback', '/compact failed: ' + data.error); return; }
        const summary = String(data.summary || '').slice(0, 400);
        addChatMessage('system', `✅ Compacted. Summary saved.\n${summary}`);
        await loadChatHistory();
    } catch (err) {
        addChatMessage('feedback', '/compact failed: ' + err.message);
    }
}

// /skill: list skills or activate/deactivate one.
async function chatSkillCommand(rawLine) {
    const arg = (typeof rawLine === 'string' ? rawLine.replace(/^\/skill\b/i, '').trim() : '');
    try {
        if (!arg || arg === 'list') {
            const res = await apiGet('/api/skills');
            const data = await res.json();
            const lines = (data.skills || []).map(s => `${s.name}${s.user_only ? ' [user-only]' : ''} — ${s.description || ''}`);
            addChatMessage('system', `Active skill: ${data.active_skill || 'off'}\nAvailable skills:\n${lines.join('\n') || '(none)'}\nUse /skill NAME or /skill off.`);
            return;
        }
        if (arg.toLowerCase() === 'off') {
            await apiFetch(`/api/contexts/${encodeURIComponent(chatUserId)}`, {
                method: 'PUT',
                body: JSON.stringify({ settings: { active_skill: null } })
            });
            addChatMessage('system', 'Active skill disabled.');
            return;
        }
        await apiFetch(`/api/contexts/${encodeURIComponent(chatUserId)}`, {
            method: 'PUT',
            body: JSON.stringify({ settings: { active_skill: arg } })
        });
        addChatMessage('system', `Skill '${arg}' is active for your messages.`);
    } catch (err) {
        addChatMessage('feedback', '/skill failed: ' + err.message);
    }
}

// /thinking on|off|auto: set settings.thinking_mode via context exec.
async function chatThinkingCommand(rawLine) {
    const arg = (typeof rawLine === 'string' ? rawLine.replace(/^\/thinking\b/i, '').trim().toLowerCase() : '');
    if (!['on', 'off', 'auto'].includes(arg)) {
        addChatMessage('system', 'Usage: /thinking on|off|auto');
        return;
    }
    try {
        const res = await apiFetch('/api/context/exec', {
            method: 'POST',
            body: JSON.stringify({ user_id: chatUserId, line: `/context set settings.thinking_mode=${arg}` })
        });
        const data = await res.json();
        if (data.error) { addChatMessage('feedback', '/thinking: ' + data.error); return; }
        addChatMessage('system', `Thinking mode set to ${arg}.`);
    } catch (err) {
        addChatMessage('feedback', '/thinking failed: ' + err.message);
    }
}

// /delegations: show delegated subtasks for this session.
async function chatDelegationsCommand() {
    try {
        const res = await apiGet('/api/delegations/' + encodeURIComponent(chatUserId));
        const data = await res.json();
        const list = data.delegations || [];
        if (list.length === 0) { addChatMessage('system', 'No delegations yet.'); return; }
        const lines = list.map(d => {
            const icon = d.status === 'done' ? '✅' : (d.status === 'failed' ? '❌' : '⏳');
            const task = String(d.task || '').slice(0, 80);
            const result = String(d.result || '').replace(/\n/g, ' ').slice(0, 120);
            return `${icon} ${d.id} [${d.status}] ${task}\n    result: ${result}`;
        });
        addChatMessage('system', '🤝 Delegations:\n' + lines.join('\n'));
    } catch (err) {
        addChatMessage('feedback', '/delegations failed: ' + err.message);
    }
}

function showSlashHelp() {
    const list = SLASH_COMMANDS.map(c => `<b>${escapeHtml(c.name)}</b> - ${escapeHtml(c.desc)}`).join('<br>');
    addChatMessage('system', 'Available commands:<br>' + list +
        '<br><br><i>/context examples:</i><br>' +
        '<code>/context set custom_data.device=main settings.max_llm_turns=20</code><br>' +
        '<code>/context set settings.voice_tts_enabled=true</code><br>' +
        '<code>/context get settings.max_llm_turns</code><br>' +
        '<code>/context show settings</code>');
}

/// Send a `/context …` line to the backend for parsing + applying. The
/// backend uses the same shared parser as the TUI and Discord, so syntax
/// is identical across all three frontends.
async function chatContextCommand(rawLine) {
    const line = (typeof rawLine === 'string' && rawLine.trim()) || '/context';
    try {
        const res = await apiFetch('/api/context/exec', {
            method: 'POST',
            body: JSON.stringify({ user_id: chatUserId, line })
        });
        const data = await res.json();
        if (data.error) {
            addChatMessage('feedback', '/context: ' + data.error);
        } else if (data.response) {
            addChatMessage('system', '<pre style="margin:0;white-space:pre-wrap;font-size:0.8rem">' + escapeHtml(data.response) + '</pre>');
        }
    } catch (err) {
        addChatMessage('feedback', '/context failed: ' + err.message);
    }
}

let slashActiveIndex = -1;

function chatInput(e) {
    const input = e.target;
    const val = input.value;
    const box = document.getElementById('slash-commands');
    if (!box) return;
    const beforeCursor = val.slice(0, input.selectionStart);
    const slashWord = beforeCursor.match(/\/(\w*)$/);
    if (!slashWord) {
        box.style.display = 'none';
        return;
    }
    const query = slashWord[1].toLowerCase();
    const matches = SLASH_COMMANDS.filter(c => c.name.slice(1).toLowerCase().startsWith(query));
    if (matches.length === 0) {
        box.style.display = 'none';
        return;
    }
    slashActiveIndex = -1;
    box.innerHTML = matches.map((c, i) =>
        `<div class="slash-cmd ${i === 0 ? 'active' : ''}" data-cmd="${escapeHtml(c.name)}" onclick="runSlashCommand('${escapeHtml(c.name)}')"
         onmouseenter="slashActiveIndex=${i};renderSlashActive()">
            <span class="slash-name">${escapeHtml(c.name)}</span>
            <span class="slash-desc">${escapeHtml(c.desc)}</span>
        </div>`
    ).join('');
    box.style.display = 'block';
}

function renderSlashActive() {
    const box = document.getElementById('slash-commands');
    if (!box) return;
    box.querySelectorAll('.slash-cmd').forEach((el, i) => {
        el.classList.toggle('active', i === slashActiveIndex);
    });
}

function runSlashCommand(name) {
    const cmd = SLASH_COMMANDS.find(c => c.name === name);
    if (!cmd) return;
    const input = document.getElementById('chat-input');
    const val = input.value;
    const beforeCursor = val.slice(0, input.selectionStart);
    const afterCursor = val.slice(input.selectionStart);
    const newBefore = beforeCursor.replace(/\/\w*$/, '');
    input.value = newBefore + afterCursor;
    input.focus();
    input.setSelectionRange(newBefore.length, newBefore.length);
    document.getElementById('slash-commands').style.display = 'none';
    // Execute the command
    try { cmd.action(); } catch (err) { console.error('Slash command error:', err); }
}

function chatKeyDown(e) {
    const box = document.getElementById('slash-commands');
    const visible = box && box.style.display !== 'none';
    if (visible && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
        e.preventDefault();
        const items = box.querySelectorAll('.slash-cmd');
        if (e.key === 'ArrowDown') slashActiveIndex = Math.min(slashActiveIndex + 1, items.length - 1);
        else slashActiveIndex = Math.max(slashActiveIndex - 1, 0);
        renderSlashActive();
        items[slashActiveIndex]?.scrollIntoView({ block: 'nearest' });
        return;
    }
    if (visible && e.key === 'Enter') {
        e.preventDefault();
        const items = box.querySelectorAll('.slash-cmd');
        const active = items[Math.max(0, slashActiveIndex)];
        if (active) runSlashCommand(active.dataset.cmd);
        return;
    }
    if (visible && e.key === 'Escape') {
        e.preventDefault();
        box.style.display = 'none';
        return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        // If the input is exactly a slash command (with optional trailing
        // whitespace), execute it instead of sending it as a chat message.
        const input = e.target;
        const raw = (input.value || '').trim();
        if (raw.startsWith('/')) {
            const head = raw.split(/\s+/)[0];
            const cmd = SLASH_COMMANDS.find(c => c.name === head);
            if (cmd) {
                input.value = '';
                input.style.height = 'auto';
                if (box) box.style.display = 'none';
                try {
                    // Commands flagged with `takesArgs` receive the entire
                    // raw line (e.g. `/context set foo=bar`) so they can
                    // do their own parsing. All other commands ignore args.
                    if (cmd.takesArgs) {
                        cmd.action(raw);
                    } else {
                        cmd.action();
                    }
                } catch (err) { console.error('Slash command error:', err); }
                return;
            }
        }
        if (visible && slashActiveIndex >= 0) {
            const items = box.querySelectorAll('.slash-cmd');
            const active = items[slashActiveIndex];
            if (active) { runSlashCommand(active.dataset.cmd); return; }
        }
        chatSendMessage();
    }
}

async function loadChatUserInfo() {
    try {
        const res = await apiGet('/api/contexts/' + encodeURIComponent(chatUserId));
        const data = await res.json();
        if (data.context) {
            chatBotName = data.context.settings?.agent_name || 'Praxis';
            const ctxUsername = data.context.username;
            if (ctxUsername) {
                chatUsername = ctxUsername;
                localStorage.setItem('praxis_chat_username', chatUsername);
            }
        } else if (data.username) {
            chatUsername = data.username;
            localStorage.setItem('praxis_chat_username', chatUsername);
        }
    } catch {}
    document.getElementById('chat-user-name-label').textContent = chatUsername || 'User';
    const botLabel = document.getElementById('chat-bot-name');
    if (botLabel) botLabel.textContent = chatBotName;
    loadAvatar();
}

async function loadChatStatus() {
    const active = await checkAgentActive(chatUserId);
    updateAgentUI(active);
    await loadCLStatus();
    loadAvatar();
}

async function checkAgentActive(userId) {
    try {
        const res = await apiGet(`/api/agent/status/${encodeURIComponent(userId)}`);
        const data = await res.json();
        return data.active;
    } catch { return false; }
}

function updateAgentUI(active) {
    isAgentActive = active;
    const badge = document.getElementById('chat-agent-badge');
    const startBtn = document.getElementById('chat-start-btn');
    const stopBtn = document.getElementById('chat-stop-btn');
    if (active) {
        badge.textContent = 'Agent: Active';
        badge.className = 'agent-badge active';
        startBtn.style.display = 'none';
        stopBtn.style.display = '';
        startChatPolling();
    } else {
        badge.textContent = 'Agent: Inactive';
        badge.className = 'agent-badge';
        startBtn.style.display = '';
        stopBtn.style.display = 'none';
        stopChatPolling();
    }
}

function loadAvatar() {
    const img = document.getElementById('chat-user-avatar');
    // Avatar is keyed by the human's username so it persists across sessions
    // belonging to the same user. Fall back to user_id if no username is set.
    const key = (chatUsername && chatUsername !== 'User') ? chatUsername : chatUserId;
    img.src = `/api/avatar/${encodeURIComponent(key)}?t=${Date.now()}`;
    img.style.display = '';
    img.onerror = () => { img.src = '/logo.png'; };
}

async function chatStartAgent() {
    const uid = chatUserId;
    try {
        const beginRes = await apiFetch('/api/agent/begin', {
            method: 'POST',
            body: JSON.stringify({ user_id: uid, message: 'Hello' })
        });
        await beginRes.json();
        addChatMessage('system', `Agent started for ${uid}`);
        updateAgentUI(true);
        await loadCLStatus();
        // Load the TTS switch state from the user's settings.
        try {
            const ctxRes = await apiFetch(`/api/contexts/${encodeURIComponent(uid)}`);
            const ctxData = await ctxRes.json();
            const on = !!(ctxData && ctxData.settings && ctxData.settings.web_chat_tts);
            chatTtsOn = on;
            const btn = document.getElementById('chat-tts-btn');
            if (btn) {
                btn.setAttribute('aria-pressed', on ? 'true' : 'false');
                btn.classList.toggle('tts-on', on);
                btn.textContent = on ? '🔊' : '🔉';
            }
        } catch (_) { /* switch state stays default */ }
    } catch (err) { addChatMessage('feedback', 'Failed to start: ' + err.message); }
}

async function chatStopAgent() {
    try {
        await apiFetch(`/api/agent/stop/${encodeURIComponent(chatUserId)}`, { method: 'POST' });
        addChatMessage('system', 'Agent stopped');
        updateAgentUI(false);
    } catch (err) { addChatMessage('feedback', 'Failed to stop: ' + err.message); }
}

async function chatSendMessage() {
    const input = document.getElementById('chat-input');
    const msg = input.value.trim();
    if (!msg && chatAttachments.length === 0) return;
    input.value = '';
    input.style.height = 'auto';

    const attList = [...chatAttachments];
    chatAttachments = [];
    renderAttachments();

    const displayMsg = msg || '(attachments)';
    startChatTimer(60);

    // Show user message immediately (optimistic)
    addChatMessage('user', displayMsg);
    chatSeenIds.add('user:' + displayMsg);
    autoScrollChat();

    try {
        const res = await apiFetch('/api/chat/send', {
            method: 'POST',
            body: JSON.stringify({
                user_id: chatUserId,
                message: msg,
                attachments: attList
            })
        });
        const data = await res.json();
        if (data.error) {
            addChatMessage('feedback', data.error);
            stopChatTimer();
        } else if (data.type === 'agent_started') {
            addChatMessage('system', data.message);
            updateAgentUI(true);
        }
    } catch (err) {
        addChatMessage('feedback', 'Send failed: ' + err.message);
        stopChatTimer();
    }
}

function autoScrollChat() {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    container.scrollTop = container.scrollHeight;
}

const questionSelections = new Map(); // questionId -> Set of selected indices

function addChatQuestionCard(questionId, text, suggestions) {
    const container = document.getElementById('chat-messages');
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();

    const card = document.createElement('div');
    card.className = 'chat-msg question-card';
    card.id = 'qcard-' + questionId;
    card.dataset.questionId = questionId;

    questionSelections.set(questionId, new Set());

    const optsId = 'qopts-' + questionId;
    const ansId = 'qans-' + questionId;

    card.innerHTML = `<div class="msg-row">
        <div class="msg-col">
            <img class="msg-avatar" src="/logo.png" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.png'">
            <span class="msg-label">Question</span>
        </div>
        <div class="msg-content">
            <div class="question-text">${renderMarkdown(text)}</div>
            <div class="question-options" id="${optsId}"></div>
            <div class="question-submit" id="qsub-${questionId}" style="display:none;margin-top:0.5rem">
                <button class="chat-option submit-btn" onclick="submitChatOptions('${questionId}')">Submit</button>
            </div>
            <div class="question-answer" id="${ansId}" style="display:none"></div>
        </div>
    </div>`;

    const optsDiv = card.querySelector('.question-options');
    suggestions.forEach((s, i) => {
        const emoji = ['1️⃣','2️⃣','3️⃣','4️⃣','5️⃣','6️⃣','7️⃣','8️⃣','9️⃣','🔟'][i] || '◾';
        const btn = document.createElement('button');
        btn.className = 'chat-option';
        btn.dataset.idx = i;
        btn.innerHTML = `<span class="emoji">${emoji}</span> ${escapeHtml(s)}`;
        btn.onclick = () => toggleChatOption(questionId, i, btn);
        optsDiv.appendChild(btn);
    });

    container.appendChild(card);
    autoScrollChat();
}

function toggleChatOption(questionId, idx, btn) {
    const selected = questionSelections.get(questionId);
    if (!selected) return;
    if (selected.has(idx)) {
        selected.delete(idx);
        btn.classList.remove('selected');
    } else {
        selected.add(idx);
        btn.classList.add('selected');
    }
    // Show/hide submit button
    const sub = document.getElementById('qsub-' + questionId);
    if (sub) sub.style.display = selected.size > 0 ? '' : 'none';
}

async function submitChatOptions(questionId) {
    const selected = questionSelections.get(questionId);
    if (!selected || selected.size === 0) return;

    const card = document.getElementById('qcard-' + questionId);
    const optsDiv = document.getElementById('qopts-' + questionId);
    const subDiv = document.getElementById('qsub-' + questionId);
    const ansDiv = document.getElementById('qans-' + questionId);

    // Disable all option buttons
    if (optsDiv) {
        optsDiv.querySelectorAll('.chat-option').forEach(btn => {
            btn.disabled = true;
            btn.classList.remove('selected');
            if (!btn.classList.contains('submit-btn')) {
                btn.style.opacity = '0.5';
                btn.style.cursor = 'not-allowed';
            }
        });
    }
    if (subDiv) subDiv.style.display = 'none';

    // Collect selected texts for display
    const labels = [];
    const indices = Array.from(selected).sort((a, b) => a - b);
    indices.forEach(idx => {
        const btn = optsDiv?.querySelector(`[data-idx="${idx}"]`);
        const text = btn?.textContent?.trim() || ('Option ' + (idx + 1));
        labels.push(text);
    });

    if (ansDiv) {
        ansDiv.style.display = '';
        ansDiv.textContent = 'Selected: ' + labels.join(', ');
    }

    // Send all selected options to backend
    for (const idx of indices) {
        try {
            await apiFetch('/api/chat/send', {
                method: 'POST',
                body: JSON.stringify({
                    user_id: chatUserId,
                    message: '',
                    is_option: true,
                    question_id: questionId,
                    option_index: idx
                })
            });
        } catch (err) { console.error('Option send failed:', err); }
    }
}

function showChatQuestion(questionId, text, suggestions) {
    addChatQuestionCard(questionId, text, suggestions);
}
function startChatPolling() {
    stopChatPolling();
    chatPollInterval = setInterval(pollChatMessages, 3000);
    startChatStream();
}

function stopChatPolling() {
    if (chatPollInterval) { clearInterval(chatPollInterval); chatPollInterval = null; }
    stopChatStream();
}

function startChatStream() {
    stopChatStream();
    if (!chatUserId || !authToken) return;
    const url = `/api/chat/stream/${encodeURIComponent(chatUserId)}?token=${encodeURIComponent(authToken)}`;
    console.log('[SSE] connecting to', url);
    const es = new EventSource(url);
    let streamBuffer = '';
    let streamMsg = null;

    es.onopen = () => {
        console.log('[SSE] connection opened for user', chatUserId);
    };

    es.addEventListener('typing', (e) => {
        console.log('[SSE] typing event:', e.data);
    });

    es.addEventListener('agent_start', (e) => {
        console.log('[SSE] agent_start event');
        addAgentLoopBanner('start');
        updateAgentUI(true);
        document.body.classList.add('agent-loop-running');
    });

    es.addEventListener('agent_stop', (e) => {
        console.log('[SSE] agent_stop event');
        addAgentLoopBanner('stop');
        updateAgentUI(false);
        document.body.classList.remove('agent-loop-running');
        stopChatTimer();
    });

    es.addEventListener('chat_tts', (e) => {
        console.log('[SSE] chat_tts event received');
        try {
            const d = JSON.parse(e.data);
            if (d.audio) chatPlayTts(d.audio);
        } catch (err) { console.error('[SSE chat_tts error]', err); }
    });

    es.addEventListener('feedback', (e) => {
        console.log('[SSE] feedback event received');
        try {
            const d = JSON.parse(e.data);
            if (d.data) addChatMessage('feedback', d.data);
        } catch (err) { console.error('[SSE feedback error]', err); }
    });
    es.addEventListener('stream_abort', () => {
        // A failed/cancelled generation is never a completed reply. Remove its
        // provisional bubble so a subsequent call cannot append duplicate text.
        if (streamMsg) streamMsg.remove();
        streamMsg = null;
        streamBuffer = '';
        stopChatTimer();
    });
    es.addEventListener('char', (e) => {
        const t0 = performance.now();
        try {
            const d = JSON.parse(e.data);
            if (d.data) {
                if (!streamMsg) {
                    console.log('[SSE] first char received, creating stream message element');
                    streamMsg = document.createElement('div');
                    // Tag streamed messages produced inside an agent loop so
                    // they are visually distinct from plain text replies.
                    const loopClass = document.body.classList.contains('agent-loop-running')
                        ? ' agent-loop-msg' : '';
                    streamMsg.className = 'chat-msg assistant' + loopClass;
                    const botAvatar = `/api/avatar/bot?t=${Date.now()}`;
                    const botName = chatBotName || 'Praxis';
                    streamMsg.innerHTML = `<div class="msg-row">
                        <div class="msg-col">
                            <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.png'" onclick="showAvatarModal('bot')">
                            <span class="msg-label">${escapeHtml(botName)}</span>
                        </div>
                        <div class="msg-content markdown stream-live"></div>
                    </div>`;
                    const container = document.getElementById('chat-messages');
                    const welcome = container.querySelector('.chat-welcome');
                    if (welcome) welcome.remove();
                    container.appendChild(streamMsg);
                    autoScrollChat();
                }
                streamBuffer += d.data;
                const contentEl = streamMsg.querySelector('.msg-content');
                if (contentEl) {
                    contentEl.innerHTML = renderMarkdown(streamBuffer);
                }
                autoScrollChat();
            }
            const elapsed = performance.now() - t0;
            if (elapsed > 5) console.warn('[SSE] slow char render:', elapsed.toFixed(1), 'ms');
        } catch (err) { console.error('[SSE char error]', err, e.data); }
    });
    es.addEventListener('assistant', (e) => {
        console.log('[SSE] assistant (final) event received');
        try {
            const d = JSON.parse(e.data);
            if (d.data) {
                chatSeenIds.add('assistant:' + d.data.slice(0, 80));
                if (streamMsg) {
                    console.log('[SSE] finalizing stream message, buffer length:', streamBuffer.length);
                    const contentEl = streamMsg.querySelector('.msg-content');
                    if (contentEl) {
                        contentEl.classList.remove('stream-live');
                        contentEl.innerHTML = renderMarkdown(d.data);
                    }
                    streamMsg = null;
                    streamBuffer = '';
                } else {
                    console.log('[SSE] no stream message — adding as regular assistant message');
                    addChatMessage('assistant', d.data);
                }
                stopChatTimer();
                autoScrollChat();
            }
        } catch (err) { console.error('[SSE assistant error]', err); }
    });
    es.addEventListener('question', (e) => {
        console.log('[SSE] question event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            showChatQuestion(inner.question_id, inner.text, inner.suggestions);
        } catch (err) { console.error('[SSE question error]', err); }
    });
    es.addEventListener('image', (e) => {
        console.log('[SSE] image event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            addChatImage(inner.path, inner.caption);
        } catch (err) { console.error('[SSE image error]', err); }
    });
    es.addEventListener('chat_image', (e) => {
        console.log('[SSE] chat_image event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            // data URLs render inline; server paths need the /api prefix check
            addChatImage(inner.url || inner.path, inner.caption);
        } catch (err) { console.error('[SSE chat_image error]', err); }
    });
    es.addEventListener('tool_call', (e) => {
        console.log('[SSE] tool_call event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            const tool = inner.tool || '?';
            let detail = '';
            const ap = inner.args_preview || {};
            if (ap.path) detail = escapeHtml(ap.path);
            else if (ap.query) detail = escapeHtml(String(ap.query));
            else if (ap.command) detail = escapeHtml(String(ap.command));
            else if (ap.args) detail = escapeHtml(String(ap.args));
            addChatMessage('tool', `🔧 <b>${escapeHtml(tool)}</b>`, detail);
        } catch (err) { console.error('[SSE tool_call error]', err); }
    });
    es.addEventListener('tool_result', (e) => {
        console.log('[SSE] tool_result event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            const tool = inner.tool || '?';
            const dur = inner.duration_ms != null ? ` (${inner.duration_ms} ms)` : '';
            const result = inner.result || '';
            addChatMessage('tool', `✅ ${escapeHtml(tool)}${escapeHtml(dur)}`, String(result));
        } catch (err) { console.error('[SSE tool_result error]', err); }
    });
    es.addEventListener('discord_message', (e) => {
        console.log('[SSE] discord_message event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            addDiscordMirrorMessage(inner);
        } catch (err) { console.error('[SSE discord_message error]', err); }
    });
    es.addEventListener('delegation_update', (e) => {
        console.log('[SSE] delegation_update event received');
        try {
            const d = JSON.parse(e.data);
            const inner = JSON.parse(d.data);
            const icon = inner.status === 'done' ? '✅' : '❌';
            const result = String(inner.result || '').slice(0, 400);
            addChatMessage('system', `${icon} Delegation ${inner.delegation_id} ${inner.status === 'done' ? 'finished' : 'failed'}\n${result}`);
        } catch (err) { console.error('[SSE delegation_update error]', err); }
    });
    es.onerror = (err) => {
        console.error('[SSE] connection error (EventSource readyState=' + es.readyState + '), reconnecting in 3s');
        stopChatStream();
        setTimeout(startChatStream, 3000);
    };
    chatEventSource = es;
    console.log('[SSE] EventSource created, readyState:', es.readyState);
}

function stopChatStream() {
    if (chatEventSource) {
        chatEventSource.close();
        chatEventSource = null;
    }
    for (const es of chatTtsSideSources) es.close();
    chatTtsSideSources = [];
    chatTtsSideUserIds = [];
    // A broken/reconnecting SSE can never deliver chat_tts audio; hide the
    // speaking chip so it cannot get stuck in "speaking" state.
    chatTtsActive(false);
}

/// Open the TTS-only side channel for the paired Discord context (if any).
/// The main chat stream stays untouched; this connection only feeds
/// chatPlayTts so Discord-generated replies are spoken in the dashboard too.
async function startTtsSideChannel() {
    // Close previous side channels when switching sessions.
    for (const es of chatTtsSideSources) es.close();
    chatTtsSideSources = [];
    chatTtsSideUserIds = [];
    if (!authToken) return;
    let pairings = [];
    try {
        const res = await apiGet('/api/pairings');
        if (!res || !res.ok) return;
        pairings = (await res.json()).pairings || [];
    } catch (_) { return; }
    // Subscribe to ALL pairings except the active session (the active one
    // already receives chat_tts via the main stream). The side channel is
    // TTS-only, so this cannot duplicate chat content — it just makes sure
    // replies generated for ANY Discord pairing are spoken in the dashboard.
    const targets = pairings.map(p => p.user_id).filter(id => id !== chatUserId);
    if (targets.length === 0) return;
    for (const pairedUserId of targets) {
        chatTtsSideUserIds.push(pairedUserId);
        const url = `/api/chat/stream/${encodeURIComponent(pairedUserId)}/tts?token=${encodeURIComponent(authToken)}`;
        console.log('[SSE-TTS] opening side channel for', pairedUserId);
        const es = new EventSource(url);
        es.addEventListener('chat_tts', (e) => {
            try {
                const d = JSON.parse(e.data);
                if (d.audio) {
                    console.log('[SSE-TTS] chat_tts from side channel', pairedUserId);
                    chatPlayTts(d.audio);
                }
            } catch (err) { console.error('[SSE-TTS chat_tts error]', err); }
        });
        es.onerror = () => {
            console.warn('[SSE-TTS] side channel error for', pairedUserId, '; retrying in 5s');
            es.close();
            const idx = chatTtsSideSources.indexOf(es);
            if (idx >= 0) chatTtsSideSources.splice(idx, 1);
            setTimeout(() => {
                if (chatTtsSideUserIds.includes(pairedUserId)) startTtsSideChannel();
            }, 5000);
        };
        chatTtsSideSources.push(es);
    }
}

async function pollChatMessages() {
    const myGen = chatPollGen;
    try {
        const res = await apiGet(`/api/messages/${encodeURIComponent(chatUserId)}`);
        if (myGen !== chatPollGen) return; // stale response for a different session
        const data = await res.json();
        if (data.messages && data.messages.length > 0) {
            for (const m of data.messages) {
                const id = m.role + ':' + (m.content || '').slice(0, 80);
                if (!chatSeenIds.has(id)) {
                    chatSeenIds.add(id);
                    renderChatMessage(m);
                }
            }
        }
        await loadCLStatus();
    } catch {}
}

async function loadChatHistory() {
    // Only clear the DOM — do NOT delete messages on the backend! The previous
    // implementation called clearChatMessages() here, which has the side effect
    // of issuing DELETE /api/messages/:user_id, wiping the user's history every
    // time the chat tab opened or a session was switched.
    const container = document.getElementById('chat-messages');
    if (container) {
        container.innerHTML = '<div class="chat-welcome">Loading messages...</div>';
    }
    chatSeenIds.clear();

    const mySessionId = chatSessionId;
    console.log('[HISTORY] loading history for session', chatSessionId, 'user_id', chatUserId);
    try {
        // chat_only=1: hide rows up to the /clear marker. The Messages tab
        // keeps the full history; a chat clear never deletes anything.
        const res = await apiGet(`/api/messages/${encodeURIComponent(chatUserId)}?chat_only=1`);
        if (chatSessionId !== mySessionId) return; // switched away while loading
        const data = await res.json();
        const msgs = data.messages || [];
        console.log('[HISTORY] got', msgs.length, 'messages');
        const welcome = container && container.querySelector('.chat-welcome');
        if (welcome) welcome.remove();
        if (msgs.length === 0) {
            if (container) {
                container.innerHTML = '<div class="chat-welcome">No messages yet. Start typing or run /start to begin.</div>';
            }
            return;
        }
        for (const m of msgs) {
            const id = m.role + ':' + (m.content || '').slice(0, 80);
            chatSeenIds.add(id);
            renderChatMessage(m);
        }
        if (container) container.scrollTop = container.scrollHeight;
    } catch (err) { console.error('[HISTORY] load error:', err); }
}

function renderChatMessage(m) {
    // Discord-mirror rows from history: render with the same badge as live
    // discord_message SSE events so the chat looks identical after a reload.
    if ((m.role === 'discord_user' || m.role === 'discord_bot') && m.discord_meta) {
        addDiscordMirrorMessage({ ...m.discord_meta, content: m.content });
        return;
    }
    // Skip intermediate assistant messages that have tool_calls — only show final responses
    if (m.role === 'assistant' && m.tool_calls && m.tool_calls.length > 0) {
        return; // intermediate step, not shown
    }
    if (m.role === 'assistant' && m.content) {
        // Check for web question
        if (m.content.includes('__WEB_QUESTION__')) {
            const parts = m.content.split('__WEB_QUESTION__');
            if (parts.length >= 2) {
                const inner = parts[1];
                const qParts = inner.split('__');
                const qid = qParts[0];
                const text = qParts.slice(1).join('__');
                const lines = text.split('\n');
                const suggLines = lines.filter(l => l.match(/^\d+\s/));
                const suggestions = suggLines.map(l => l.replace(/^\d+\s/, ''));
                const questionText = lines.slice(0, lines.indexOf(suggLines[0] || '')).join('\n');
                if (suggestions.length > 0) addChatQuestionCard(qid, questionText, suggestions);
                return;
            }
        }
        addChatMessage('assistant', m.content);
        stopChatTimer();
    }
    if (m.role === 'user' && m.content) {
        addChatMessage('user', m.content);
    }
    if (m.role === 'tool' && m.content) {
        const toolName = m.tool_name || 'tool';
        addChatMessage('tool', `<span class="tool-name">${escapeHtml(toolName)}</span>`, m.content);
    }
    if (m.role === 'system' && m.content) {
        addChatMessage('system', m.content);
    }
}

// ═══ Chat Timer ════════════════════════════════════════════════════════════════

let chatTimerId = null;
let chatTimerTotal = 0;

function startChatTimer(seconds) {
    stopChatTimer();
    chatTimerTotal = seconds;
    const bar = document.getElementById('chat-timer-bar');
    bar.style.display = '';
    document.getElementById('chat-timer-text').textContent = seconds + 's';
    document.getElementById('chat-timer-fill').style.width = '100%';
    const start = Date.now();
    chatTimerId = setInterval(() => {
        const elapsed = (Date.now() - start) / 1000;
        const remaining = Math.max(0, chatTimerTotal - elapsed);
        const pct = (remaining / chatTimerTotal) * 100;
        document.getElementById('chat-timer-fill').style.width = pct + '%';
        document.getElementById('chat-timer-text').textContent = Math.ceil(remaining) + 's';
        if (remaining <= 0) stopChatTimer();
    }, 200);
}

function stopChatTimer() {
    if (chatTimerId) { clearInterval(chatTimerId); chatTimerId = null; }
    document.getElementById('chat-timer-bar').style.display = 'none';
}

// ═══ Chat Messages ══════════════════════════════════════════════════════════════

function addChatMessage(type, content, extra = null) {
    const container = document.getElementById('chat-messages');
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();

    const div = document.createElement('div');
    div.className = `chat-msg ${type}`;

    const botAvatar = `/api/avatar/bot?t=${Date.now()}`;
    const avatarKey = (chatUsername && chatUsername !== 'User') ? chatUsername : chatUserId;
    const userAvatar = `/api/avatar/${encodeURIComponent(avatarKey)}?t=${Date.now()}`;
    const botName = chatBotName || 'Praxis';
    const userName = chatUsername || 'User';

    if (type === 'user') {
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${userAvatar}" alt="" onerror="this.src='/logo.png'" onclick="showAvatarModal('user')">
                <span class="msg-label">${escapeHtml(userName)}</span>
            </div>
            <div class="msg-content">${escapeHtml(content)}</div>
        </div>`;
    } else if (type === 'assistant') {
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.png'" onclick="showAvatarModal('bot')">
                <span class="msg-label">${escapeHtml(botName)}</span>
            </div>
            <div class="msg-content markdown">${renderMarkdown(content)}</div>
        </div>`;
    } else if (type === 'tool') {
        const args = extra || '';
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-cyan)" onerror="this.src='/logo.png'" onclick="showAvatarModal('bot')">
                <span class="msg-label">Tool</span>
            </div>
            <div class="msg-content">${content}${args ? `<div class="tool-out">${escapeHtml(args)}</div>` : ''}</div>
        </div>`;
    } else {
        div.textContent = content;
    }

    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}

/// Render a visual banner showing that the agent loop has started or stopped.
/// This is intentionally distinct from regular chat text so the user can
/// clearly see when the agent is doing work vs. plain conversation.
function addChatImage(path, caption) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();
    const div = document.createElement('div');
    div.className = 'chat-msg image';
    const botAvatar = `/api/avatar/bot?t=${Date.now()}`;
    const botName = chatBotName || 'Praxis';
    div.innerHTML = `<div class="msg-row">
        <div class="msg-col">
            <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.png'" onclick="showAvatarModal('bot')">
            <span class="msg-label">${escapeHtml(botName)}</span>
        </div>
        <div class="msg-content">
            ${caption ? `<div style="margin-bottom:0.5rem;font-size:0.85rem;color:var(--text-secondary)">${escapeHtml(caption)}</div>` : ''}
            <img src="${escapeHtml(path)}" style="max-width:100%;border-radius:8px;border:1px solid var(--border);cursor:pointer;" onclick="window.open('${escapeHtml(path)}','_blank')" alt="Screenshot">
        </div>
    </div>`;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}

// Mirror of Discord traffic (user + bot) inside the dashboard chat. The
// Discord badge makes the origin obvious next to dashboard-native messages.
function addDiscordMirrorMessage(inner) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();

    const isBot = inner.direction === 'bot';
    const kind = inner.channel_kind === 'dm' ? 'DM' : '#';
    const channelLabel = kind === 'DM' ? 'DM' : ('#' + (inner.channel_id || '?'));
    const author = isBot ? (chatBotName || 'Praxis') : (inner.author || 'Discord-User');
    const time = new Date().toLocaleTimeString();

    const div = document.createElement('div');
    div.className = `chat-msg discord ${isBot ? 'discord-bot' : 'discord-user'}`;
    div.innerHTML = `<div class="msg-row">
        <div class="msg-col">
            <span class="discord-badge" title="Nachricht aus Discord">🎮</span>
            <span class="msg-label">${escapeHtml(author)}</span>
        </div>
        <div class="msg-content">
            <div class="discord-meta"><span class="discord-channel">${escapeHtml(channelLabel)}</span><span class="discord-time">${escapeHtml(time)}</span></div>
            <div class="discord-text">${renderMarkdown(inner.content || '')}</div>
        </div>
    </div>`;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}

function addAgentLoopBanner(kind) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();
    const div = document.createElement('div');
    div.className = `chat-msg agent-loop-banner ${kind}`;
    const icon = kind === 'start' ? '▶' : '■';
    const label = kind === 'start' ? 'Agent loop started' : 'Agent loop stopped';
    const time = new Date().toLocaleTimeString();
    div.innerHTML = `<span class="alb-icon">${icon}</span><span class="alb-label">${label}</span><span class="alb-time">${escapeHtml(time)}</span>`;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}

async function clearChatMessages() {
    // Chat-only clear: hide everything up to now from the CHAT view. Rows are
    // kept (Messages tab still shows them); new messages appear again.
    try {
        const res = await apiFetch(`/api/messages/${encodeURIComponent(chatUserId)}/clear-chat`, { method: 'POST' });
        if (!res.ok) throw new Error('Clear failed');
    } catch (err) {
        console.error('[CLEAR] chat clear failed:', err);
        addChatMessage('feedback', 'Failed to clear the chat view');
        return;
    }
    const container = document.getElementById('chat-messages');
    container.innerHTML = '<div class="chat-welcome">Chat cleared. Your history stays in the Messages tab — new messages appear here again.</div>';
    chatSeenIds.clear();
}

// /deletemessages: hard-delete ALL messages of the CURRENT user/session from
// the database (chat view + Messages tab + LLM history). Discord itself is
// never touched — only the local copies/mirrors for this user_id. The
// context (settings, memory, pairing) is kept.
async function deleteAllMessages() {
    if (!confirm(`Delete ALL messages for this session (${chatUserId})?\n\nThis removes the local chat history, the Messages-tab history and the LLM conversation history. Discord itself is NOT affected. This cannot be undone.`)) {
        return;
    }
    try {
        const res = await apiFetch(`/api/messages/${encodeURIComponent(chatUserId)}`, { method: 'DELETE' });
        if (!res.ok) throw new Error('Delete failed');
    } catch (err) {
        console.error('[DELETE] message delete failed:', err);
        addChatMessage('feedback', 'Failed to delete messages');
        return;
    }
    // Reset the clear marker too, so the view starts fresh at zero.
    try {
        await apiFetch(`/api/contexts/${encodeURIComponent(chatUserId)}`, {
            method: 'PUT',
            body: JSON.stringify({ custom_data: { chat_cleared_message_id: 0 } })
        });
    } catch (_) { /* marker reset is best-effort */ }
    const container = document.getElementById('chat-messages');
    container.innerHTML = '<div class="chat-welcome">All messages for this session deleted. Discord was not affected. New messages appear here.</div>';
    chatSeenIds.clear();
}

// ═══ Chat Sessions ══════════════════════════════════════════════════════════════
//
// Each chat session has its OWN user_id at the storage layer (fully separate
// context + messages). When a new session is created we POST to
// /api/contexts/:user_id/fork with a new randomly-generated id; the backend
// clones the parent context (settings, custom_data, etc.) under the new id.
// Sessions belonging to the same human share `ctx.username` for display.

function loadChatSessions() {
    const stored = localStorage.getItem('praxis_chat_sessions');
    if (stored) {
        try { chatSessions = JSON.parse(stored); } catch { chatSessions = []; }
    }
    if (chatSessions.length === 0) {
        chatSessions = [{ id: 'default', name: 'Default', username: chatUsername }];
        saveChatSessions();
    }
    renderChatSessionList();
    // Discord pairings appear as chat sessions too (their full history lives
    // under the paired user_id). Merged in the background; the list re-renders
    // when they arrive.
    mergeDiscordPairingSessions();
}

/// Fetch pairings and add one chat-session entry per paired context (marked
/// with a Discord badge). Idempotent: existing entries are updated, local
/// sessions are never removed.
async function mergeDiscordPairingSessions() {
    try {
        const res = await apiGet('/api/pairings');
        if (!res || !res.ok) return;
        const data = await res.json();
        const pairings = data.pairings || [];
        if (pairings.length === 0) return;
        let changed = false;
        for (const p of pairings) {
            const existing = chatSessions.find(s => s.id === p.user_id);
            if (existing) {
                if (existing.discord !== p.discord_user_id) {
                    existing.discord = p.discord_user_id;
                    changed = true;
                }
            } else {
                chatSessions.push({
                    id: p.user_id,
                    name: `🎮 Discord (${p.discord_user_id})`,
                    username: chatUsername,
                    discord: p.discord_user_id,
                });
                changed = true;
            }
        }
        if (changed) {
            saveChatSessions();
            renderChatSessionList();
        }
    } catch (err) { console.error('[SESSION] pairing merge failed:', err); }
}

function saveChatSessions() {
    localStorage.setItem('praxis_chat_sessions', JSON.stringify(chatSessions));
}

function renderChatSessionList() {
    const list = document.getElementById('chat-session-list');
    if (!list) return;
    list.innerHTML = chatSessions.map(s => {
        const label = s.name || s.username || s.id;
        return `
        <div class="chat-session ${s.id === chatUserId ? 'active' : ''}" onclick="switchChatSession('${escapeHtml(s.id)}')">
            <span class="session-name" ondblclick="event.stopPropagation();startRenameSession('${escapeHtml(s.id)}', this)">${escapeHtml(label)}</span>
            ${chatSessions.length > 1 ? `<span class="session-del" onclick="event.stopPropagation();deleteChatSession('${escapeHtml(s.id)}')">×</span>` : ''}
        </div>
    `;
    }).join('');
}

function startRenameSession(id, el) {
    const s = chatSessions.find(s => s.id === id);
    if (!s) return;
    const oldName = s.name;
    const input = document.createElement('input');
    input.type = 'text';
    input.value = oldName;
    input.className = 'session-rename-input';
    input.style.cssText = 'background:var(--bg-primary);border:1px solid var(--accent-purple);color:var(--text-primary);border-radius:4px;padding:0.2rem 0.4rem;font-size:0.85rem;width:100%;';
    el.replaceWith(input);
    input.focus();
    input.select();
    const save = () => {
        const newName = input.value.trim();
        if (newName && newName !== oldName) {
            s.name = newName;
            saveChatSessions();
        }
        renderChatSessionList();
    };
    input.addEventListener('blur', save);
    input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); save(); }
        if (e.key === 'Escape') { input.value = oldName; renderChatSessionList(); }
    });
}

async function chatNewSession() {
    const newId = 'sess-' + Math.random().toString(36).slice(2, 10) + Date.now().toString(36).slice(-4);
    const name = `Session ${chatSessions.length + 1}`;
    console.log('[SESSION] forking new session:', newId, 'from parent', chatUserId);

    // Fork the parent context on the backend. The new session inherits
    // settings/custom_data but starts with no messages and turn=0.
    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(chatUserId)}/fork`, {
            method: 'POST',
            body: JSON.stringify({ new_user_id: newId, username: chatUsername })
        });
        if (!res.ok) {
            const text = await res.text().catch(() => '');
            console.error('[SESSION] fork failed:', res.status, text);
            addChatMessage('feedback', 'Failed to create session: ' + res.status);
            return;
        }
    } catch (err) {
        console.error('[SESSION] fork failed:', err);
        addChatMessage('feedback', 'Failed to create session: ' + err.message);
        return;
    }

    chatSessions.push({ id: newId, name, username: chatUsername });
    saveChatSessions();
    await switchChatSession(newId);
}

async function switchChatSession(id) {
    document.body.classList.remove('chat-conversations-open');
    console.log('[SESSION] switching to session:', id, '(from', chatUserId, ')');
    stopChatPolling();
    chatPollGen++;
    chatUserId = id;
    chatSessionId = id;
    localStorage.setItem('praxis_chat_active_session', id);
    // Reset the seen-id set; it tracks dedup keys that are session-local.
    chatSeenIds = new Set();
    // Clear the chat view (DOM only — does NOT delete messages on the server).
    const container = document.getElementById('chat-messages');
    if (container) {
        container.innerHTML = '<div class="chat-welcome">Loading messages...</div>';
    }
    renderChatSessionList();
    await loadChatUserInfo();
    await loadChatHistory();
    await loadChatStatus();
    await loadTtsSwitchState();
    // Always open the SSE stream for the active session so agent_start /
    // streaming-token events arrive even before user interaction.
    startChatStream();
    startTtsSideChannel();
    // If an agent loop happens to already be running for this session,
    // startChatPolling will be triggered via updateAgentUI(true).
}

function deleteChatSession(id) {
    if (chatSessions.length <= 1) return;
    const s = chatSessions.find(x => x.id === id);
    if (s && s.discord) {
        // Discord-paired contexts are managed by the pairing; deleting the
        // context here would break the Discord bot coupling.
        alert('This conversation is linked to your Discord pairing and cannot be deleted here. Remove the pairing in the Pairings tab instead.');
        return;
    }
    if (!confirm('Delete this chat session and all its messages? This cannot be undone.')) return;
    // Best-effort backend cleanup of the session context + messages.
    apiFetch(`/api/contexts/${encodeURIComponent(id)}`, { method: 'DELETE' }).catch(() => {});
    chatSessions = chatSessions.filter(s => s.id !== id);
    saveChatSessions();
    if (chatUserId === id) {
        switchChatSession(chatSessions[0].id);
    } else {
        renderChatSessionList();
    }
}

// ═══ Attachments ═══════════════════════════════════════════════════════════════

function renderAttachments() {
    const preview = document.getElementById('chat-attachments-preview');
    if (chatAttachments.length === 0) { preview.style.display = 'none'; return; }
    preview.style.display = 'flex';
    preview.innerHTML = chatAttachments.map((a, i) => `
        <div class="chat-attachment">
            <span>${escapeHtml(a.split('/').pop())}</span>
            <span class="remove" onclick="removeAttachment(${i})">✕</span>
        </div>`).join('');
}

function removeAttachment(i) { chatAttachments.splice(i, 1); renderAttachments(); }

async function chatHandleFiles(files) {
    if (!files.length) return;
    const form = new FormData();
    for (const f of files) form.append('files', f);
    try {
        const res = await fetch('/api/upload-file', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${authToken}` },
            body: form
        });
        const data = await res.json();
        if (data.files) {
            chatAttachments.push(...data.files);
            renderAttachments();
        } else if (data.error) addChatMessage('feedback', data.error);
    } catch (err) { addChatMessage('feedback', 'Upload failed: ' + err.message); }
    document.getElementById('chat-file-input').value = '';
}

async function chatUploadBotAvatar() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/*';
    input.onchange = async () => {
        const file = input.files[0];
        if (!file) return;
        const form = new FormData();
        form.append('bot', file);
        try {
            const res = await fetch('/api/upload-avatar', {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${authToken}` },
                body: form
            });
            const data = await res.json();
            if (data.success) {
                const img = document.getElementById('chat-bot-avatar');
                img.src = data.url + '?t=' + Date.now();
            }
            else alert(data.error || 'Upload failed');
        } catch (err) { alert('Upload error: ' + err.message); }
    };
    input.click();
}

// ═══ Avatar Upload ══════════════════════════════════════════════════════════════

async function chatUploadAvatar() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/*';
    input.onchange = async () => {
        const file = input.files[0];
        if (!file) return;
        const form = new FormData();
        // Use the human-readable username so the avatar is shared across all
        // chat sessions belonging to the same user.
        const key = (chatUsername && chatUsername !== 'User') ? chatUsername : chatUserId;
        form.append(key, file);
        try {
            const res = await fetch('/api/upload-avatar', {
                method: 'POST',
                headers: { 'Authorization': `Bearer ${authToken}` },
                body: form
            });
            const data = await res.json();
            if (data.success) loadAvatar();
            else alert(data.error || 'Upload failed');
        } catch (err) { alert('Upload error: ' + err.message); }
    };
    input.click();
}

// ═══ SM Status ════════════════════════════════════════════════════════════════

function contextSmFile(ctx) {
    return ctx?.sm_file || ctx?.cl_file || '';
}

function contextSmData(ctx) {
    return Object.prototype.hasOwnProperty.call(ctx || {}, 'sm_data')
        ? (ctx.sm_data || {}) : (ctx?.cl_data || {});
}

async function loadCLStatus() {
    try {
        const encodedUser = encodeURIComponent(chatUserId);
        let res = await apiGet(`/api/sm/${encodedUser}`);
        if (!res.ok && res.status === 404) {
            // Legacy route retained while backend route names converge.
            res = await apiGet(`/api/cl/${encodedUser}`);
        }
        const data = await res.json();
        const bar = document.getElementById('chat-sm-status');
        const smFile = contextSmFile(data);
        const vars = contextSmData(data);
        if (smFile || data.system_template || data.active_state || Object.keys(vars).length > 0) {
            bar.style.display = 'flex';
            document.getElementById('sm-status-file').textContent = smFile ? `Statemachine: ${smFile}` : '-';
            document.getElementById('sm-status-state').textContent = data.active_state ? `State: ${data.active_state}` : '-';
            const temps = data.active_templates || [];
            const templateBadge = document.getElementById('sm-status-temps');
            templateBadge.textContent = data.system_template
                ? `System: ${data.system_template}`
                : (temps.length ? `Templates: ${temps.join(', ')}` : '-');
            templateBadge.title = temps.length ? `Template stack: ${temps.join(', ')}` : 'Effective system template';
            templateBadge.className = data.system_template || temps.length ? 'sm-badge template' : 'sm-badge';
            const skillBadge = document.getElementById('sm-status-skill');
            skillBadge.textContent = data.active_skill ? `Skill: ${data.active_skill}` : '';
            skillBadge.style.display = data.active_skill ? '' : 'none';
            // sm_data is canonical; contextSmData accepts older API responses.
            const varKeys = Object.keys(vars).filter(k => k !== 'active_state' && k !== 'active_templates');
            const varsEl = document.getElementById('sm-status-vars');
            if (varKeys.length > 0) {
                varsEl.textContent = varKeys.slice(0, 5).map(k => `${k}: ${JSON.stringify(vars[k]).substring(0, 40)}`).join(', ');
                varsEl.style.display = '';
            } else {
                varsEl.style.display = 'none';
            }
        } else {
            bar.style.display = 'none';
        }
    } catch {}
}

// ═══ Contexts ════════════════════════════════════════════════════════════════

async function loadContexts() {
    try {
        const res = await apiGet('/api/contexts');
        if (!res.ok) { document.getElementById('contexts-list').innerHTML = errorHtml(res.status); return; }
        const data = await res.json();
        const list = document.getElementById('contexts-list');
        if (!data.contexts || data.contexts.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No contexts found</span></div>'; return;
        }
        list.innerHTML = data.contexts.map(ctx => {
            const uid = ctx.user_id || '';
            const safeUid = uid.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
            return `<div class="data-item">
                <div><span class="name">${escapeHtml(uid)}</span>
                <span class="meta">Updated: ${ctx.updated_at ? new Date(ctx.updated_at).toLocaleString() : '-'}</span></div>
                <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="viewContext('${safeUid}')">View</button>
                <button class="btn btn-sm btn-danger" onclick="deleteContext('${safeUid}')">Delete</button>
                </div></div>`;
        }).join('');
    } catch (err) { document.getElementById('contexts-list').innerHTML = errorHtml(err.message); }
}

async function viewContext(userId) {
    try {
        const res = await apiGet(`/api/contexts/${encodeURIComponent(userId)}`);
        if (!res.ok) { alert('Failed to load context'); return; }
        const ctx = await res.json();

        const redundantKeys = new Set([
            'mimo_api_key', 'minimax_api_key', 'voice_elevenlabs_api_key', 'voice_elevenlabs_stt_api_key',
            'sm_file', 'cl_file', 'active_state', 'active_templates', 'llm_turn', 'compaction_summary', 'download'
        ]);

        const filteredSettings = ctx.settings ? Object.entries(ctx.settings).filter(([k]) => !redundantKeys.has(k)) : [];
        const settingsHtml = filteredSettings.length > 0
            ? filteredSettings.map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v ?? ''))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">No settings</span></div>';

        const customDataHtml = ctx.custom_data && Object.keys(ctx.custom_data).length > 0
            ? Object.entries(ctx.custom_data).map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const smData = contextSmData(ctx);
        const smDataHtml = Object.keys(smData).length > 0
            ? Object.entries(smData).map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const uid = escapeHtml(userId);
        showModal('Context: ' + uid, `
            <div id="context-view-mode">
                <div class="data-list" style="margin-bottom:1rem">
                    <div class="data-item"><span class="name">User ID</span><span class="meta">${escapeHtml(ctx.user_id || '')}</span></div>
                    <div class="data-item"><span class="name">Turn</span><span class="meta">${ctx.turn ?? 0}</span></div>
                    <div class="data-item"><span class="name">Mode</span><span class="meta">${escapeHtml(ctx.mode || '')}</span></div>
                    <div class="data-item"><span class="name">Statemachine</span><span class="meta">${escapeHtml(contextSmFile(ctx) || '-')}</span></div>
                    <div class="data-item"><span class="name">Active State</span><span class="meta">${escapeHtml(ctx.active_state || '-')}</span></div>
                    <div class="data-item"><span class="name">Active Templates</span><span class="meta">${(ctx.active_templates || []).join(', ') || '-'}</span></div>
                </div>
                <h3>Settings</h3><div class="data-list" style="margin-bottom:1rem;max-height:200px;overflow-y:auto">${settingsHtml}</div>
                <h3>Custom Data</h3><div class="data-list" style="margin-bottom:1rem">${customDataHtml}</div>
                <h3>SM Data</h3><div class="data-list" style="margin-bottom:1rem">${smDataHtml}</div>
                <button class="btn btn-primary" style="width:auto" id="ctx-edit-btn">Edit</button>
                <button class="btn btn-danger" style="width:auto" id="ctx-delete-btn">Delete</button>
            </div>
            <div id="context-edit-mode" style="display:none">
                <textarea id="context-edit-json" class="code-editor" style="min-height:400px">${escapeHtml(JSON.stringify(ctx, null, 2))}</textarea>
                <div style="display:flex;gap:0.5rem;margin-top:1rem">
                    <button class="btn btn-primary" style="width:auto" id="ctx-save-btn">Save</button>
                    <button class="btn btn-secondary" style="width:auto" id="ctx-cancel-btn">Cancel</button>
                </div>
            </div>`);
        document.getElementById('ctx-edit-btn').onclick = () => {
            document.getElementById('context-view-mode').style.display = 'none';
            document.getElementById('context-edit-mode').style.display = 'block';
        };
        document.getElementById('ctx-save-btn').onclick = () => saveContextEdit(userId);
        document.getElementById('ctx-cancel-btn').onclick = () => {
            document.getElementById('context-view-mode').style.display = 'block';
            document.getElementById('context-edit-mode').style.display = 'none';
        };
        document.getElementById('ctx-delete-btn').onclick = () => deleteContext(userId);
    } catch (err) { alert('Failed: ' + err.message); }
}

async function saveContextEdit(userId) {
    let updates;
    try { updates = JSON.parse(document.getElementById('context-edit-json').value); }
    catch (e) { alert('Invalid JSON: ' + e.message); return; }
    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(userId)}`, {
            method: 'PUT', body: JSON.stringify(updates)
        });
        if (res.ok) viewContext(userId);
        else alert('Save failed: ' + await res.text());
    } catch (err) { alert('Failed: ' + err.message); }
}

async function deleteContext(userId) {
    if (!confirm(`Delete context for "${userId}"?`)) return;
    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(userId)}`, { method: 'DELETE' });
        if (res.ok) { closeModal(); loadContexts(); }
        else alert('Failed: ' + await res.text());
    } catch (err) { alert('Failed: ' + err.message); }
}

// ═══ Templates ════════════════════════════════════════════════════════════════

async function loadTemplates() {
    try {
        const res = await apiGet('/api/templates');
        const data = await res.json();
        const list = document.getElementById('templates-list');
        let html = '<div class="data-item" style="margin-bottom:0.5rem"><button class="btn btn-sm btn-primary" onclick="createTemplate()">+ New Template</button></div>';
        if (!data.templates || data.templates.length === 0) {
            html += '<div class="data-item"><span class="name">No templates found</span></div>';
        } else {
            const folders = {}, root = [];
            for (const t of data.templates) {
                const idx = t.name.lastIndexOf('/');
                if (idx > 0) { const f = t.name.substring(0, idx); if (!folders[f]) folders[f] = []; folders[f].push(t); }
                else root.push(t);
            }
            const renderTpl = t => `<div class="data-item" style="padding-left:1.5rem">
                <div><span class="name">${escapeHtml(t.name.split('/').pop())}</span><span class="meta">${t.is_system ? 'System' : 'User'}</span></div>
                <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="editTemplate('${escapeHtml(t.name)}')">Edit</button>
                <button class="btn btn-sm btn-danger" onclick="deleteTemplate('${escapeHtml(t.name)}')">Delete</button>
                </div></div>`;
            for (const t of root) html += renderTpl(t);
            for (const folder of Object.keys(folders).sort()) {
                html += `<div class="data-item" style="cursor:pointer;border-left:2px solid var(--border);margin-top:0.5rem" onclick="this.nextElementSibling.style.display=this.nextElementSibling.style.display==='none'?'block':'none'">
                    <span class="name" style="font-weight:600">&#128193; ${escapeHtml(folder)}/</span><span class="meta">${folders[folder].length} template(s)</span></div><div style="display:block">`;
                for (const t of folders[folder]) html += renderTpl(t);
                html += '</div>';
            }
        }
        list.innerHTML = html;
    } catch (err) { console.error('Templates:', err); }
}

function createTemplate() {
    showModal('Create Template', `
        <div style="margin-bottom:0.5rem">
            <label style="font-size:0.8rem;color:var(--text-secondary)">Template name (use / for folders):</label>
            <input id="new-template-name" type="text" placeholder="tasks/my_task" style="width:100%;padding:0.3rem;margin-top:0.2rem">
        </div>
        <textarea id="template-content" class="code-editor"><poml>
  <task>
    <p>Your template content here</p>
  </task>
</poml></textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewTemplate()">Create</button>`);
}

async function saveNewTemplate() {
    const name = document.getElementById('new-template-name').value.trim();
    const content = document.getElementById('template-content').value;
    if (!name) { alert('Name is required'); return; }
    await apiFetch('/api/templates', { method: 'POST', body: JSON.stringify({ name, content }) });
    closeModal(); loadTemplates();
}

async function deleteTemplate(name) {
    if (!confirm(`Delete template "${name}"?`)) return;
    await apiFetch(`/api/templates/${name}`, { method: 'DELETE' });
    loadTemplates();
}

async function editTemplate(name) {
    const res = await apiGet(`/api/templates/${name}`);
    const tpl = await res.json();
    let userOptions = '<option value="">No user (skip preview)</option>';
    try {
        const ctxRes = await apiGet('/api/contexts');
        if (ctxRes.ok) {
            const ctxData = await ctxRes.json();
            userOptions += (ctxData.contexts || []).map(c => c.user_id).filter(Boolean)
                .map(uid => `<option value="${escapeHtml(uid)}">${escapeHtml(uid)}</option>`).join('');
        }
    } catch {}
    showModal('Edit Template: ' + name, `
        <div style="margin-bottom:0.5rem">
            <label style="font-size:0.8rem;color:var(--text-secondary)">Preview as user:</label>
            <select id="template-preview-user" style="width:100%;padding:0.3rem;margin-top:0.2rem">${userOptions}</select>
        </div>
        <textarea id="template-content" class="code-editor">${escapeHtml(tpl.content || '')}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveTemplate('${escapeHtml(name)}')">Save & Preview</button>
        <div id="template-preview" style="margin-top:1rem;display:none"></div>`);
}

async function saveTemplate(name) {
    const content = document.getElementById('template-content').value;
    const userId = document.getElementById('template-preview-user')?.value || '';
    const previewEl = document.getElementById('template-preview');
    const res = await apiFetch(`/api/templates/${name}`, {
        method: 'PUT', body: JSON.stringify({ content, user_id: userId })
    });
    const data = await res.json();
    if (previewEl) {
        previewEl.style.display = 'block';
        previewEl.innerHTML = data.rendered_preview
            ? `<div style="font-size:0.75rem;color:var(--text-secondary);margin-bottom:0.3rem">Rendered preview:</div><pre style="background:var(--bg-tertiary,#1a1a2e);padding:0.75rem;border-radius:6px;white-space:pre-wrap;font-size:0.8rem;max-height:400px;overflow:auto">${escapeHtml(data.rendered_preview)}</pre>`
            : `<div style="color:var(--error,#f44)">${escapeHtml(data.error || '')}</div>`;
    }
    if (data.success) loadTemplates();
}

// ═══ Tools ════════════════════════════════════════════════════════════════

async function loadTools() {
    try {
        const res = await apiGet('/api/tools');
        const data = await res.json();
        const list = document.getElementById('tools-list');
        if (!data.tools || data.tools.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No tools found</span></div>'; return;
        }
        list.innerHTML = data.tools.map(t => {
            const safeName = escapeHtml(t.name);
            const nameArg = JSON.stringify(String(t.name || ''));
            return `<div class="data-item tool-item">
                <div class="tool-copy"><span class="name">${safeName}</span><span class="meta">${escapeHtml(t.description || '')}</span></div>
                <button type="button" class="toggle ${t.is_enabled ? 'active' : ''}" aria-label="${t.is_enabled ? 'Disable' : 'Enable'} ${safeName}" aria-pressed="${t.is_enabled ? 'true' : 'false'}" onclick='toggleTool(${nameArg}, ${!t.is_enabled})'></button>
            </div>`;
        }).join('');
    } catch (err) { console.error('Tools error:', err); }
}

async function toggleTool(name, enabled) {
    await apiFetch(`/api/tools/${name}`, { method: 'PUT', body: JSON.stringify({ is_enabled: enabled }) });
    loadTools();
}

// ═══ Secrets ════════════════════════════════════════════════════════════════

async function loadSecrets() {
    try {
        const res = await apiGet('/api/secrets');
        const data = await res.json();
        const container = document.getElementById('secrets-content');
        const knownKeys = ['discord_bot_token', 'openai_api_key', 'anthropic_api_key', 'llamacpp_api_key', 'minimax_api_key',
            'mimo_api_key', 'elevenlabs_api_key', 'gateway_api_key', 'dashboard_admin_password'];
        const customKeys = Object.keys(data).filter(k => !knownKeys.includes(k));

        const builtInHtml = knownKeys.map(key => `<div class="data-item">
            <span class="name">${escapeHtml(key)}</span><span class="meta">${escapeHtml(data[key] || 'Not set')}</span>
        </div>`).join('');

        const customHtml = customKeys.length > 0 ? `<h3 style="margin-top:1rem">Custom Secrets</h3>
            <div class="data-list">${customKeys.map(key => `<div class="data-item">
                <span class="name">${escapeHtml(key)}</span><span class="meta">${escapeHtml(data[key] || 'Not set')}</span>
                <button class="btn btn-sm btn-danger" style="margin-left:auto" onclick="deleteCustomSecret('${escapeHtml(key)}')">Delete</button>
            </div>`).join('')}</div>` : '';

        container.innerHTML = `
            <div class="data-list">${builtInHtml}</div>${customHtml}
            <div style="margin-top:1.5rem"><h3>Update Secret</h3>
            <div class="form-group"><label>Field</label><select id="secret-field" style="width:100%;padding:0.75rem;background:var(--bg-secondary);border:1px solid var(--border);border-radius:8px;color:var(--text-primary)">
                ${knownKeys.map(k => `<option value="${k}">${escapeHtml(k)}</option>`).join('')}
                ${customKeys.map(k => `<option value="${k}">${escapeHtml(k)} (custom)</option>`).join('')}
            </select></div>
            <div class="form-group"><label>New Value</label><input type="password" id="secret-value" placeholder="Enter new value"></div>
            <div class="form-group"><label>Master Password (required to save to disk)</label><input type="password" id="secret-master" placeholder="Enter MASTER_KEY"></div>
            <button class="btn btn-primary" onclick="saveSecret()">Save Secret</button><p id="secret-msg" class="hidden" style="margin-top:0.5rem"></p></div>
            <div style="margin-top:1.5rem"><h3>Add Custom Secret</h3>
            <div class="form-group"><label>Key Name</label><input type="text" id="custom-secret-key" placeholder="e.g. MY_API_KEY"></div>
            <div class="form-group"><label>Value</label><input type="password" id="custom-secret-value" placeholder="Enter value"></div>
            <div class="form-group"><label>Master Password</label><input type="password" id="custom-secret-master" placeholder="Enter MASTER_KEY"></div>
            <button class="btn btn-primary" onclick="addCustomSecret()">Add Custom Secret</button><p id="custom-secret-msg" class="hidden" style="margin-top:0.5rem"></p></div>`;
    } catch (err) { console.error('Secrets error:', err); }
}

async function saveSecret() {
    const field = document.getElementById('secret-field').value;
    const value = document.getElementById('secret-value').value;
    const master = document.getElementById('secret-master').value;
    const msgEl = document.getElementById('secret-msg');
    if (!value) { msgEl.textContent = 'Value is required'; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); return; }
    const body = { [field]: value };
    if (master) body.master_password = master;
    try {
        const res = await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify(body) });
        msgEl.textContent = await res.text();
        msgEl.style.color = 'var(--success)'; msgEl.classList.remove('hidden');
        document.getElementById('secret-value').value = '';
        document.getElementById('secret-master').value = '';
        setTimeout(loadSecrets, 1500);
    } catch (err) { msgEl.textContent = 'Failed: ' + err.message; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); }
}

async function addCustomSecret() {
    const key = document.getElementById('custom-secret-key').value.trim();
    const value = document.getElementById('custom-secret-value').value;
    const master = document.getElementById('custom-secret-master').value;
    const msgEl = document.getElementById('custom-secret-msg');
    if (!key || !value) { msgEl.textContent = 'Key and value are required'; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); return; }
    const body = { [key]: value };
    if (master) body.master_password = master;
    try {
        const res = await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify(body) });
        msgEl.textContent = await res.text();
        msgEl.style.color = 'var(--success)'; msgEl.classList.remove('hidden');
        document.getElementById('custom-secret-key').value = '';
        document.getElementById('custom-secret-value').value = '';
        document.getElementById('custom-secret-master').value = '';
        setTimeout(loadSecrets, 1500);
    } catch (err) { msgEl.textContent = 'Failed: ' + err.message; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); }
}

async function deleteCustomSecret(key) {
    if (!confirm(`Delete custom secret "${key}"?`)) return;
    try {
        await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify({ [key]: '' }) });
        loadSecrets();
    } catch (err) { alert('Failed to delete: ' + err.message); }
}

// ═══ Pairings ════════════════════════════════════════════════════════════════

async function loadPairings() {
    try {
        const [pairingsRes, pendingRes] = await Promise.all([apiGet('/api/pairings'), apiGet('/api/pairings/pending')]);
        const data = await pairingsRes.json();
        const pendingData = await pendingRes.json();
        document.getElementById('pairings-list').innerHTML = (data.pairings || []).length
            ? data.pairings.map(p => `<div class="data-item">
                <div><span class="name">${escapeHtml(p.user_id)}</span><span class="meta">Discord: ${escapeHtml(p.discord_user_id)} | Paired: ${escapeHtml(p.paired_at || '-')}</span></div>
                <div class="actions"><button class="btn btn-sm btn-danger" onclick="deletePairing('${escapeHtml(p.user_id)}')">Delete</button></div>
            </div>`).join('')
            : '<div class="data-item"><span class="name">No pairings found</span></div>';
        document.getElementById('pending-pairings-list').innerHTML = (pendingData.pending_pairings || []).length
            ? pendingData.pending_pairings.map(p => `<div class="data-item">
                <div><span class="name">${escapeHtml(p.code)}</span><span class="meta">Discord: ${escapeHtml(p.discord_user_id)} | Expires: ${escapeHtml(p.expires_at)}</span></div>
                <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="approvePendingPairing('${escapeHtml(p.code)}')">Approve</button>
                <button class="btn btn-sm btn-danger" onclick="deletePendingPairing('${escapeHtml(p.code)}')">Delete</button>
                </div></div>`).join('')
            : '<div class="data-item"><span class="name">No pending pairings</span></div>';
    } catch (err) { console.error('Pairings error:', err); }
}

async function deletePairing(userId) { if (!confirm('Delete this pairing?')) return; await apiFetch(`/api/pairings/${userId}`, { method: 'DELETE' }); loadPairings(); }
async function approvePendingPairing(code) { if (!confirm('Approve this pending pairing?')) return; await apiFetch(`/api/pairings/pending/${code}/approve`, { method: 'POST' }); loadPairings(); }
async function deletePendingPairing(code) { if (!confirm('Delete this pending pairing?')) return; await apiFetch(`/api/pairings/pending/${code}`, { method: 'DELETE' }); loadPairings(); }

// ═══ Statemachine Files ═════════════════════════════════════════════════════

async function loadSmFiles() {
    try {
        const res = await apiGet('/api/sm-files');
        const data = await res.json();
        const list = document.getElementById('sm-files-list');
        if (!data.sm_files || data.sm_files.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No statemachine files found</span></div>'; return;
        }
        list.innerHTML = data.sm_files.map(f => `<div class="data-item">
            <span class="name">${escapeHtml(f.name)}</span>
            <div class="actions"><button class="btn btn-sm btn-primary" onclick="editSmFile('${escapeHtml(f.name)}')">Edit</button></div>
        </div>`).join('');
    } catch (err) { console.error('SM files error:', err); }
}

function createSmFile() {
    showModal('New Statemachine', `
        <div class="form-group"><label for="new-sm-name">File Name</label><input type="text" id="new-sm-name" placeholder="e.g. my_workflow.sm"></div>
        <textarea id="sm-content" class="code-editor" placeholder="[state start]\nmode = chat\n\n[transitions]\n-> done"></textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewSmFile()">Create</button>`);
}

async function saveNewSmFile() {
    let name = document.getElementById('new-sm-name').value.trim();
    const content = document.getElementById('sm-content').value;
    if (!name) { alert('File name is required'); return; }
    if (!name.endsWith('.sm')) name += '.sm';
    const res = await apiFetch(`/api/sm-files/${name}`, { method: 'PUT', body: JSON.stringify({ content }) });
    const result = await res.text();
    if (result.includes('Error')) alert(result);
    else { closeModal(); loadSmFiles(); }
}

async function editSmFile(name) {
    const res = await apiGet(`/api/sm-files/${name}`);
    const content = await res.text();
    showModal('Edit Statemachine: ' + name, `
        <textarea id="sm-content" class="code-editor">${escapeHtml(content)}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveSmFile('${escapeHtml(name)}')">Save</button>`);
}

async function saveSmFile(name) {
    const content = document.getElementById('sm-content').value;
    const res = await apiFetch(`/api/sm-files/${name}`, { method: 'PUT', body: JSON.stringify({ content }) });
    const result = await res.text();
    if (result.includes('Error')) alert(result);
    else { closeModal(); loadSmFiles(); }
}

// ═══ Cron Jobs ════════════════════════════════════════════════════════════════

async function loadCronJobs() {
    try {
        const res = await apiGet('/api/cron-jobs');
        const data = await res.json();
        const list = document.getElementById('cron-jobs-list');
        if (!data.cron_jobs || data.cron_jobs.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No cron jobs found</span></div>'; return;
        }
        list.innerHTML = data.cron_jobs.map(j => `<div class="data-item">
            <div><span class="name">${escapeHtml(j.name)}</span><span class="meta">${escapeHtml(j.schedule)} | Runs: ${j.run_count}</span></div>
            <div class="toggle ${j.enabled ? 'active' : ''}"></div>
        </div>`).join('');
    } catch (err) { console.error('Cron error:', err); }
}

// ═══ Messages & Memory ══════════════════════════════════════════════════════

async function populateUserDropdowns() {
    try {
        const res = await apiGet('/api/contexts');
        if (!res.ok) return;
        const data = await res.json();
        const users = (data.contexts || []).map(c => c.user_id).filter(Boolean);
        for (const selectId of ['message-user-id', 'memory-user-id']) {
            const select = document.getElementById(selectId);
            if (!select) continue;
            const current = select.value;
            select.innerHTML = '<option value="">Select user...</option>' +
                users.map(uid => `<option value="${escapeHtml(uid)}">${escapeHtml(uid)}</option>`).join('');
            if (current && users.includes(current)) select.value = current;
        }
    } catch {}
}

async function loadMessages() {
    const userId = document.getElementById('message-user-id').value;
    if (!userId) { alert('Please select a User'); return; }
    const list = document.getElementById('messages-list');
    list.innerHTML = '<div class="data-item"><span class="name">Loading...</span></div>';
    try {
        const res = await apiGet(`/api/messages/${encodeURIComponent(userId)}`);
        if (!res.ok) { list.innerHTML = `<div class="data-item"><span style="color:var(--error)">Error ${res.status}</span></div>`; return; }
        const data = await res.json();
        if (!data.messages || data.messages.length === 0) { list.innerHTML = '<div class="data-item"><span class="name">No messages found</span></div>'; return; }
        list.innerHTML = data.messages.map(m => {
            let toolCallsHtml = '';
            if (m.tool_calls && m.tool_calls.length > 0) {
                toolCallsHtml = '<div class="tool-calls">' + m.tool_calls.map(tc =>
                    `<div class="tool-call"><span class="tool-name">${escapeHtml(tc.name)}</span>: <span class="tool-args">${escapeHtml(tc.arguments)}</span></div>`
                ).join('') + '</div>';
            }
            return `<div class="message ${m.role}">
                <div class="role">${escapeHtml(m.role)}${m.tool_call_id ? ' (tool: ' + escapeHtml(m.tool_call_id) + ')' : ''}</div>
                ${toolCallsHtml}
                <div class="content">${escapeHtml(m.content || '')}</div>
            </div>`;
        }).join('');
        const info = document.createElement('div'); info.className = 'data-item';
        info.innerHTML = `<span class="name">${data.message_count} messages (~${data.total_tokens} tokens)</span>`;
        list.prepend(info);
    } catch (err) { list.innerHTML = `<div class="data-item"><span style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`; }
}

async function loadMemory() {
    const userId = document.getElementById('memory-user-id').value;
    if (!userId) { alert('Please select a User'); return; }
    const container = document.getElementById('memory-content');
    container.innerHTML = '<div class="data-item"><span class="name">Loading...</span></div>';
    try {
        const res = await apiGet(`/api/memory/${encodeURIComponent(userId)}`);
        if (!res.ok) { container.innerHTML = `<div class="data-item"><span style="color:var(--error)">Error ${res.status}</span></div>`; return; }
        const data = await res.json();
        container.innerHTML = `
            <h3>Learned Facts</h3>
            <div class="data-list">${(data.learned_facts || []).map(f => `<div class="data-item"><span class="name">${escapeHtml(f)}</span></div>`).join('') || '<div class="data-item"><span class="name">None</span></div>'}</div>
            <h3 style="margin-top:1rem">Last Topics</h3>
            <div class="data-list">${(data.last_topics || []).map(t => `<div class="data-item"><span class="name">${escapeHtml(t)}</span></div>`).join('') || '<div class="data-item"><span class="name">None</span></div>'}</div>
            <h3 style="margin-top:1rem">Custom Variables</h3>
            <pre class="code-editor">${JSON.stringify(data.custom_variables || {}, null, 2)}</pre>`;
    } catch (err) { container.innerHTML = `<div class="data-item"><span style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`; }
}

// ═══ VM ════════════════════════════════════════════════════════════════

let vncRfb = null;
let vncConnectedVm = null;
let vncModule = null;
let clipboardEnabled = false;

function vncLog(msg, level) {
    const log = document.getElementById('vm-vnc-log');
    if (!log) return;
    const time = new Date().toLocaleTimeString();
    const colors = { info: 'var(--text-primary)', warn: '#FFD600', error: '#FF5252', success: '#4CAF50' };
    const div = document.createElement('div');
    div.innerHTML = `<span style="color:var(--text-secondary)">${time}</span> <span style="color:${colors[level] || colors.info}">${escapeHtml(msg)}</span>`;
    log.appendChild(div); log.scrollTop = log.scrollHeight;
}

async function loadVM() {
    try { await loadVMStatus(); await loadVMActivity(); }
    catch (err) { console.error('Failed to load VM:', err); }
    startVMRefreshLoop();
}

function startVMRefreshLoop() {
    if (vmRefreshInterval) clearInterval(vmRefreshInterval);
    vmRefreshInterval = setInterval(() => { loadVMStatus(); }, 5000);
}

async function loadVMStatus() {
    const container = document.getElementById('vm-status-bar');
    try {
        const res = await apiGet('/api/vm');
        if (!res.ok) {
            container.innerHTML = '<div class="data-item"><span class="name" style="color:var(--text-secondary)">VM feature not available. Add VM_ENABLED=true to .env</span></div>';
            return;
        }
        const data = await res.json();
        const vms = data.vms || [];

        if (vms.length === 0) {
            container.innerHTML = '<div class="data-item"><span class="name">No VMs running</span></div>';
            disconnectVNC();
        } else {
            container.innerHTML = vms.map(vm => {
                const isStopped = vm.status === 'stopped' || vm.status === 'stopped';
                return `<div class="data-item">
                    <div>
                        <span class="name">${escapeHtml(vm.name)}</span>
                        <span class="meta">Status: ${escapeHtml(vm.status)} | PID: ${vm.pid || '-'} | VNC: ${vm.vnc_port || '-'} | Layout: ${(vm.keyboard_layout || 'us').toUpperCase()}${vm.current_iso ? ' | CD: ' + escapeHtml(vm.current_iso.split('/').pop()) : ''}</span>
                    </div>
                    <div class="actions">
                        ${isStopped ? `<button class="btn btn-sm btn-primary" onclick="vmStartByName('${escapeHtml(vm.name)}')">Start</button>` : ''}
                        ${vm.status === 'running' ? `<button class="btn btn-sm btn-danger" onclick="vmStop('${escapeHtml(vm.name)}')">Stop</button>` : ''}
                        ${vm.status === 'running' ? `<button class="btn btn-sm btn-secondary" onclick="vmReboot('${escapeHtml(vm.name)}')">Reboot</button>` : ''}
                    </div>
                </div>`;
            }).join('');

            const runningVm = vms.find(vm => vm.status === 'running');
            if (runningVm) {
                if (vncConnectedVm !== runningVm.name) connectVNC(runningVm.name);
            } else { disconnectVNC(); }
        }

        const config = data.config || {};
        if (config.vm_enabled !== undefined) {
            document.getElementById('vm-config').innerHTML = `
                <div class="data-item"><span class="name">VM Enabled</span><span class="meta">${config.vm_enabled ? 'Yes' : 'No'}</span></div>
                <div class="data-item"><span class="name">CPU Cores</span><span class="meta">${config.vm_cpu_cores || '-'}</span></div>
                <div class="data-item"><span class="name">RAM (MB)</span><span class="meta">${config.vm_ram_mb || '-'}</span></div>
                <div class="data-item"><span class="name">Disk Size</span><span class="meta">${config.vm_disk_size || '-'}</span></div>
                <div class="data-item"><span class="name">Architecture</span><span class="meta">${config.vm_arch || '-'}</span></div>`;
        }
    } catch (err) { container.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`; }
}

async function vmStartByName(name) {
    let savedLayout = 'us';
    try {
        const ctxRes = await apiGet('/api/contexts/default');
        if (ctxRes.ok) { const ctx = await ctxRes.json(); savedLayout = ctx.settings?.vm_keyboard_layout || 'us'; }
    } catch {}

    const layouts = [
        { value: 'us', label: 'US (QWERTY)' }, { value: 'de', label: 'DE (QWERTZ)' },
        { value: 'fr', label: 'FR (AZERTY)' }, { value: 'es', label: 'ES (Spanish)' },
        { value: 'it', label: 'IT (Italian)' }, { value: 'gb', label: 'GB (British)' },
    ];
    const layoutOptions = layouts.map(l => `<option value="${l.value}" ${l.value === savedLayout ? 'selected' : ''}>${l.label}</option>`).join('');

    showModal('Start VM', `
        <div class="form-group"><label>VM Name</label><input type="text" id="vm-start-name" value="${escapeHtml(name)}" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>CPU Cores</label><input type="number" id="vm-start-cpu" value="2" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>RAM (MB)</label><input type="number" id="vm-start-ram" value="4096" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>Disk Size</label><input type="text" id="vm-start-disk" value="40G" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>ISO Path (optional)</label><input type="text" id="vm-start-iso" placeholder="/path/to/linux.iso" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>Keyboard Layout</label><select id="vm-start-layout" style="width:100%;padding:0.5rem">${layoutOptions}</select></div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoStart()">Start VM</button>`);
}

async function vmStart() { vmStartByName('praxis-vm'); }

async function vmDoStart() {
    const body = {
        name: document.getElementById('vm-start-name').value || 'praxis-vm',
        cpu_cores: parseInt(document.getElementById('vm-start-cpu').value) || 2,
        ram_mb: parseInt(document.getElementById('vm-start-ram').value) || 4096,
        disk_size: document.getElementById('vm-start-disk').value || '40G',
        keyboard_layout: document.getElementById('vm-start-layout').value || 'us',
    };
    const iso = document.getElementById('vm-start-iso').value;
    if (iso) body.iso_path = iso;
    try {
        const res = await apiFetch('/api/vm/start', { method: 'POST', body: JSON.stringify(body) });
        const data = await res.json();
        closeModal();
        alert(data.message || data.error || 'Done');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmStop(name) {
    name = name || 'praxis-vm';
    if (!confirm(`Stop VM "${name}"?`)) return;
    try {
        const res = await apiFetch('/api/vm/stop', { method: 'POST', body: JSON.stringify({ name }) });
        const data = await res.json();
        alert(data.message || data.error || 'Stopped');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmReboot(name) {
    name = name || 'praxis-vm';
    if (!confirm('Reboot VM?')) return;
    try {
        const res = await apiFetch('/api/vm/reboot', { method: 'POST', body: JSON.stringify({ name }) });
        const data = await res.json();
        alert(data.message || data.error || 'Rebooting');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

function vmRefresh() { loadVM(); }

async function vmInsertCD() {
    const iso = prompt('Path to ISO file:');
    if (!iso) return;
    try {
        const res = await apiFetch('/api/vm/cd', { method: 'POST', body: JSON.stringify({ name: 'praxis-vm', iso_path: iso }) });
        const data = await res.json();
        alert(data.message || data.error || 'CD inserted');
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmEjectCD() {
    try {
        const res = await apiFetch('/api/vm/cd', { method: 'POST', body: JSON.stringify({ name: 'praxis-vm', iso_path: null }) });
        const data = await res.json();
        alert(data.message || data.error || 'CD ejected');
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmCreateSnapshot() {
    const name = prompt('Snapshot name:');
    if (!name) return;
    try {
        const res = await apiFetch('/api/vm/snapshot', { method: 'POST', body: JSON.stringify({ snapshot_name: name }) });
        const data = await res.json();
        alert(data.message || data.error || 'Snapshot created');
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmAddSharedFolder() {
    showModal('Add Shared Folder', `
        <div class="form-group"><label>Host Path</label><input type="text" id="vm-sf-host" placeholder="/home/user/projects" style="width:100%;padding:0.5rem"></div>
        <div class="form-group"><label>Mount Point (in VM)</label><input type="text" id="vm-sf-mount" value="/mnt/projects" style="width:100%;padding:0.5rem"></div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoAddSharedFolder()">Add</button>`);
}

async function vmDoAddSharedFolder() {
    const host_path = document.getElementById('vm-sf-host').value;
    const mount_point = document.getElementById('vm-sf-mount').value;
    if (!host_path) { alert('Host path required'); return; }
    try {
        const res = await apiFetch('/api/vm/shared-folder', { method: 'POST', body: JSON.stringify({ host_path, mount_point }) });
        const data = await res.json();
        closeModal();
        alert(data.message || data.error || 'Shared folder added');
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmClipboardToggle() {
    clipboardEnabled = !clipboardEnabled;
    const btn = document.getElementById('vm-clipboard-btn');
    if (clipboardEnabled) {
        btn.textContent = '📋 Clipboard: ON';
        btn.style.background = 'rgba(0,217,255,0.2)';
        btn.style.color = 'var(--accent-cyan)';
        btn.style.borderColor = 'var(--accent-cyan)';
        alert('Shared clipboard enabled. You can now copy text from the dashboard to the VM clipboard and vice versa.');
    } else {
        btn.textContent = '📋 Clipboard: OFF';
        btn.style.background = '';
        btn.style.color = '';
        btn.style.borderColor = '';
    }
}

async function vmClipboardSet() {
    const text = prompt('Enter text to copy to VM clipboard:');
    if (!text) return;
    try {
        const res = await apiFetch('/api/vm/clipboard/set', {
            method: 'POST',
            body: JSON.stringify({ name: 'praxis-vm', content: text })
        });
        const data = await res.json();
        if (data.success) vncLog(`Clipboard set: ${text.substring(0, 30)}${text.length > 30 ? '...' : ''}`, 'success');
        else vncLog(`Clipboard failed: ${data.error}`, 'error');
    } catch (err) { vncLog(`Clipboard error: ${err.message}`, 'error'); }
}

async function vmClipboardGet() {
    try {
        const res = await apiFetch('/api/vm/clipboard/get?name=praxis-vm');
        const data = await res.json();
        if (data.success && data.content) {
            await navigator.clipboard.writeText(data.content);
            vncLog(`Clipboard copied to host: ${data.content.substring(0, 30)}${data.content.length > 30 ? '...' : ''}`, 'success');
        } else {
            vncLog(`Clipboard read failed: ${data.error || 'empty'}`, 'error');
        }
    } catch (err) { vncLog(`Clipboard error: ${err.message}`, 'error'); }
}

// VNC
async function connectVNC(vmName) {
    const placeholder = document.getElementById('vm-vnc-placeholder');
    const screen = document.getElementById('vm-vnc-screen');
    const log = document.getElementById('vm-vnc-log');
    if (log) log.innerHTML = '';
    if (!screen) return;

    try {
        vncLog(`Starting VNC to ${vmName}`, 'info');
        if (!vncModule) { vncModule = await import('/static/novnc/core/rfb.js'); vncLog('noVNC loaded', 'success'); }
        const RFB = vncModule.default || vncModule.RFB;
        if (vncRfb) { vncRfb.disconnect(); vncRfb = null; }
        const wsUrl = `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/websockify?vm=${encodeURIComponent(vmName)}`;

        placeholder.textContent = 'Connecting to VNC...';
        placeholder.style.display = 'block';
        screen.innerHTML = '';

        vncRfb = new RFB(screen, wsUrl, { credentials: { password: '' }, shared: true, wsProtocols: ['binary'] });
        vncRfb.addEventListener('connect', () => {
            vncLog('VNC connected!', 'success');
            placeholder.style.display = 'none';
            vncConnectedVm = vmName;
        });
        vncRfb.addEventListener('disconnect', e => {
            vncLog(`Disconnected: ${e.detail?.reason || 'unknown'}`, e.detail?.clean ? 'warn' : 'error');
            placeholder.style.display = 'block';
            placeholder.textContent = 'VNC disconnected. Click Refresh to reconnect.';
            vncConnectedVm = null;
        });
        vncRfb.scaleViewport = true;
        vncRfb.resizeSession = false;
    } catch (err) { vncLog(`VNC error: ${err.message}`, 'error'); }
}

function disconnectVNC() {
    if (vncRfb) { vncRfb.disconnect(); vncRfb = null; }
    const placeholder = document.getElementById('vm-vnc-placeholder');
    const screen = document.getElementById('vm-vnc-screen');
    if (placeholder) { placeholder.style.display = 'block'; placeholder.textContent = 'VM not running. Click "Start VM" to begin.'; }
    if (screen) screen.innerHTML = '';
    vncConnectedVm = null;
}

// Activity Log
async function loadVMActivity() {
    const container = document.getElementById('vm-activity-log');
    try {
        const res = await apiGet('/api/vm/activity?limit=100');
        if (!res.ok) return;
        const data = await res.json();
        const activities = data.activities || [];
        if (activities.length === 0) {
            container.innerHTML = '<div style="color:var(--text-secondary)">No VM activity yet.</div>'; return;
        }
        container.innerHTML = activities.map(a => {
            const time = a.created_at ? new Date(a.created_at).toLocaleTimeString() : '';
            let color = 'var(--text-primary)'; let icon = '[TOOL]';
            const name = a.action || '';
            if (name === 'vm_shell') { color = '#00D9FF'; icon = '[SHELL]'; }
            else if (name === 'vm_keys') { color = '#FFD600'; icon = '[KEYS]'; }
            else if (name === 'vm_mouse') { color = '#FF9800'; icon = '[MOUSE]'; }
            else if (name === 'vm_screenshot') { color = '#6C63FF'; icon = '[SCREEN]'; }
            else if (name === 'vm_start') { color = '#00E676'; icon = '[START]'; }
            else if (name === 'vm_stop') { color = '#FF5252'; icon = '[STOP]'; }
            else if (name === 'vm_file_transfer') { color = '#E040FB'; icon = '[FILE]'; }
            else if (name === 'vm_snapshot') { color = '#7C4DFF'; icon = '[SNAP]'; }
            else if (name === 'vm_shared_folder') { color = '#00BCD4'; icon = '[SHARE]'; }
            else if (name === 'execute_terminal') { color = '#4CAF50'; icon = '[TERM]'; }
            else if (name === 'write_file') { color = '#2196F3'; icon = '[WRITE]'; }
            else if (name === 'edit_file') { color = '#2196F3'; icon = '[EDIT]'; }
            else if (name === 'read_file') { color = '#90CAF9'; icon = '[READ]'; }
            else if (name.startsWith('agent_')) { color = '#FFA726'; icon = '[AGENT]'; }
            else if (name.startsWith('discord_')) { color = '#7289DA'; icon = '[DISCORD]'; }
            else if (name.startsWith('learn_')) { color = '#CE93D8'; icon = '[LEARN]'; }
            else if (name.startsWith('llm_')) { color = '#FF5252'; icon = '[LLM]'; }

            let inputSummary = '';
            try {
                const inp = JSON.parse(a.input || '{}');
                if (inp.command) inputSummary = inp.command.substring(0, 80);
                else if (inp.keys) inputSummary = `"${inp.keys}"`;
                else if (inp.path) inputSummary = inp.path;
                else if (inp.message) inputSummary = inp.message.substring(0, 60);
                else if (inp.action) inputSummary = inp.action;
                else inputSummary = (a.input || '').substring(0, 80);
            } catch { inputSummary = (a.input || '').substring(0, 80); }
            const outputPreview = (a.output || '').substring(0, 120).replace(/\n/g, ' ');
            const vmTag = a.vm_id ? ` [${a.vm_id}]` : '';

            return `<div style="margin-bottom:0.3rem;padding:0.2rem 0;border-bottom:1px solid rgba(255,255,255,0.05)">
                <span style="color:var(--text-secondary);font-size:0.75rem">${time}</span>
                <span style="color:${color};font-weight:600;font-size:0.8rem">${icon}${vmTag} ${escapeHtml(name)}</span>
                <span style="color:var(--text-primary);font-size:0.8rem"> ${escapeHtml(inputSummary)}</span>
                ${outputPreview ? `<div style="color:var(--text-secondary);font-size:0.75rem;padding-left:1rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml(outputPreview)}</div>` : ''}
            </div>`;
        }).join('');
    } catch (err) { console.error('Activity load error:', err); }
}

// ═══ Helpers ════════════════════════════════════════════════════════════════

function escapeHtml(str) { const d = document.createElement('div'); d.textContent = str; return d.innerHTML; }
function errorHtml(msg) { return `<div class="data-item"><span style="color:var(--error)">${escapeHtml(msg)}</span></div>`; }

// ═══ Markdown Rendering ═══════════════════════════════════════════════════════
//
// Streaming-aware markdown renderer. Key design points:
//
//   1. SAFE: every piece of user/LLM content is HTML-escaped before any
//      transformation runs. Inline HTML in the input is rendered as text.
//   2. STREAMING-FRIENDLY: open delimiters (unclosed ```, **, *, `) do not
//      leave dangling regex artifacts. We pre-process the buffer so an open
//      delimiter renders sensibly while waiting for its close, instead of
//      flickering "raw stars" until the close arrives.
//   3. RICH: handles fenced code blocks (with optional language), inline
//      code, **bold**, *italic*, ~~strikethrough~~, headings, links
//      [text](url), block quotes, ordered + unordered lists (including
//      nested), horizontal rules, [THINK]…[/THINK] reasoning blocks.
//   4. FAST ENOUGH: the per-token cost is dominated by `escapeHtml` (one
//      DOM round-trip) and a handful of linear regex passes. The streaming
//      handler still re-renders the whole buffer per char event; if that
//      ever becomes a bottleneck we can switch to an incremental approach.

function renderMarkdown(text) {
    if (!text) return '';

    // ── 1. Extract reasoning blocks BEFORE escaping so we can render the
    //      content as a styled think-block while still safely escaping it. ──
    const thinks = [];
    let src = text.replace(/\[THINK\]([\s\S]*?)(?:\[\/THINK\]|$)/g, (m, content, offset, str) => {
        // Match closed [...]/[/THINK] as well as open-and-still-streaming
        // ([THINK]... at the end). For an open think we emit the partial.
        const closed = m.endsWith('[/THINK]');
        thinks.push({ content, closed });
        return `\u0000T${thinks.length - 1}\u0000`;
    });

    // ── 2. Extract fenced code blocks BEFORE escaping. We allow open
    //      (still-streaming) fences: a ``` with no closing ``` yet. ──
    const codeBlocks = [];
    src = src.replace(/```([a-zA-Z0-9_+\-.#]*)\n?([\s\S]*?)(?:```|$)/g, (m, lang, body) => {
        const closed = m.endsWith('```') && m.length > 3;
        codeBlocks.push({ lang: (lang || '').trim().toLowerCase(), body, closed });
        return `\u0000C${codeBlocks.length - 1}\u0000`;
    });

    // ── 3. Extract inline code spans BEFORE escaping. Open backticks
    //      stay as plain text until they close. ──
    const inlineCodes = [];
    src = src.replace(/`([^`\n]+)`/g, (m, body) => {
        inlineCodes.push(body);
        return `\u0000I${inlineCodes.length - 1}\u0000`;
    });

    // ── 4. Now safe to escape everything else. ──
    let h = escapeHtml(src);

    // ── 5. Block-level transforms (operate on whole lines). ──
    h = h
        .replace(/^######\s+(.*)$/gm, '<h6>$1</h6>')
        .replace(/^#####\s+(.*)$/gm, '<h5>$1</h5>')
        .replace(/^####\s+(.*)$/gm, '<h4>$1</h4>')
        .replace(/^###\s+(.*)$/gm, '<h3>$1</h3>')
        .replace(/^##\s+(.*)$/gm, '<h2>$1</h2>')
        .replace(/^#\s+(.*)$/gm, '<h1>$1</h1>')
        .replace(/^\s*[-*_]{3,}\s*$/gm, '<hr>')
        .replace(/^&gt;\s?(.*)$/gm, '<blockquote>$1</blockquote>')
        // Lists: capture optional indentation so we can reconstruct nesting.
        .replace(/^([ \t]*)([-*+])\s+(.*)$/gm,
            (_m, indent, _b, body) => `\u0000UL${indent.length}\u0000${body}`)
        .replace(/^([ \t]*)(\d+)\.\s+(.*)$/gm,
            (_m, indent, _n, body) => `\u0000OL${indent.length}\u0000${body}`);

    h = wrapLists(h);

    // ── 6. Inline transforms. We use protectors around emphasis to avoid
    //      the "open star never closes" flicker during streaming.        ──
    // Bold: **...** (require a closing pair; partial open stays as text).
    h = h.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>');
    // Italic: *...* but not at start of bold (** already consumed). Also _..._
    h = h.replace(/(^|[\s\W])\*([^*\n]+)\*(?=[\s\W]|$)/g, '$1<em>$2</em>');
    h = h.replace(/(^|[\s\W])_([^_\n]+)_(?=[\s\W]|$)/g, '$1<em>$2</em>');
    // Strikethrough
    h = h.replace(/~~([^~\n]+)~~/g, '<del>$1</del>');
    // Links [text](url) — only http/https/mailto/anchor URLs to be safe.
    h = h.replace(/\[([^\]]+)\]\(((?:https?:\/\/|mailto:|#)[^\s)]+)\)/g, (_m, label, url) => {
        const safeUrl = url.replace(/"/g, '&quot;');
        return `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${label}</a>`;
    });
    // Bare URLs (auto-link).
    h = h.replace(/(^|[\s])(https?:\/\/[^\s<]+)/g, (_m, pre, url) => {
        const cleanUrl = url.replace(/[.,;:!?)]+$/, '');
        const trail = url.slice(cleanUrl.length);
        const safeUrl = cleanUrl.replace(/"/g, '&quot;');
        return `${pre}<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${cleanUrl}</a>${trail}`;
    });

    // ── 7. Paragraph breaks. Convert remaining \n to <br> but collapse
    //      consecutive newlines into paragraph gaps for breathing room. ──
    h = h.replace(/\n{2,}/g, '<br><br>').replace(/\n/g, '<br>');

    // ── 8. Re-insert protected blocks. ──
    h = h.replace(/\u0000C(\d+)\u0000/g, (_m, idx) => {
        const cb = codeBlocks[parseInt(idx, 10)];
        const lang = cb.lang ? ` data-lang="${escapeHtml(cb.lang)}"` : '';
        const cls = cb.closed ? 'code-block' : 'code-block code-block-open';
        const langLabel = cb.lang ? `<span class="code-lang">${escapeHtml(cb.lang)}</span>` : '';
        return `<pre class="${cls}"${lang}>${langLabel}<code>${escapeHtml(cb.body)}</code></pre>`;
    });
    h = h.replace(/\u0000I(\d+)\u0000/g, (_m, idx) => {
        return `<code>${escapeHtml(inlineCodes[parseInt(idx, 10)])}</code>`;
    });
    h = h.replace(/\u0000T(\d+)\u0000/g, (_m, idx) => {
        const t = thinks[parseInt(idx, 10)];
        const cls = t.closed ? 'think-block' : 'think-block think-block-open';
        return `<div class="${cls}"><span class="think-label">`
            + `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"></circle><path d="M12 16v-4"></path><path d="M12 8h.01"></path></svg>`
            + ` Thinking${t.closed ? '' : '…'}</span>`
            + `<div class="think-content">${escapeHtml(t.content)}</div></div>`;
    });

    return h;
}

// Helper for renderMarkdown: turn the marker-encoded list lines from step 5
// back into a properly-nested <ul>/<ol> tree. We use a simple stack keyed by
// indentation depth (in spaces) and tag (UL/OL).
function wrapLists(text) {
    const lines = text.split('\n');
    const out = [];
    const stack = []; // entries: { tag, depth }

    const closeUntil = (predicate) => {
        while (stack.length && !predicate(stack[stack.length - 1])) {
            const top = stack.pop();
            out.push(`</${top.tag.toLowerCase()}>`);
        }
    };

    for (const line of lines) {
        const m = line.match(/^\u0000(UL|OL)(\d+)\u0000(.*)$/);
        if (m) {
            const tag = m[1];
            const depth = parseInt(m[2], 10);
            const body = m[3];

            // Close any deeper or differently-tagged lists at this level.
            closeUntil(top => top.depth < depth || (top.depth === depth && top.tag === tag));
            // Open a new list if needed.
            if (!stack.length || stack[stack.length - 1].depth < depth || stack[stack.length - 1].tag !== tag) {
                out.push(`<${tag.toLowerCase()}>`);
                stack.push({ tag, depth });
            }
            out.push(`<li>${body}</li>`);
        } else {
            // Non-list line: close all open lists.
            closeUntil(() => false);
            out.push(line);
        }
    }
    closeUntil(() => false);
    return out.join('\n');
}

// ═══ Modal ════════════════════════════════════════════════════════════════

function showModal(title, content) {
    closeModal();
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.id = 'modal-overlay';
    overlay.innerHTML = `<div class="modal"><h3>${title}</h3>${content}<button class="btn btn-secondary" style="margin-top:1rem" onclick="closeModal()">Close</button></div>`;
    document.body.appendChild(overlay);
}

let pendingAvatarFile = null;

function showAvatarModal(which) {
    closeModal();
    pendingAvatarFile = null;
    const isBot = which === 'bot';
    const imgUrl = isBot ? `/api/avatar/bot?t=${Date.now()}` : `/api/avatar/${chatUserId}?t=${Date.now()}`;
    const name = isBot ? (chatBotName || 'Praxis') : (chatUserId || 'User');
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.id = 'modal-overlay';
    overlay.innerHTML = `
        <div class="modal avatar-modal">
            <h3>${escapeHtml(name)} Avatar</h3>
            <img id="avatar-current-preview" class="avatar-preview" src="${imgUrl}" alt="" onerror="this.src='/logo.png'">
            <img id="avatar-new-preview" class="avatar-preview" src="" alt="New avatar" style="display:none;margin-top:0.5rem">
            <input type="file" id="avatar-upload-input" accept="image/*" style="display:none" onchange="handleAvatarFileSelect(this, '${which}')">
            <button class="btn btn-primary" style="margin-top:1rem" onclick="document.getElementById('avatar-upload-input').click()">Upload Photo</button>
            <button id="avatar-confirm-btn" class="btn btn-primary" style="margin-top:0.5rem;display:none;background:var(--success)" onclick="confirmAvatarUpload('${which}')">Confirm Upload</button>
            <button class="btn btn-secondary" style="margin-top:0.5rem" onclick="closeModal()">Close</button>
        </div>`;
    document.body.appendChild(overlay);
}

function handleAvatarFileSelect(input, which) {
    const file = input.files[0];
    if (!file) return;
    pendingAvatarFile = file;
    const reader = new FileReader();
    reader.onload = (e) => {
        const newPreview = document.getElementById('avatar-new-preview');
        newPreview.src = e.target.result;
        newPreview.style.display = 'block';
        document.getElementById('avatar-confirm-btn').style.display = '';
    };
    reader.readAsDataURL(file);
}

async function confirmAvatarUpload(which) {
    if (!pendingAvatarFile) return;
    const isBot = which === 'bot';
    const form = new FormData();
    form.append(isBot ? 'bot' : chatUserId, pendingAvatarFile);
    try {
        const res = await fetch('/api/upload-avatar', {
            method: 'POST',
            headers: { 'Authorization': `Bearer ${authToken}` },
            body: form
        });
        const data = await res.json();
        if (data.success) {
            if (isBot) {
                const img = document.getElementById('chat-bot-avatar');
                img.src = data.url + '?t=' + Date.now();
            } else {
                loadAvatar();
            }
            closeModal();
        } else {
            alert(data.error || 'Upload failed');
        }
    } catch (err) { alert('Upload error: ' + err.message); }
}

function closeModal() { const o = document.getElementById('modal-overlay'); if (o) o.remove(); }

function toggleTheme() {
    const current = document.body.getAttribute('data-theme') || 'dark';
    const next = current === 'light' ? 'dark' : 'light';
    document.body.setAttribute('data-theme', next);
    localStorage.setItem('praxis-theme', next);
    const btn = document.getElementById('theme-toggle');
    if (btn) btn.textContent = next === 'light' ? '☀️ Light' : '🌙 Dark';
}

// ═══ Events ════════════════════════════════════════════════════════════════

document.addEventListener('DOMContentLoaded', () => {
    const savedTheme = localStorage.getItem('praxis-theme') || 'dark';
    document.body.setAttribute('data-theme', savedTheme);
    const themeBtn = document.getElementById('theme-toggle');
    if (themeBtn) themeBtn.textContent = savedTheme === 'light' ? '☀️ Light' : '🌙 Dark';

    if (authToken) validateTokenAndLoad();

    document.getElementById('login-form').addEventListener('submit', async e => {
        e.preventDefault();
        const pwd = document.getElementById('password').value;
        const errEl = document.getElementById('login-error');
        try { await login(pwd); errEl.classList.add('hidden'); showScreen('dashboard-screen'); loadOverview(); initChatTab(); }
        catch (err) { errEl.textContent = err.message; errEl.classList.remove('hidden'); }
    });

    document.getElementById('logout-btn').addEventListener('click', logout);
    document.querySelectorAll('.nav-links li').forEach(li => {
        li.addEventListener('click', () => showTab(li.dataset.tab));
    });
    document.getElementById('load-messages-btn').addEventListener('click', loadMessages);
    document.getElementById('load-memory-btn').addEventListener('click', loadMemory);
});

/* ── Web chat speech: mic (STT) + reply TTS toggle ─────────────────────────── */
let chatMicStream = null;
let chatMediaRecorder = null;
let chatMicChunks = [];
let chatTtsOn = false;
let chatTtsAudio = null;

async function chatToggleMic() {
  const btn = document.getElementById('chat-mic-btn');
  if (chatMediaRecorder && chatMediaRecorder.state === 'recording') {
    chatMediaRecorder.stop();
    return;
  }
  try {
    chatMicStream = await navigator.mediaDevices.getUserMedia({ audio: true });
    chatMicChunks = [];
    chatMediaRecorder = new MediaRecorder(chatMicStream);
    chatMediaRecorder.ondataavailable = (e) => { if (e.data.size) chatMicChunks.push(e.data); };
    chatMediaRecorder.onstop = async () => {
      chatMicStream.getTracks().forEach(t => t.stop());
      chatMicStream = null;
      btn.classList.remove('recording');
      btn.textContent = '🎤';
      const blob = new Blob(chatMicChunks, { type: 'audio/webm' });
      if (blob.size < 2000) { addChatMessage('system', '(Recording too short)'); return; }
      addChatMessage('system', '⏳ Transcribing...');
      try {
        const fd = new FormData();
        fd.append('audio', blob, 'speech.webm');
        const res = await apiFetch(`/api/stt?user_id=${encodeURIComponent(chatUserId)}`, { method: 'POST', body: fd });
        const raw = await res.text();
        let data = null;
        try { data = JSON.parse(raw); } catch (_) { /* non-JSON response */ }
        if (!data) {
          addChatMessage('feedback', `STT failed: server returned ${res.status} ${raw.slice(0, 120)}`);
          return;
        }
        if (data.error) { addChatMessage('feedback', 'STT failed: ' + data.error); return; }
        const ta = document.getElementById('chat-input');
        ta.value = (ta.value ? ta.value + ' ' : '') + (data.text || '');
        if (data.low_confidence) addChatMessage('system', '⚠️ Low speech confidence – check the text.');
        ta.focus();
      } catch (err) { addChatMessage('feedback', 'STT failed: ' + err.message); }
    };
    chatMediaRecorder.start();
    btn.classList.add('recording');
    btn.textContent = '⏺';
    addChatMessage('system', '🎙 Recording... click again to send.');
  } catch (err) {
    addChatMessage('feedback', 'Microphone unavailable: ' + err.message);
  }
}

// Browser autoplay policy: audio started outside a user gesture is blocked.
// Unlock once per session on the first real user interaction (clicking the
// TTS button) by playing a tiny silent clip; later SSE-triggered playback
// is then allowed.
let chatTtsUnlocked = false;
function chatTtsUnlock() {
  if (chatTtsUnlocked) return;
  try {
    const a = new Audio('data:audio/mpeg;base64,SUQzBAAAAAAAI1RTU0UAAAAPAAADTGF2ZjYwLjE2LjEwMQAAAAAAAAAAAAAAAAAA//uwxAAAACYzIEQAAACAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAEAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAACAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAAIAAAAAAAAAAAAAAAAAAAAAAAD/+7DEAAA-');
    a.volume = 0;
    const p = a.play();
    if (p) p.then(() => { chatTtsUnlocked = true; console.log('[TTS] audio unlocked'); }).catch(() => {});
  } catch (_) { /* unlock is best-effort */ }
}

async function chatToggleTts() {
  const btn = document.getElementById('chat-tts-btn');
  chatTtsOn = !chatTtsOn;
  btn.setAttribute('aria-pressed', chatTtsOn ? 'true' : 'false');
  btn.classList.toggle('tts-on', chatTtsOn);
  btn.textContent = chatTtsOn ? '🔊' : '🔉';
  if (chatTtsOn) chatTtsUnlock(); // user gesture: unlock autoplay for later SSE playback
  try {
    await apiFetch(`/api/contexts/${encodeURIComponent(chatUserId)}`, {
      method: 'PUT',
      body: JSON.stringify({ settings: { web_chat_tts: chatTtsOn } })
    });
    addChatMessage('system', chatTtsOn ? '🔊 Replies will be read aloud.' : '🔉 TTS off.');
  } catch (err) {
    addChatMessage('feedback', 'Could not save TTS setting: ' + err.message);
  }
}

function chatPlayTts(dataUrl) {
  if (!chatTtsOn) {
    console.warn('[TTS] chat_tts received but TTS switch is OFF — not playing');
    return; // respect the switch
  }
  console.log('[TTS] playing audio,', Math.round(dataUrl.length / 1024), 'KB payload');
  if (chatTtsAudio) { chatTtsAudio.pause(); }
  chatTtsAudio = new Audio(dataUrl);
  chatTtsAudio.onended = () => chatTtsActive(false);
  chatTtsAudio.onerror = (e) => { console.error('[TTS] audio error', e); chatTtsActive(false); };
  chatTtsActive(true);
  chatTtsAudio.play().catch((err) => {
    // Autoplay policy blocked playback (no user gesture yet). Show a
    // click-to-play state on the indicator instead of failing silently.
    console.warn('[TTS] play blocked by autoplay policy', err);
    const chip = document.getElementById('chat-tts-indicator');
    if (chip) {
      chip.style.display = '';
      chip.innerHTML = '🔊 Voice ready – <span class="tts-stop-hint">▶ click to play</span>';
      chip.onclick = () => {
        chatTtsUnlock();
        chip.innerHTML = '🔊 Praxis is speaking… <span class="tts-stop-hint">■ stop</span>';
        chip.onclick = () => chatTtsActive(false);
        chatTtsAudio.play().catch((e2) => { console.error('[TTS] manual play failed', e2); chatTtsActive(false); });
      };
    }
  });
}

// Visible "Praxis is speaking" indicator in the chat header. Clicking it
// stops playback immediately.
function chatTtsActive(on) {
  const chip = document.getElementById('chat-tts-indicator');
  if (!chip) return;
  chip.style.display = on ? '' : 'none';
  const btn = document.getElementById('chat-tts-btn');
  if (btn) btn.classList.toggle('tts-speaking', on);
  if (!on && chatTtsAudio) { try { chatTtsAudio.pause(); } catch (_) {} chatTtsAudio = null; }
}
