const API_BASE = '';
let authToken = localStorage.getItem('praxis_token');
let chatPollInterval = null;
let chatEventSource = null;
let chatUserId = 'default';
let chatSessionId = 'default';
let chatAttachments = [];
let vmRefreshInterval = null;
let chatSeenIds = new Set();
let chatLastResponse = '';
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
    const res = await fetch(`${API_BASE}${path}`, {
        ...options,
        headers: { ...getAuthHeaders(), ...options.headers }
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
    if (tabId === 'chat') {
        content.classList.add('chat-expanded');
        sidebar.classList.add('collapsed');
        if (sidebarCollapsed) sidebar.classList.add('collapsed');
    } else {
        content.classList.remove('chat-expanded');
        if (!sidebarCollapsed) sidebar.classList.remove('collapsed');
    }
    loadTabData(tabId);
}

function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    const sidebar = document.getElementById('sidebar');
    sidebar.classList.toggle('collapsed', sidebarCollapsed);
    const isChat = document.getElementById('tab-chat').classList.contains('active');
    if (!isChat) {
        document.querySelector('.content').classList.toggle('chat-expanded', sidebarCollapsed);
    }
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

async function initChatTab() {
    const input = document.getElementById('chat-input');
    input.addEventListener('input', () => {
        input.style.height = 'auto';
        input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    });
    loadChatSessions();
    await loadChatUserInfo();
    // Scroll to bottom when chat tab opens
    const container = document.getElementById('chat-messages');
    if (container) container.scrollTop = container.scrollHeight;
}

async function loadChatUserInfo() {
    try {
        const res = await apiGet('/api/contexts/' + encodeURIComponent(chatUserId));
        const data = await res.json();
        if (data.context) {
            chatBotName = data.context.settings?.agent_name || 'Praxis';
        }
    } catch {}
    document.getElementById('chat-user-name-label').textContent = chatUserId;
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
    img.src = `/api/avatar/${chatUserId}?t=${Date.now()}`;
    img.style.display = '';
    img.onerror = () => { img.src = '/logo.svg'; };
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
    // Only auto-scroll if user is already near the bottom (within 80px)
    const nearBottom = (container.scrollHeight - container.scrollTop - container.clientHeight) < 80;
    if (nearBottom) {
        container.scrollTop = container.scrollHeight;
    }
}

function chatKeyDown(e) {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); chatSendMessage(); }
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
            <img class="msg-avatar" src="/logo.svg" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.svg'">
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

    es.addEventListener('feedback', (e) => {
        console.log('[SSE] feedback event received');
        try {
            const d = JSON.parse(e.data);
            if (d.data) addChatMessage('feedback', d.data);
        } catch (err) { console.error('[SSE feedback error]', err); }
    });
    es.addEventListener('char', (e) => {
        const t0 = performance.now();
        try {
            const d = JSON.parse(e.data);
            if (d.data) {
                if (!streamMsg) {
                    console.log('[SSE] first char received, creating stream message element');
                    streamMsg = document.createElement('div');
                    streamMsg.className = 'chat-msg assistant';
                    const botAvatar = `/api/avatar/bot?t=${Date.now()}`;
                    const botName = chatBotName || 'Praxis';
                    streamMsg.innerHTML = `<div class="msg-row">
                        <div class="msg-col">
                            <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.svg'" onclick="showAvatarModal('bot')">
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
                    // Ultra-fast path: raw textContent while streaming, no markdown
                    contentEl.textContent = streamBuffer;
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
}

async function pollChatMessages() {
    try {
        const res = await apiGet(`/api/messages/${encodeURIComponent(chatUserId)}`);
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
    clearChatMessages();
    console.log('[HISTORY] loading history for session', chatSessionId);
    try {
        const res = await apiGet(`/api/messages/${encodeURIComponent(chatUserId)}`);
        const data = await res.json();
        console.log('[HISTORY] got', (data.messages || []).length, 'messages');
        if (data.messages && data.messages.length > 0) {
            for (const m of data.messages) {
                const id = m.role + ':' + (m.content || '').slice(0, 80);
                chatSeenIds.add(id);
                renderChatMessage(m);
            }
        }
    } catch (err) { console.error('[HISTORY] load error:', err); }
}

function renderChatMessage(m) {
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
    const userAvatar = `/api/avatar/${chatUserId}?t=${Date.now()}`;
    const botName = chatBotName || 'Praxis';
    const userName = chatUserId || 'User';

    if (type === 'user') {
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${userAvatar}" alt="" onerror="this.src='/logo.svg'" onclick="showAvatarModal('user')">
                <span class="msg-label">${escapeHtml(userName)}</span>
            </div>
            <div class="msg-content">${escapeHtml(content)}</div>
        </div>`;
    } else if (type === 'assistant') {
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-purple)" onerror="this.src='/logo.svg'" onclick="showAvatarModal('bot')">
                <span class="msg-label">${escapeHtml(botName)}</span>
            </div>
            <div class="msg-content markdown">${renderMarkdown(content)}</div>
        </div>`;
    } else if (type === 'tool') {
        const args = extra || '';
        div.innerHTML = `<div class="msg-row">
            <div class="msg-col">
                <img class="msg-avatar" src="${botAvatar}" alt="" style="border-color:var(--accent-cyan)" onerror="this.src='/logo.svg'" onclick="showAvatarModal('bot')">
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

function clearChatMessages() {
    const container = document.getElementById('chat-messages');
    container.innerHTML = '<div class="chat-welcome">Start an agent to begin chatting. Your messages appear here with tool calls visible inline.</div>';
    chatSeenIds.clear();
    chatLastResponse = '';
}

// ═══ Chat Sessions ══════════════════════════════════════════════════════════════

function loadChatSessions() {
    const stored = localStorage.getItem('praxis_chat_sessions');
    if (stored) {
        try { chatSessions = JSON.parse(stored); } catch { chatSessions = []; }
    }
    if (chatSessions.length === 0) {
        chatSessions = [{ id: 'default', name: 'Default' }];
        saveChatSessions();
    }
    renderChatSessionList();
}

function saveChatSessions() {
    localStorage.setItem('praxis_chat_sessions', JSON.stringify(chatSessions));
}

function renderChatSessionList() {
    const list = document.getElementById('chat-session-list');
    if (!list) return;
    list.innerHTML = chatSessions.map(s => `
        <div class="chat-session ${s.id === chatSessionId ? 'active' : ''}" onclick="switchChatSession('${escapeHtml(s.id)}')">
            <span class="session-name" ondblclick="event.stopPropagation();startRenameSession('${escapeHtml(s.id)}', this)">${escapeHtml(s.name)}</span>
            ${chatSessions.length > 1 ? `<span class="session-del" onclick="event.stopPropagation();deleteChatSession('${escapeHtml(s.id)}')">×</span>` : ''}
        </div>
    `).join('');
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
    const id = 'session-' + Math.random().toString(36).slice(2, 8);
    const name = `Session ${chatSessions.length + 1}`;
    console.log('[SESSION] creating new session:', id, name);
    chatSessions.push({ id, name });
    saveChatSessions();
    await switchChatSession(id);
    // Persist session on backend
    try {
        await apiPost('/api/contexts/' + encodeURIComponent(chatUserId), { session_id: id });
        console.log('[SESSION] backend persistence done for', id);
    } catch (err) { console.error('[SESSION] backend persistence failed:', err); }
    renderChatSessionList();
}

async function switchChatSession(id) {
    console.log('[SESSION] switching to session:', id, '(from', chatSessionId, ')');
    stopChatPolling();
    chatSessionId = id;
    clearChatMessages();
    // Update backend context session
    try {
        await apiPost('/api/contexts/' + encodeURIComponent(chatUserId), { session_id: id });
        console.log('[SESSION] context updated on backend for', id);
    } catch (err) { console.error('[SESSION] context update failed:', err); }
    renderChatSessionList();
    await loadChatHistory();
    await loadChatStatus();
    startChatPolling();
}

function deleteChatSession(id) {
    if (chatSessions.length <= 1) return;
    chatSessions = chatSessions.filter(s => s.id !== id);
    saveChatSessions();
    if (chatSessionId === id) {
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
        form.append(chatUserId, file);
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

// ═══ CL Status ════════════════════════════════════════════════════════════════

async function loadCLStatus() {
    try {
        const res = await apiGet(`/api/cl/${encodeURIComponent(chatUserId)}`);
        const data = await res.json();
        const bar = document.getElementById('chat-cl-status');
        if (data.cl_file || data.active_state || (data.sm_data && Object.keys(data.sm_data).length > 0)) {
            bar.style.display = 'flex';
            document.getElementById('cl-status-file').textContent = data.cl_file ? `SM: ${data.cl_file}` : '-';
            document.getElementById('cl-status-state').textContent = data.active_state ? `State: ${data.active_state}` : '-';
            const temps = data.active_templates || [];
            document.getElementById('cl-status-temps').textContent = temps.length
                ? `Templates: ${temps.join(', ')}`
                : '-';
            document.getElementById('cl-status-temps').className = temps.length ? 'cl-badge template' : 'cl-badge';
            // Show SM variables
            const vars = data.sm_data || {};
            const varKeys = Object.keys(vars).filter(k => k !== 'active_state' && k !== 'active_templates');
            const varsEl = document.getElementById('cl-status-vars');
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
            'cl_file', 'active_state', 'active_templates', 'llm_turn', 'compaction_summary', 'download'
        ]);

        const filteredSettings = ctx.settings ? Object.entries(ctx.settings).filter(([k]) => !redundantKeys.has(k)) : [];
        const settingsHtml = filteredSettings.length > 0
            ? filteredSettings.map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v ?? ''))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">No settings</span></div>';

        const customDataHtml = ctx.custom_data && Object.keys(ctx.custom_data).length > 0
            ? Object.entries(ctx.custom_data).map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const clDataHtml = ctx.cl_data && Object.keys(ctx.cl_data).length > 0
            ? Object.entries(ctx.cl_data).map(([k, v]) => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span></div>`).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const uid = escapeHtml(userId);
        showModal('Context: ' + uid, `
            <div id="context-view-mode">
                <div class="data-list" style="margin-bottom:1rem">
                    <div class="data-item"><span class="name">User ID</span><span class="meta">${escapeHtml(ctx.user_id || '')}</span></div>
                    <div class="data-item"><span class="name">Turn</span><span class="meta">${ctx.turn ?? 0}</span></div>
                    <div class="data-item"><span class="name">Mode</span><span class="meta">${escapeHtml(ctx.mode || '')}</span></div>
                    <div class="data-item"><span class="name">SM File</span><span class="meta">${escapeHtml(ctx.cl_file || '-')}</span></div>
                    <div class="data-item"><span class="name">Active State</span><span class="meta">${escapeHtml(ctx.active_state || '-')}</span></div>
                    <div class="data-item"><span class="name">Active Templates</span><span class="meta">${(ctx.active_templates || []).join(', ') || '-'}</span></div>
                </div>
                <h3>Settings</h3><div class="data-list" style="margin-bottom:1rem;max-height:200px;overflow-y:auto">${settingsHtml}</div>
                <h3>Custom Data</h3><div class="data-list" style="margin-bottom:1rem">${customDataHtml}</div>
                <h3>SM Data</h3><div class="data-list" style="margin-bottom:1rem">${clDataHtml}</div>
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
        list.innerHTML = data.tools.map(t => `<div class="data-item">
            <div><span class="name">${escapeHtml(t.name)}</span><span class="meta">${escapeHtml(t.description || '')}</span></div>
            <div class="toggle ${t.is_enabled ? 'active' : ''}" onclick="toggleTool('${escapeHtml(t.name)}', ${!t.is_enabled})"></div>
        </div>`).join('');
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
        const knownKeys = ['discord_bot_token', 'openai_api_key', 'anthropic_api_key', 'minimax_api_key',
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

function renderMarkdown(text) {
    if (!text) return '';
    // Extract think blocks first before escaping
    const thinks = [];
    let h = text.replace(/\[THINK\]([\s\S]*?)\[\/THINK\]/g, (match, content) => {
        thinks.push(content);
        return `__THINK_${thinks.length - 1}__`;
    });
    // Escape remaining text
    h = escapeHtml(h);
    // Markdown transforms
    h = h
        .replace(/^#### (.*$)/gim, '<h4>$1</h4>')
        .replace(/^### (.*$)/gim, '<h3>$1</h3>')
        .replace(/^## (.*$)/gim, '<h2>$1</h2>')
        .replace(/^# (.*$)/gim, '<h1>$1</h1>')
        .replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>')
        .replace(/\*(.*?)\*/g, '<em>$1</em>')
        .replace(/`([^`]+)`/g, '<code>$1</code>')
        .replace(/^\> (.*$)/gim, '<blockquote>$1</blockquote>')
        .replace(/^\- (.*$)/gim, '<li>$1</li>')
        .replace(/^\d+\. (.*$)/gim, '<li>$1</li>');
    h = h.replace(/(<li>.*?\u003c\/li>\n?)+/g, (m) => '<ul>' + m.replace(/\n/g, '') + '</ul>');
    h = h.replace(/\n/g, '<br>');
    // Re-insert think blocks as styled HTML
    thinks.forEach((content, i) => {
        h = h.replace(
            `__THINK_${i}__`,
            `<div class="think-block"><span class="think-label"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"></circle><path d="M12 16v-4"></path><path d="M12 8h.01"></path></svg> Thinking</span><div class="think-content">${escapeHtml(content)}</div></div>`
        );
    });
    return h;
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
            <img id="avatar-current-preview" class="avatar-preview" src="${imgUrl}" alt="" onerror="this.src='/logo.svg'">
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
