const API_BASE = '';
let authToken = localStorage.getItem('praxis_token');
let chatPollInterval = null;
let chatUserId = 'default';
let chatAttachments = [];
let vmRefreshInterval = null;

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

// ═══ Screen & Tab Management ═══════════════════════════════════════════════════

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
    loadTabData(tabId);
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

// ═══ Overview ═══════════════════════════════════════════════════════════════════

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
            : '<div class="data-item"><span class="name">No users. Pair a bot or create a context first.</span></div>';
    } catch (err) { console.error('Overview error:', err); }
}

function quickStartAgent(userId) {
    chatUserId = userId;
    document.getElementById('chat-user-name').textContent = userId;
    showTab('chat');
    setTimeout(() => chatStartAgent(), 300);
}

// ═══ Chat Tab ═══════════════════════════════════════════════════════════════════

function initChatTab() {
    document.getElementById('chat-user-name').textContent = chatUserId;
    // Auto-expand textarea
    const input = document.getElementById('chat-input');
    input.addEventListener('input', () => {
        input.style.height = 'auto';
        input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    });
}

async function loadChatStatus() {
    const isActive = await checkAgentActive(chatUserId);
    updateAgentUI(isActive);
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
    img.onerror = () => { img.style.display = 'none'; };
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
        const res = await apiFetch(`/api/agent/stop/${encodeURIComponent(chatUserId)}`, { method: 'POST' });
        await res.json();
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
    // Clear attachments UI
    chatAttachments = [];
    renderAttachments();

    const displayMsg = msg || '(attachments)';
    addChatMessage('user', displayMsg);
    startChatTimer(30);

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
        }
    } catch (err) {
        addChatMessage('feedback', 'Send failed: ' + err.message);
        stopChatTimer();
    }
}

function chatKeyDown(e) {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); chatSendMessage(); }
}

// ═══ Chat Polling (for responses & feedback) ════════════════════════════════════

function startChatPolling() {
    stopChatPolling();
    chatPollInterval = setInterval(pollChatMessages, 1500);
}

function stopChatPolling() {
    if (chatPollInterval) { clearInterval(chatPollInterval); chatPollInterval = null; }
}

async function pollChatMessages() {
    try {
        // Poll messages for this user
        const res = await apiGet(`/api/messages/${encodeURIComponent(chatUserId)}`);
        const data = await res.json();
        if (data.messages && data.messages.length > 0) {
            const last = data.messages[data.messages.length - 1];
            renderLastActivity(last);
        }

        // Poll CL status
        await loadCLStatus();
    } catch {}
}

let lastDisplayedMsgId = 0;
function renderLastActivity(msg) {
    // Simple dedup by content length
    const msgs = document.getElementById('chat-messages');
    const existing = msgs.querySelectorAll('.chat-msg');
    if (msg.content && msg.content !== lastDisplayedMsgId) {
        lastDisplayedMsgId = msg.content;
        if (msg.role === 'assistant' && msg.content && msg.content !== 'Hello') {
            addChatMessage('assistant', msg.content);
            stopChatTimer();
        }
        if (msg.tool_calls && msg.tool_calls.length > 0) {
            msg.tool_calls.forEach(tc => {
                addChatMessage('tool', `<span class="tool-name">${escapeHtml(tc.name)}</span> ${escapeHtml(tc.arguments)}`);
            });
        }
    }
}

// ═══ Chat Timer ═════════════════════════════════════════════════════════════════

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
        if (remaining <= 0) { stopChatTimer(); }
    }, 200);
}

function stopChatTimer() {
    if (chatTimerId) { clearInterval(chatTimerId); chatTimerId = null; }
    document.getElementById('chat-timer-bar').style.display = 'none';
}

// ═══ Chat Messages ══════════════════════════════════════════════════════════════

function addChatMessage(type, content) {
    const container = document.getElementById('chat-messages');
    // Remove welcome message
    const welcome = container.querySelector('.chat-welcome');
    if (welcome) welcome.remove();

    const div = document.createElement('div');
    div.className = `chat-msg ${type}`;
    div.innerHTML = content;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
}

// ═══ Attachments ════════════════════════════════════════════════════════════════

function renderAttachments() {
    const preview = document.getElementById('chat-attachments-preview');
    if (chatAttachments.length === 0) {
        preview.style.display = 'none';
        return;
    }
    preview.style.display = 'flex';
    preview.innerHTML = chatAttachments.map((a, i) => `
        <div class="chat-attachment">
            <span>${escapeHtml(a.split('/').pop())}</span>
            <span class="remove" onclick="removeAttachment(${i})">&#10005;</span>
        </div>
    `).join('');
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
        } else if (data.error) {
            addChatMessage('feedback', data.error);
        }
    } catch (err) { addChatMessage('feedback', 'Upload failed: ' + err.message); }
    document.getElementById('chat-file-input').value = '';
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

