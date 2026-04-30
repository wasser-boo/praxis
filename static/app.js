const API_BASE = '';
let authToken = localStorage.getItem('praxis_token');

// ── Auth ──────────────────────────────────────────────────────────────────────

async function login(password) {
    const res = await fetch(`${API_BASE}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ password })
    });

    if (!res.ok) {
        throw new Error('Invalid password');
    }

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
    return {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${authToken}`
    };
}

async function apiFetch(path, options = {}) {
    const res = await fetch(`${API_BASE}${path}`, {
        ...options,
        headers: { ...getAuthHeaders(), ...options.headers }
    });

    if (res.status === 401) {
        logout();
        throw new Error('Session expired');
    }

    return res;
}

// ── Screen Management ─────────────────────────────────────────────────────────

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

// ── Data Loading ──────────────────────────────────────────────────────────────

async function loadTabData(tab) {
    try {
        switch (tab) {
            case 'overview':
                await loadOverview();
                break;
            case 'contexts':
                await loadContexts();
                break;
            case 'templates':
                await loadTemplates();
                break;
            case 'tools':
                await loadTools();
                break;
            case 'secrets':
                await loadSecrets();
                break;
            case 'pairings':
                await loadPairings();
                break;
            case 'cl-files':
                await loadClFiles();
                break;
            case 'cron-jobs':
                await loadCronJobs();
                break;
        }
    } catch (err) {
        console.error(`Failed to load ${tab}:`, err);
    }
}

async function loadOverview() {
    try {
        const [statusRes, contextsRes, pairingsRes] = await Promise.all([
            apiFetch('/api/status'),
            apiFetch('/api/contexts'),
            apiFetch('/api/pairings')
        ]);

        const status = await statusRes.json();
        const contexts = await contextsRes.json();
        const pairings = await pairingsRes.json();

        document.getElementById('version').textContent = status.version || '-';
        document.getElementById('context-count').textContent = contexts.contexts?.length || 0;
        document.getElementById('pairing-count').textContent = pairings.pairings?.length || 0;
    } catch (err) {
        console.error('Failed to load overview:', err);
    }
}

async function loadContexts() {
    const res = await apiFetch('/api/contexts');
    const data = await res.json();
    const list = document.getElementById('contexts-list');

    if (!data.contexts || data.contexts.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No contexts found</span></div>';
        return;
    }

    list.innerHTML = data.contexts.map(ctx => `
        <div class="data-item">
            <div>
                <span class="name">${ctx.user_id}</span>
                <span class="meta">Updated: ${new Date(ctx.updated_at).toLocaleString()}</span>
            </div>
            <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="viewContext('${ctx.user_id}')">View</button>
            </div>
        </div>
    `).join('');
}

async function viewContext(userId) {
    const res = await apiFetch(`/api/contexts/${userId}`);
    const ctx = await res.json();

    showModal('Context: ' + userId, `
        <pre class="code-editor" readonly>${JSON.stringify(ctx, null, 2)}</pre>
    `);
}

async function loadTemplates() {
    const res = await apiFetch('/api/templates');
    const data = await res.json();
    const list = document.getElementById('templates-list');

    if (!data.templates || data.templates.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No templates found</span></div>';
        return;
    }

    list.innerHTML = data.templates.map(t => `
        <div class="data-item">
            <div>
                <span class="name">${t.name}</span>
                <span class="meta">${t.is_system ? 'System' : 'User'}</span>
            </div>
            <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="editTemplate('${t.name}')">Edit</button>
            </div>
        </div>
    `).join('');
}

async function editTemplate(name) {
    const res = await apiFetch(`/api/templates/${name}`);
    const tpl = await res.json();

    showModal('Edit Template: ' + name, `
        <textarea id="template-content" class="code-editor">${tpl.content || ''}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveTemplate('${name}')">Save</button>
    `);
}

async function saveTemplate(name) {
    const content = document.getElementById('template-content').value;
    await apiFetch(`/api/templates/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ content })
    });
    closeModal();
    loadTemplates();
}

async function loadTools() {
    const res = await apiFetch('/api/tools');
    const data = await res.json();
    const list = document.getElementById('tools-list');

    if (!data.tools || data.tools.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No tools found</span></div>';
        return;
    }

    list.innerHTML = data.tools.map(t => `
        <div class="data-item">
            <div>
                <span class="name">${t.name}</span>
                <span class="meta">${t.description || ''}</span>
            </div>
            <div class="toggle ${t.is_enabled ? 'active' : ''}" onclick="toggleTool('${t.name}', ${!t.is_enabled})"></div>
        </div>
    `).join('');
}

async function toggleTool(name, enabled) {
    await apiFetch(`/api/tools/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ is_enabled: enabled })
    });
    loadTools();
}

async function loadSecrets() {
    const res = await apiFetch('/api/secrets');
    const data = await res.json();
    const container = document.getElementById('secrets-content');

    container.innerHTML = `
        <div class="data-list">
            ${Object.entries(data).map(([key, value]) => `
                <div class="data-item">
                    <span class="name">${key}</span>
                    <span class="meta">${value || 'Not set'}</span>
                </div>
            `).join('')}
        </div>
    `;
}

async function loadPairings() {
    const res = await apiFetch('/api/pairings');
    const data = await res.json();
    const list = document.getElementById('pairings-list');

    if (!data.pairings || data.pairings.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No pairings found</span></div>';
        return;
    }

    list.innerHTML = data.pairings.map(p => `
        <div class="data-item">
            <div>
                <span class="name">${p.user_id}</span>
                <span class="meta">Discord: ${p.discord_user_id}</span>
            </div>
            <div class="actions">
                <button class="btn btn-sm btn-danger" onclick="deletePairing('${p.user_id}')">Delete</button>
            </div>
        </div>
    `).join('');
}