// ═══ CL Status ══════════════════════════════════════════════════════════════════

async function loadCLStatus() {
    try {
        const res = await apiGet(`/api/cl/${encodeURIComponent(chatUserId)}`);
        const data = await res.json();
        const bar = document.getElementById('chat-cl-status');
        if (data.cl_file || data.active_state) {
            bar.style.display = 'flex';
            document.getElementById('cl-status-file').textContent = data.cl_file ? `SM: ${data.cl_file}` : '-';
            document.getElementById('cl-status-state').textContent = data.active_state ? `State: ${data.active_state}` : '-';
            const temps = data.active_templates || [];
            document.getElementById('cl-status-temps').textContent = temps.length
                ? `Templates: ${temps.join(', ')}`
                : '-';
            document.getElementById('cl-status-temps').className = temps.length ? 'cl-badge template' : 'cl-badge';
        } else {
            bar.style.display = 'none';
        }
    } catch {}
}

// ═══ Options Box ════════════════════════════════════════════════════════════════

function showOptions(options) {
    const box = document.getElementById('chat-options-box');
    if (!options || options.length === 0) { box.style.display = 'none'; return; }
    box.style.display = 'flex';
    box.innerHTML = options.map((o, i) => {
        const label = typeof o === 'string' ? o : o.label;
        const emoji = ['1⃣','2⃣','3⃣','4⃣','5⃣','6⃣'][i] || '◾';
        return `<div class="chat-option" onclick="selectOption(${i})"><span class="emoji">${emoji}</span> ${escapeHtml(label)}</div>`;
    }).join('');
}

function selectOption(i) {
    document.getElementById('chat-options-box').style.display = 'none';
    const input = document.getElementById('chat-input');
    input.value = i.toString();
    chatSendMessage();
}

// ═══ Contexts ═══════════════════════════════════════════════════════════════════

async function loadContexts() {
    try {
        const res = await apiGet('/api/contexts');
        if (!res.ok) { document.getElementById('contexts-list').innerHTML = errorHtml(res.status); return; }
        const data = await res.json();
        const list = document.getElementById('contexts-list');
        if (!data.contexts || data.contexts.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No contexts found</span></div>';
            return;
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
                <h3>Settings</h3>
                <div class="data-list" style="margin-bottom:1rem;max-height:200px;overflow-y:auto">${settingsHtml}</div>
                <h3>Custom Data</h3>
                <div class="data-list" style="margin-bottom:1rem">${customDataHtml}</div>
                <h3>SM Data</h3>
                <div class="data-list" style="margin-bottom:1rem">${clDataHtml}</div>
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

// ═══ Templates ═════════════════════════════════════════════════════════════════

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
                    <span class="name" style="font-weight:600">&#x1F4C1; ${escapeHtml(folder)}/</span><span class="meta">${folders[folder].length} template(s)</span></div><div style="display:block">`;
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
            <input id="new-template-name" type="text" placeholder="tasks/my_task">
        </div>
        <textarea id="template-content" class="code-editor"><poml><task><p>Your content here</p></task></poml></textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewTemplate()">Create</button>`);
}

async function saveNewTemplate() {
    const name = document.getElementById('new-template-name').value.trim();
    const content = document.getElementById('template-content').value;
    if (!name) { alert('Name required'); return; }
    await apiFetch('/api/templates', { method: 'POST', body: JSON.stringify({ name, content }) });
    closeModal(); loadTemplates();
}

async function deleteTemplate(name) {
    if (!confirm(`Delete "${name}"?`)) return;
    await apiFetch(`/api/templates/${name}`, { method: 'DELETE' });
    loadTemplates();
}

async function editTemplate(name) {
    const res = await apiGet(`/api/templates/${name}`);
    const tpl = await res.json();
    let userOpts = '<option value="">No user</option>';
    try {
        const ctxRes = await apiGet('/api/contexts');
        if (ctxRes.ok) {
            const ctxData = await ctxRes.json();
            userOpts += (ctxData.contexts || []).map(c => c.user_id).filter(Boolean)
                .map(uid => `<option value="${escapeHtml(uid)}">${escapeHtml(uid)}</option>`).join('');
        }
    } catch {}
    showModal('Edit: ' + name, `
        <div style="margin-bottom:0.5rem">
            <label style="font-size:0.8rem;color:var(--text-secondary)">Preview as:</label>
            <select id="template-preview-user">${userOpts}</select>
        </div>
        <textarea id="template-content" class="code-editor">${escapeHtml(tpl.content || '')}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveTemplate('${escapeHtml(name)}')">Save & Preview</button>
        <div id="template-preview" style="margin-top:1rem;display:none"></div>`);
}

async function saveTemplate(name) {
    const content = document.getElementById('template-content').value;
    const userId = document.getElementById('template-preview-user').value;
    const res = await apiFetch(`/api/templates/${name}`, {
        method: 'PUT', body: JSON.stringify({ content, user_id: userId })
    });
    const data = await res.json();
    const preview = document.getElementById('template-preview');
    if (preview) {
        preview.style.display = 'block';
        preview.innerHTML = data.rendered_preview
            ? `<div style="font-size:0.75rem;color:var(--text-secondary);margin-bottom:0.3rem">Rendered:</div><pre style="background:var(--bg-tertiary);padding:0.75rem;border-radius:6px;white-space:pre-wrap;font-size:0.8rem;max-height:400px;overflow:auto">${escapeHtml(data.rendered_preview)}</pre>`
            : `<div style="color:var(--error)">${escapeHtml(data.error || '')}</div>`;
    }
    if (data.success) loadTemplates();
}

// ═══ Tools ══════════════════════════════════════════════════════════════════════

async function loadTools() {
    try {
        const res = await apiGet('/api/tools');
        const data = await res.json();
        const list = document.getElementById('tools-list');
        if (!data.tools || data.tools.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No tools</span></div>'; return;
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

// ═══ Secrets ═══════════════════════════════════════════════════════════════════

async function loadSecrets() {
    try {
        const res = await apiGet('/api/secrets');
        const data = await res.json();
        const knownKeys = ['discord_bot_token', 'openai_api_key', 'anthropic_api_key', 'minimax_api_key',
            'mimo_api_key', 'elevenlabs_api_key', 'gateway_api_key', 'dashboard_admin_password'];
        const customKeys = Object.keys(data).filter(k => !knownKeys.includes(k));

        container = document.getElementById('secrets-content');
        container.innerHTML = `
            <div class="data-list">${knownKeys.map(k => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(data[k] || 'Not set')}</span></div>`).join('')}</div>
            ${customKeys.length > 0 ? `<h3 style="margin-top:1rem">Custom</h3><div class="data-list">${customKeys.map(k => `<div class="data-item"><span class="name">${escapeHtml(k)}</span><span class="meta">${escapeHtml(data[k] || 'Not set')}</span><button class="btn btn-sm btn-danger" onclick="deleteCustomSecret('${escapeHtml(k)}')">Delete</button></div>`).join('')}</div>` : ''}
            <div style="margin-top:1.5rem"><h3>Update Secret</h3>
            <div class="form-group"><label>Field</label><select id="secret-field" style="width:100%;padding:0.75rem;background:var(--bg-secondary);border:1px solid var(--border);border-radius:8px;color:var(--text-primary)">
                ${knownKeys.map(k => `<option value="${k}">${escapeHtml(k)}</option>`).join('')}
                ${customKeys.map(k => `<option value="${k}">${escapeHtml(k)} (custom)</option>`).join('')}
            </select></div>
            <div class="form-group"><label>New Value</label><input type="password" id="secret-value" placeholder="Enter value"></div>
            <div class="form-group"><label>Master Password</label><input type="password" id="secret-master" placeholder="MASTER_KEY"></div>
            <button class="btn btn-primary" onclick="saveSecret()">Save</button>
            <p id="secret-msg" class="hidden" style="margin-top:0.5rem"></p></div>
            <div style="margin-top:1.5rem"><h3>Add Custom</h3>
            <div class="form-group"><label>Key</label><input type="text" id="custom-secret-key" placeholder="MY_KEY"></div>
            <div class="form-group"><label>Value</label><input type="password" id="custom-secret-value" placeholder="Value"></div>
            <div class="form-group"><label>Master Password</label><input type="password" id="custom-secret-master" placeholder="MASTER_KEY"></div>
            <button class="btn btn-primary" onclick="addCustomSecret()">Add</button>
            <p id="custom-secret-msg" class="hidden" style="margin-top:0.5rem"></p></div>`;
    } catch (err) { console.error(err); }
}

async function saveSecret() {
    const field = document.getElementById('secret-field').value;
    const value = document.getElementById('secret-value').value;
    const master = document.getElementById('secret-master').value;
    const msgEl = document.getElementById('secret-msg');
    if (!value) { msgEl.textContent = 'Value required'; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); return; }
    const body = { [field]: value };
    if (master) body.master_password = master;
    try {
        const res = await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify(body) });
        msgEl.textContent = await res.text();
        msgEl.style.color = 'var(--success)';
        msgEl.classList.remove('hidden');
        document.getElementById('secret-value').value = '';
        document.getElementById('secret-master').value = '';
        setTimeout(loadSecrets, 1500);
    } catch (err) {
        msgEl.textContent = 'Failed: ' + err.message;
        msgEl.style.color = 'var(--error)';
        msgEl.classList.remove('hidden');
    }
}

async function addCustomSecret() {
    const key = document.getElementById('custom-secret-key').value.trim();
    const value = document.getElementById('custom-secret-value').value;
    const master = document.getElementById('custom-secret-master').value;
    const msgEl = document.getElementById('custom-secret-msg');
    if (!key || !value) { msgEl.textContent = 'Key and value required'; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); return; }
    const body = { [key]: value };
    if (master) body.master_password = master;
    try {
        const res = await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify(body) });
        msgEl.textContent = await res.text();
        msgEl.style.color = 'var(--success)';
        msgEl.classList.remove('hidden');
        document.getElementById('custom-secret-key').value = '';
        document.getElementById('custom-secret-value').value = '';
        document.getElementById('custom-secret-master').value = '';
        setTimeout(loadSecrets, 1500);
    } catch (err) { msgEl.textContent = 'Failed: ' + err.message; msgEl.style.color = 'var(--error)'; msgEl.classList.remove('hidden'); }
}

async function deleteCustomSecret(key) {
    if (!confirm(`Delete "${key}"?`)) return;
    try {
        await apiFetch('/api/secrets', { method: 'PUT', body: JSON.stringify({ [key]: '' }) });
        loadSecrets();
    } catch (err) { alert('Failed: ' + err.message); }
}

// ═══ Pairings ══════════════════════════════════════════════════════════════════

async function loadPairings() {
    try {
        const [pRes, ppRes] = await Promise.all([apiGet('/api/pairings'), apiGet('/api/pairings/pending')]);
        const data = await pRes.json();
        const pendingData = await ppRes.json();
        document.getElementById('pairings-list').innerHTML = (data.pairings || []).length
            ? data.pairings.map(p => `<div class="data-item"><div><span class="name">${escapeHtml(p.user_id)}</span><span class="meta">Discord: ${escapeHtml(p.discord_user_id)}</span></div><div class="actions"><button class="btn btn-sm btn-danger" onclick="deletePairing('${escapeHtml(p.user_id)}')">Delete</button></div></div>`).join('')
            : '<div class="data-item"><span class="name">No pairings</span></div>';
        document.getElementById('pending-pairings-list').innerHTML = (pendingData.pending_pairings || []).length
            ? pendingData.pending_pairings.map(p => `<div class="data-item"><div><span class="name">${escapeHtml(p.code)}</span><span class="meta">Discord: ${escapeHtml(p.discord_user_id)}</span></div><div class="actions"><button class="btn btn-sm btn-primary" onclick="approvePendingPairing('${escapeHtml(p.code)}')">Approve</button><button class="btn btn-sm btn-danger" onclick="deletePendingPairing('${escapeHtml(p.code)}')">Delete</button></div></div>`).join('')
            : '<div class="data-item"><span class="name">No pending</span></div>';
    } catch (err) { console.error(err); }
}

async function deletePairing(userId) { if (!confirm('Delete?')) return; await apiFetch(`/api/pairings/${userId}`, { method: 'DELETE' }); loadPairings(); }
async function approvePendingPairing(code) { if (!confirm('Approve?')) return; await apiFetch(`/api/pairings/pending/${code}/approve`, { method: 'POST' }); loadPairings(); }
async function deletePendingPairing(code) { if (!confirm('Delete?')) return; await apiFetch(`/api/pairings/pending/${code}`, { method: 'DELETE' }); loadPairings(); }

// ═══ Statemachine Files ════════════════════════════════════════════════════════

async function loadSmFiles() {
    try {
        const res = await apiGet('/api/sm-files');
        const data = await res.json();
        const list = document.getElementById('sm-files-list');
        if (!data.sm_files || data.sm_files.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No SM files</span></div>'; return;
        }
        list.innerHTML = data.sm_files.map(f => `<div class="data-item">
            <span class="name">${escapeHtml(f.name)}</span>
            <div class="actions"><button class="btn btn-sm btn-primary" onclick="editSmFile('${escapeHtml(f.name)}')">Edit</button></div>
        </div>`).join('');
    } catch (err) { console.error(err); }
}

function createSmFile() {
    showModal('New Statemachine', `
        <div class="form-group"><label>File Name</label><input type="text" id="new-sm-name" placeholder="workflow.sm"></div>
        <textarea id="sm-content" class="code-editor">[state start]\nmode = chat\n\n[transitions]\n-> done</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewSmFile()">Create</button>`);
}

async function saveNewSmFile() {
    let name = document.getElementById('new-sm-name').value.trim();
    const content = document.getElementById('sm-content').value;
    if (!name) { alert('Name required'); return; }
    if (!name.endsWith('.sm')) name += '.sm';
    const res = await apiFetch(`/api/sm-files/${name}`, { method: 'PUT', body: JSON.stringify({ content }) });
    const text = await res.text();
    if (text.includes('Error')) { alert(text); }
    else { closeModal(); loadSmFiles(); }
}

async function editSmFile(name) {
    const res = await apiGet(`/api/sm-files/${name}`);
    const content = await res.text();
    showModal('Edit: ' + name, `
        <textarea id="sm-content" class="code-editor">${escapeHtml(content)}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveSmFile('${escapeHtml(name)}')">Save</button>`);
}

async function saveSmFile(name) {
    const content = document.getElementById('sm-content').value;
    const res = await apiFetch(`/api/sm-files/${name}`, { method: 'PUT', body: JSON.stringify({ content }) });
    const text = await res.text();
    if (text.includes('Error')) alert(text);
    else { closeModal(); loadSmFiles(); }
}

// ═══ Cron Jobs ═════════════════════════════════════════════════════════════════

async function loadCronJobs() {
    try {
        const res = await apiGet('/api/cron-jobs');
        const data = await res.json();
        document.getElementById('cron-jobs-list').innerHTML = (data.cron_jobs || []).length
            ? data.cron_jobs.map(j => `<div class="data-item"><div><span class="name">${escapeHtml(j.name)}</span><span class="meta">${escapeHtml(j.schedule)} | Runs: ${j.run_count}</span></div><div class="toggle ${j.enabled ? 'active' : ''}"></div></div>`).join('')
            : '<div class="data-item"><span class="name">No cron jobs</span></div>';
    } catch (err) { console.error(err); }
}

// ═══ Messages & Memory ═════════════════════════════════════════════════════════

async function populateUserDropdowns() {
    try {
        const res = await apiGet('/api/contexts');
        if (!res.ok) return;
        const data = await res.json();
        const users = (data.contexts || []).map(c => c.user_id).filter(Boolean);
        for (const sid of ['message-user-id', 'memory-user-id']) {
            const s = document.getElementById(sid);
            if (!s) continue;
            const cur = s.value;
            s.innerHTML = '<option value="">Select user...</option>' + users.map(u => `<option value="${escapeHtml(u)}">${escapeHtml(u)}</option>`).join('');
            if (cur && users.includes(cur)) s.value = cur;
        }
    } catch {}
}

async function loadMessages() {
    const uid = document.getElementById('message-user-id').value;
    if (!uid) { alert('Select a user'); return; }
    const list = document.getElementById('messages-list');
    list.innerHTML = '<div class="data-item">Loading...</div>';
    try {
        const res = await apiGet(`/api/messages/${encodeURIComponent(uid)}`);
        const data = await res.json();
        if (!data.messages || data.messages.length === 0) {
            list.innerHTML = '<div class="data-item">No messages</div>'; return;
        }
        list.innerHTML = data.messages.map(m => `<div class="data-item">
            <span class="name">${escapeHtml(m.role)}</span>
            <span class="meta">${escapeHtml((m.content || '').substring(0, 100))}</span>
        </div>`).join('');
        const info = document.createElement('div');
        info.className = 'data-item';
        info.innerHTML = `<span class="name">${data.message_count} msgs (~${data.total_tokens} tokens)</span>`;
        list.prepend(info);
    } catch (err) { list.innerHTML = errorHtml(err.message); }
}

async function loadMemory() {
    const uid = document.getElementById('memory-user-id').value;
    if (!uid) { alert('Select a user'); return; }
    const container = document.getElementById('memory-content');
    container.innerHTML = '<div class="data-item">Loading...</div>';
    try {
        const res = await apiGet(`/api/memory/${encodeURIComponent(uid)}`);
        const data = await res.json();
        container.innerHTML = `
            <h3>Facts</h3><div class="data-list">${(data.learned_facts || []).map(f => `<div class="data-item"><span class="name">${escapeHtml(f)}</span></div>`).join('') || '<div class="data-item">None</div>'}</div>
            <h3 style="margin-top:1rem">Topics</h3><div class="data-list">${(data.last_topics || []).map(t => `<div class="data-item"><span class="name">${escapeHtml(t)}</span></div>`).join('') || '<div class="data-item">None</div>'}</div>
            <h3 style="margin-top:1rem">Variables</h3><pre class="code-editor">${JSON.stringify(data.custom_variables || {}, null, 2)}</pre>`;
    } catch (err) { container.innerHTML = errorHtml(err.message); }
}

// ═══ VM ════════════════════════════════════════════════════════════════════════

let vncRfb = null;
let vncConnectedVm = null;
let vncModule = null;

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
    try {
        await loadVMStatus();
        await loadVMActivity();
    } catch (err) { console.error('VM load error:', err); }
    startVMRefreshLoop();
}

function startVMRefreshLoop() {
    if (vmRefreshInterval) clearInterval(vmRefreshInterval);
    vmRefreshInterval = setInterval(() => {
        loadVMStatus();
    }, 5000);
    document.getElementById('vm-auto-refresh-indicator').textContent = '(auto-refresh 5s)';
}

async function loadVMStatus() {
    const container = document.getElementById('vm-status-bar');
    try {
        const res = await apiGet('/api/vm');
        if (!res.ok) {
            container.innerHTML = '<div class="data-item"><span class="name" style="color:var(--text-secondary)">VM not available. Set VM_ENABLED=true</span></div>';
            return;
        }
        const data = await res.json();
        const vms = data.vms || [];

        if (vms.length === 0) {
            container.innerHTML = '<div class="data-item"><span class="name">No VMs running</span></div>';
            disconnectVNC();
            updateVMButtons(null);
        } else {
            container.innerHTML = vms.map(vm => {
                const isStopped = vm.status === 'stopped';
                return `<div class="data-item">
                    <div><span class="name">${escapeHtml(vm.name)}</span>
                    <span class="meta">Status: <span style="color:${isStopped ? 'var(--text-secondary)' : 'var(--success)'}">${escapeHtml(vm.status)}</span> | PID: ${vm.pid || '-'} | VNC: ${vm.vnc_port || '-'}${vm.current_iso ? ' | CD: ' + escapeHtml(vm.current_iso.split('/').pop()) : ''}</span></div>
                    <div class="actions">
                        ${isStopped ? `<button class="btn btn-sm btn-primary" onclick="vmStartByName('${escapeHtml(vm.name)}')">Start</button>` : ''}
                        ${vm.status === 'running' ? `<button class="btn btn-sm btn-danger" onclick="vmStop('${escapeHtml(vm.name)}')">Stop</button>` : ''}
                        ${vm.status === 'running' ? `<button class="btn btn-sm btn-secondary" onclick="vmReboot('${escapeHtml(vm.name)}')">Reboot</button>` : ''}
                    </div></div>`;
            }).join('');

            const runningVm = vms.find(vm => vm.status === 'running');
            if (runningVm) {
                if (vncConnectedVm !== runningVm.name) connectVNC(runningVm.name);
                updateVMButtons(runningVm);
            } else {
                disconnectVNC();
                updateVMButtons(vms.length > 0 ? vms[0] : null);
            }
        }

        const config = data.config || {};
        if (config.vm_enabled !== undefined) {
            document.getElementById('vm-config').innerHTML = `
                <div class="data-item"><span class="name">VM Enabled</span><span class="meta">${config.vm_enabled ? 'Yes' : 'No'}</span></div>
                <div class="data-item"><span class="name">CPU</span><span class="meta">${config.vm_cpu_cores || '-'}</span></div>
                <div class="data-item"><span class="name">RAM</span><span class="meta">${config.vm_ram_mb || '-'} MB</span></div>
                <div class="data-item"><span class="name">Disk</span><span class="meta">${config.vm_disk_size || '-'}</span></div>
                <div class="data-item"><span class="name">Arch</span><span class="meta">${config.vm_arch || '-'}</span></div>`;
        }
    } catch (err) { container.innerHTML = errorHtml(err.message); }
}

function updateVMButtons(vm) {
    const stopBtn = document.querySelector('#vm-controls .btn-danger');
    const rebootBtn = Array.from(document.querySelectorAll('#vm-controls .btn-secondary')).find(b => b.textContent === 'Reboot');
    const cdBtns = Array.from(document.querySelectorAll('#vm-controls .btn-secondary')).filter(b => b.textContent.includes('Insert CD') || b.textContent.includes('Eject CD'));
    const running = vm && vm.status === 'running';
    [stopBtn, rebootBtn, ...cdBtns].forEach(b => { if (b) b.style.display = running ? '' : 'none'; });
}

async function vmStartByName(name) {
    const layouts = [{v:'us',l:'US (QWERTY)'},{v:'de',l:'DE (QWERTZ)'},{v:'fr',l:'FR (AZERTY)'},{v:'es',l:'ES'},{v:'it',l:'IT'},{v:'gb',l:'GB'}];
    let layout = 'us';
    try {
        const ctxRes = await apiGet('/api/contexts/default');
        if (ctxRes.ok) { const ctx = await ctxRes.json(); layout = ctx.settings?.vm_keyboard_layout || 'us'; }
    } catch {}

    showModal('Start VM', `
        <div class="form-group"><label>Name</label><input type="text" id="vm-start-name" value="${escapeHtml(name)}"></div>
        <div class="form-group"><label>CPU Cores</label><input type="number" id="vm-start-cpu" value="2"></div>
        <div class="form-group"><label>RAM (MB)</label><input type="number" id="vm-start-ram" value="4096"></div>
        <div class="form-group"><label>Disk Size</label><input type="text" id="vm-start-disk" value="40G"></div>
        <div class="form-group"><label>ISO Path (optional)</label><input type="text" id="vm-start-iso" placeholder="/path/to/linux.iso"></div>
        <div class="form-group"><label>Keyboard Layout</label>
        <select id="vm-start-layout">${layouts.map(l => `<option value="${l.v}" ${l.v===layout?'selected':''}>${l.l}</option>`).join('')}</select></div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoStart()">Start</button>`);
}

async function vmStart() {
    vmStartByName('praxis-vm');
}

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
        alert(data.message || data.error || 'Started');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmStop(name) {
    name = name || 'praxis-vm';
    try {
        const res = await apiFetch('/api/vm/stop', { method: 'POST', body: JSON.stringify({ name }) });
        const data = await res.json();
        alert(data.message || data.error || 'Stopped');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmReboot(name) {
    name = name || 'praxis-vm';
    if (!confirm('Reboot?')) return;
    try {
        const res = await apiFetch('/api/vm/reboot', { method: 'POST', body: JSON.stringify({ name }) });
        const data = await res.json();
        alert(data.message || data.error || 'Rebooting');
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

function vmRefresh() { loadVM(); }
async function vmInsertCD() { const iso = prompt('ISO path:'); if (iso) { const res = await apiFetch('/api/vm/cd', { method: 'POST', body: JSON.stringify({ iso_path: iso }) }); const d = await res.json(); alert(d.message || d.error); } }
async function vmEjectCD() { const res = await apiFetch('/api/vm/cd', { method: 'POST', body: JSON.stringify({ iso_path: null }) }); const d = await res.json(); alert(d.message || d.error); }
async function vmCreateSnapshot() { const n = prompt('Name:'); if (n) { const r = await apiFetch('/api/vm/snapshot', { method: 'POST', body: JSON.stringify({ snapshot_name: n }) }); const d = await r.json(); alert(d.message || d.error); } }

async function vmAddSharedFolder() {
    showModal('Add Shared Folder', `
        <div class="form-group"><label>Host Path</label><input type="text" id="vm-sf-host" placeholder="/path/on/host"></div>
        <div class="form-group"><label>Mount Point (in VM)</label><input type="text" id="vm-sf-mount" value="/mnt/shared"></div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoAddSharedFolder()">Add</button>`);
}
async function vmDoAddSharedFolder() {
    const host_path = document.getElementById('vm-sf-host').value;
    const mount_point = document.getElementById('vm-sf-mount').value;
    if (!host_path) { alert('Host path required'); return; }
    const res = await apiFetch('/api/vm/shared-folder', { method: 'POST', body: JSON.stringify({ host_path, mount_point }) });
    const data = await res.json();
    closeModal();
    alert(data.message || data.error);
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

        placeholder.textContent = 'Connecting...';
        placeholder.style.display = 'block';
        screen.innerHTML = '';

        vncRfb = new RFB(screen, wsUrl, { credentials: { password: '' }, shared: true, wsProtocols: ['binary'] });
        vncRfb.addEventListener('connect', () => { vncLog('VNC connected!', 'success'); placeholder.style.display = 'none'; vncConnectedVm = vmName; });
        vncRfb.addEventListener('disconnect', e => {
            vncLog(`Disconnected: ${e.detail?.reason || 'unknown'}`, e.detail?.clean ? 'warn' : 'error');
            placeholder.style.display = 'block'; placeholder.textContent = 'VNC disconnected.'; vncConnectedVm = null;
        });
        vncRfb.scaleViewport = true;
        vncRfb.resizeSession = false;
    } catch (err) { vncLog(`Error: ${err.message}`, 'error'); }
}

function disconnectVNC() {
    if (vncRfb) { vncRfb.disconnect(); vncRfb = null; }
    const placeholder = document.getElementById('vm-vnc-placeholder');
    const screen = document.getElementById('vm-vnc-screen');
    if (placeholder) { placeholder.style.display = 'block'; placeholder.textContent = 'VM not running.'; }
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
            container.innerHTML = '<div style="color:var(--text-secondary)">No activity yet.</div>'; return;
        }
        container.innerHTML = activities.map(a => {
            const time = a.created_at ? new Date(a.created_at).toLocaleTimeString() : '';
            const name = a.action || '';
            let color = 'var(--text-primary)', icon = '[TOOL]';
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

            let inputSum = '';
            try {
                const inp = JSON.parse(a.input || '{}');
                if (inp.command) inputSum = inp.command.substring(0, 80);
                else if (inp.keys) inputSum = `"${inp.keys}"`;
                else if (inp.path) inputSum = inp.path;
                else if (inp.message) inputSum = inp.message.substring(0, 60);
                else if (inp.action) inputSum = inp.action;
                else inputSum = (a.input || '').substring(0, 80);
            } catch { inputSum = (a.input || '').substring(0, 80); }
            const outPrev = (a.output || '').substring(0, 120).replace(/\n/g, ' ');
            const vmTag = a.vm_id ? ` [${a.vm_id}]` : '';

            return `<div style="margin-bottom:0.3rem;padding:0.2rem 0;border-bottom:1px solid rgba(255,255,255,0.05)">
                <span style="color:var(--text-secondary);font-size:0.75rem">${time}</span>
                <span style="color:${color};font-weight:600;font-size:0.8rem">${icon}${vmTag} ${escapeHtml(name)}</span>
                <span style="color:var(--text-primary);font-size:0.8rem"> ${escapeHtml(inputSum)}</span>
                ${outPrev ? `<div style="color:var(--text-secondary);font-size:0.75rem;padding-left:1rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml(outPrev)}</div>` : ''}
            </div>`;
        }).join('');
    } catch (err) { console.error('Activity load error:', err); }
}

// ═══ Helpers ═══════════════════════════════════════════════════════════════════

function escapeHtml(str) { const d = document.createElement('div'); d.textContent = str; return d.innerHTML; }
function errorHtml(msg) { return `<div class="data-item"><span style="color:var(--error)">${escapeHtml(msg)}</span></div>`; }

// ═══ Modal ═════════════════════════════════════════════════════════════════════

function showModal(title, content) {
    closeModal();
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.id = 'modal-overlay';
    overlay.innerHTML = `<div class="modal"><h3>${title}</h3>${content}<button class="btn btn-secondary" style="margin-top:1rem" onclick="closeModal()">Close</button></div>`;
    document.body.appendChild(overlay);
}
function closeModal() { const o = document.getElementById('modal-overlay'); if (o) o.remove(); }

// ═══ Events ════════════════════════════════════════════════════════════════════

document.addEventListener('DOMContentLoaded', () => {
    if (authToken) validateTokenAndLoad();

    document.getElementById('login-form').addEventListener('submit', async e => {
        e.preventDefault();
        const pwd = document.getElementById('password').value;
        const errEl = document.getElementById('login-error');
        try { await login(pwd); errEl.classList.add('hidden'); showScreen('dashboard-screen'); loadOverview(); initChatTab(); }
        catch (err) { errEl.textContent = err.message; errEl.classList.remove('hidden'); }
    });

    document.getElementById('logout-btn').addEventListener('click', logout);
    document.querySelectorAll('.nav-links li').forEach(li => { li.addEventListener('click', () => showTab(li.dataset.tab)); });
    document.getElementById('load-messages-btn').addEventListener('click', loadMessages);
    document.getElementById('load-memory-btn').addEventListener('click', loadMemory);
});