async function deletePairing(userId) {
    if (!confirm('Delete this pairing?')) return;
    await apiFetch(`/api/pairings/${userId}`, { method: 'DELETE' });
    loadPairings();
}

async function loadClFiles() {
    const res = await apiFetch('/api/cl-files');
    const data = await res.json();
    const list = document.getElementById('cl-files-list');

    if (!data.cl_files || data.cl_files.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No CL files found</span></div>';
        return;
    }

    list.innerHTML = data.cl_files.map(f => `
        <div class="data-item">
            <span class="name">${f.name}</span>
            <div class="actions">
                <button class="btn btn-sm btn-primary" onclick="editClFile('${f.name}')">Edit</button>
            </div>
        </div>
    `).join('');
}

async function editClFile(name) {
    const res = await apiFetch(`/api/cl-files/${name}`);
    const content = await res.text();

    showModal('Edit CL File: ' + name, `
        <textarea id="cl-content" class="code-editor">${content}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveClFile('${name}')">Save</button>
    `);
}

async function saveClFile(name) {
    const content = document.getElementById('cl-content').value;
    const res = await apiFetch(`/api/cl-files/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ content })
    });
    const result = await res.text();
    if (result.includes('Error')) {
        alert(result);
    } else {
        closeModal();
        loadClFiles();
    }
}

async function loadCronJobs() {
    const res = await apiFetch('/api/cron-jobs');
    const data = await res.json();
    const list = document.getElementById('cron-jobs-list');

    if (!data.cron_jobs || data.cron_jobs.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No cron jobs found</span></div>';
        return;
    }

    list.innerHTML = data.cron_jobs.map(j => `
        <div class="data-item">
            <div>
                <span class="name">${j.name}</span>
                <span class="meta">${j.schedule} | Runs: ${j.run_count}</span>
            </div>
            <div class="toggle ${j.enabled ? 'active' : ''}"></div>
        </div>
    `).join('');
}

async function loadMessages() {
    const userId = document.getElementById('message-user-id').value;
    if (!userId) return;

    const res = await apiFetch(`/api/messages/${userId}`);
    const data = await res.json();
    const list = document.getElementById('messages-list');

    if (!data.messages || data.messages.length === 0) {
        list.innerHTML = '<div class="data-item"><span class="name">No messages found</span></div>';
        return;
    }

    list.innerHTML = data.messages.map(m => `
        <div class="message ${m.role}">
            <div class="role">${m.role}</div>
            <div class="content">${escapeHtml(m.content || '')}</div>
        </div>
    `).join('');
}

async function loadMemory() {
    const userId = document.getElementById('memory-user-id').value;
    if (!userId) return;

    const res = await apiFetch(`/api/memory/${userId}`);
    const data = await res.json();
    const container = document.getElementById('memory-content');

    container.innerHTML = `
        <h3>Learned Facts</h3>
        <div class="data-list">
            ${(data.learned_facts || []).map(f => `<div class="data-item"><span class="name">${escapeHtml(f)}</span></div>`).join('') || '<div class="data-item"><span class="name">None</span></div>'}
        </div>
        <h3 style="margin-top:1rem">Last Topics</h3>
        <div class="data-list">
            ${(data.last_topics || []).map(t => `<div class="data-item"><span class="name">${escapeHtml(t)}</span></div>`).join('') || '<div class="data-item"><span class="name">None</span></div>'}
        </div>
        <h3 style="margin-top:1rem">Custom Variables</h3>
        <pre class="code-editor">${JSON.stringify(data.custom_variables || {}, null, 2)}</pre>
    `;
}

// ── Modal ─────────────────────────────────────────────────────────────────────

function showModal(title, content) {
    const overlay = document.createElement('div');
    overlay.className = 'modal-overlay';
    overlay.id = 'modal-overlay';
    overlay.innerHTML = `
        <div class="modal">
            <h3>${title}</h3>
            ${content}
            <button class="btn btn-secondary" style="margin-top:1rem" onclick="closeModal()">Close</button>
        </div>
    `;
    document.body.appendChild(overlay);
}

function closeModal() {
    const overlay = document.getElementById('modal-overlay');
    if (overlay) overlay.remove();
}

// ── Utilities ─────────────────────────────────────────────────────────────────

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

// ── Event Listeners ───────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
    // Check if already logged in
    if (authToken) {
        showScreen('dashboard-screen');
        loadOverview();
    }

    // Login form
    document.getElementById('login-form').addEventListener('submit', async (e) => {
        e.preventDefault();
        const password = document.getElementById('password').value;
        const errorEl = document.getElementById('login-error');

        try {
            await login(password);
            errorEl.classList.add('hidden');
            showScreen('dashboard-screen');
            loadOverview();
        } catch (err) {
            errorEl.textContent = err.message;
            errorEl.classList.remove('hidden');
        }
    });

    // Logout
    document.getElementById('logout-btn').addEventListener('click', logout);

    // Navigation
    document.querySelectorAll('.nav-links li').forEach(li => {
        li.addEventListener('click', () => showTab(li.dataset.tab));
    });

    // Load messages
    document.getElementById('load-messages-btn').addEventListener('click', loadMessages);

    // Load memory
    document.getElementById('load-memory-btn').addEventListener('click', loadMemory);
});
