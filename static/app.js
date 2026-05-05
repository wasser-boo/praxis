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

async function validateTokenAndLoad() {
    try {
        const res = await fetch(`${API_BASE}/api/contexts`, {
            headers: { 'Authorization': `Bearer ${authToken}` }
        });
        if (res.ok) {
            showScreen('dashboard-screen');
            loadOverview();
        } else {
            logout();
        }
    } catch (err) {
        logout();
    }
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
            case 'vm':
                await loadVM();
                break;
            case 'messages':
                await populateUserDropdowns();
                break;
            case 'memory':
                await populateUserDropdowns();
                break;
        }
    } catch (err) {
        console.error(`Failed to load ${tab}:`, err);
    }
}

async function loadOverview() {
    try {
        const [statusRes, contextsRes, pairingsRes, pendingRes] = await Promise.all([
            apiFetch('/api/status'),
            apiFetch('/api/contexts'),
            apiFetch('/api/pairings'),
            apiFetch('/api/pairings/pending')
        ]);

        const status = await statusRes.json();
        const contexts = await contextsRes.json();
        const pairings = await pairingsRes.json();
        const pending = await pendingRes.json();

        document.getElementById('version').textContent = status.version || '-';
        document.getElementById('context-count').textContent = contexts.contexts?.length || 0;
        document.getElementById('pairing-count').textContent = pairings.pairings?.length || 0;
        document.getElementById('pending-count').textContent = pending.pending_pairings?.length || 0;
    } catch (err) {
        console.error('Failed to load overview:', err);
    }
}

async function loadContexts() {
    try {
        const res = await apiFetch('/api/contexts');

        if (!res.ok) {
            const text = await res.text();
            const list = document.getElementById('contexts-list');
            list.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">API error ${res.status}: ${escapeHtml(text)}</span></div>`;
            return;
        }

        const data = await res.json();
        const list = document.getElementById('contexts-list');

        if (!data.contexts || data.contexts.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No contexts found</span></div>';
            return;
        }

        list.innerHTML = data.contexts.map(ctx => {
            const uid = ctx.user_id || '';
            const safeUid = uid.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
            return `
            <div class="data-item">
                <div>
                    <span class="name">${escapeHtml(uid)}</span>
                    <span class="meta">Updated: ${ctx.updated_at ? new Date(ctx.updated_at).toLocaleString() : '-'}</span>
                </div>
                <div class="actions">
                    <button class="btn btn-sm btn-primary" onclick="viewContext('${safeUid}')">View</button>
                    <button class="btn btn-sm btn-danger" onclick="deleteContext('${safeUid}')">Delete</button>
                </div>
            </div>
        `}).join('');
    } catch (err) {
        console.error('Failed to load contexts:', err);
        const list = document.getElementById('contexts-list');
        if (list) list.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`;
    }
}

async function viewContext(userId) {
    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(userId)}`);

        if (!res.ok) {
            const text = await res.text();
            alert('Failed to load context: ' + res.status + ' ' + text);
            return;
        }

        const ctx = await res.json();

        const redundantKeys = new Set([
            'mimo_api_key', 'minimax_api_key', 'voice_elevenlabs_api_key', 'voice_elevenlabs_stt_api_key',
            'cl_file', 'active_state', 'active_templates', 'llm_turn', 'compaction_summary', 'download'
        ]);

        const filteredSettings = ctx.settings
            ? Object.entries(ctx.settings).filter(([k]) => !redundantKeys.has(k))
            : [];

        const settingsHtml = filteredSettings.length > 0
            ? filteredSettings.map(([k, v]) => `
                <div class="data-item">
                    <span class="name">${escapeHtml(k)}</span>
                    <span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v ?? ''))}</span>
                </div>
            `).join('')
            : '<div class="data-item"><span class="name">No settings</span></div>';

        const customDataHtml = ctx.custom_data && Object.keys(ctx.custom_data).length > 0
            ? Object.entries(ctx.custom_data).map(([k, v]) => `
                <div class="data-item">
                    <span class="name">${escapeHtml(k)}</span>
                    <span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span>
                </div>
            `).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const clDataHtml = ctx.cl_data && Object.keys(ctx.cl_data).length > 0
            ? Object.entries(ctx.cl_data).map(([k, v]) => `
                <div class="data-item">
                    <span class="name">${escapeHtml(k)}</span>
                    <span class="meta">${escapeHtml(typeof v === 'object' ? JSON.stringify(v) : String(v))}</span>
                </div>
            `).join('')
            : '<div class="data-item"><span class="name">None</span></div>';

        const ctxJson = JSON.stringify(ctx, null, 2);
        const uid = escapeHtml(userId);

        showModal('Context: ' + uid, `
            <div id="context-view-mode">
                <div class="data-list" style="margin-bottom:1rem">
                    <div class="data-item"><span class="name">User ID</span><span class="meta">${escapeHtml(ctx.user_id || '')}</span></div>
                    <div class="data-item"><span class="name">Turn</span><span class="meta">${ctx.turn ?? 0}</span></div>
                    <div class="data-item"><span class="name">Mode</span><span class="meta">${escapeHtml(ctx.mode || '')}</span></div>
                    <div class="data-item"><span class="name">User Name</span><span class="meta">${escapeHtml(ctx.user_name || '-')}</span></div>
                    <div class="data-item"><span class="name">CL File</span><span class="meta">${escapeHtml(ctx.cl_file || '-')}</span></div>
                    <div class="data-item"><span class="name">Active State</span><span class="meta">${escapeHtml(ctx.active_state || '-')}</span></div>
                    <div class="data-item"><span class="name">Active Templates</span><span class="meta">${(ctx.active_templates || []).join(', ') || '-'}</span></div>
                </div>
                <h3>Settings</h3>
                <div class="data-list" style="margin-bottom:1rem;max-height:200px;overflow-y:auto">${settingsHtml}</div>
                <h3>Custom Data</h3>
                <div class="data-list" style="margin-bottom:1rem">${customDataHtml}</div>
                <h3>CL Data</h3>
                <div class="data-list" style="margin-bottom:1rem">${clDataHtml}</div>
                <button class="btn btn-primary" style="width:auto" id="ctx-edit-btn">Edit</button>
                <button class="btn btn-danger" style="width:auto" id="ctx-delete-btn">Delete</button>
            </div>
            <div id="context-edit-mode" style="display:none">
                <textarea id="context-edit-json" class="code-editor" style="min-height:400px">${escapeHtml(ctxJson)}</textarea>
                <div style="display:flex;gap:0.5rem;margin-top:1rem">
                    <button class="btn btn-primary" style="width:auto" id="ctx-save-btn">Save</button>
                    <button class="btn btn-secondary" style="width:auto" id="ctx-cancel-btn">Cancel</button>
                </div>
            </div>
        `);

        document.getElementById('ctx-edit-btn').addEventListener('click', () => toggleContextEdit());
        document.getElementById('ctx-save-btn').addEventListener('click', () => saveContextEdit(userId));
        document.getElementById('ctx-cancel-btn').addEventListener('click', () => toggleContextEdit());
        document.getElementById('ctx-delete-btn').addEventListener('click', () => deleteContext(userId));
    } catch (err) {
        console.error('Failed to load context:', err);
        alert('Failed to load context: ' + err.message);
    }
}

function toggleContextEdit() {
    const view = document.getElementById('context-view-mode');
    const edit = document.getElementById('context-edit-mode');
    if (!view || !edit) return;
    view.style.display = view.style.display === 'none' ? 'block' : 'none';
    edit.style.display = edit.style.display === 'none' ? 'block' : 'none';
}

async function saveContextEdit(userId) {
    const jsonStr = document.getElementById('context-edit-json').value;
    let updates;
    try {
        updates = JSON.parse(jsonStr);
    } catch (e) {
        alert('Invalid JSON: ' + e.message);
        return;
    }

    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(userId)}`, {
            method: 'PUT',
            body: JSON.stringify(updates)
        });

        if (res.ok) {
            viewContext(userId);
        } else {
            const text = await res.text();
            alert('Failed to save context: ' + text);
        }
    } catch (err) {
        alert('Failed to save context: ' + err.message);
    }
}

async function deleteContext(userId) {
    if (!confirm(`Delete context for "${userId}"?`)) return;
    try {
        const res = await apiFetch(`/api/contexts/${encodeURIComponent(userId)}`, {
            method: 'DELETE'
        });
        if (res.ok) {
            const modal = document.getElementById('modal-overlay');
            if (modal) modal.style.display = 'none';
            loadContexts();
        } else {
            const text = await res.text();
            alert('Failed to delete context: ' + text);
        }
    } catch (err) {
        alert('Failed to delete context: ' + err.message);
    }
}

async function loadTemplates() {
    try {
        const res = await apiFetch('/api/templates');
        const data = await res.json();
        const list = document.getElementById('templates-list');

        let html = '<div class="data-item" style="margin-bottom:0.5rem"><button class="btn btn-sm btn-primary" onclick="createTemplate()">+ New Template</button></div>';

        if (!data.templates || data.templates.length === 0) {
            html += '<div class="data-item"><span class="name">No templates found</span></div>';
        } else {
            html += data.templates.map(t => `
                <div class="data-item">
                    <div>
                        <span class="name">${escapeHtml(t.name)}</span>
                        <span class="meta">${t.is_system ? 'System' : 'User'}</span>
                    </div>
                    <div class="actions">
                        <button class="btn btn-sm btn-primary" onclick="editTemplate('${escapeHtml(t.name)}')">Edit</button>
                        <button class="btn btn-sm btn-danger" onclick="deleteTemplate('${escapeHtml(t.name)}')">Delete</button>
                    </div>
                </div>
            `).join('');
        }
        list.innerHTML = html;
    } catch (err) {
        console.error('Failed to load templates:', err);
    }
}

function createTemplate() {
    showModal('Create Template', `
        <div style="margin-bottom:0.5rem">
            <label style="font-size:0.8rem;color:var(--text-secondary)">Template name:</label>
            <input id="new-template-name" type="text" placeholder="my_template" style="width:100%;padding:0.3rem;margin-top:0.2rem">
        </div>
        <textarea id="template-content" class="code-editor"><poml>
  <task>
    <p>Your template content here</p>
  </task>
</poml></textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewTemplate()">Create</button>
    `);
}

async function saveNewTemplate() {
    const name = document.getElementById('new-template-name').value.trim();
    const content = document.getElementById('template-content').value;
    if (!name) { alert('Name is required'); return; }
    await apiFetch('/api/templates', {
        method: 'POST',
        body: JSON.stringify({ name, content })
    });
    closeModal();
    loadTemplates();
}

async function deleteTemplate(name) {
    if (!confirm(`Delete template "${name}"?`)) return;
    await apiFetch(`/api/templates/${name}`, { method: 'DELETE' });
    loadTemplates();
}

async function editTemplate(name) {
    const res = await apiFetch(`/api/templates/${name}`);
    const tpl = await res.json();

    // Load users for dropdown
    let userOptions = '<option value="">No user (skip preview)</option>';
    try {
        const ctxRes = await apiFetch('/api/contexts');
        if (ctxRes.ok) {
            const ctxData = await ctxRes.json();
            const users = (ctxData.contexts || []).map(c => c.user_id).filter(Boolean);
            userOptions += users.map(uid => `<option value="${escapeHtml(uid)}">${escapeHtml(uid)}</option>`).join('');
        }
    } catch (e) { /* ignore */ }

    showModal('Edit Template: ' + name, `
        <div style="margin-bottom:0.5rem">
            <label style="font-size:0.8rem;color:var(--text-secondary)">Preview as user:</label>
            <select id="template-preview-user" style="width:100%;padding:0.3rem;margin-top:0.2rem">${userOptions}</select>
        </div>
        <textarea id="template-content" class="code-editor">${tpl.content || ''}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveTemplate('${name}')">Save & Preview</button>
        <div id="template-preview" style="margin-top:1rem;display:none"></div>
    `);
}

async function saveTemplate(name) {
    const content = document.getElementById('template-content').value;
    const userId = document.getElementById('template-preview-user')?.value || '';
    const previewEl = document.getElementById('template-preview');

    const res = await apiFetch(`/api/templates/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ content, user_id: userId })
    });
    const data = await res.json();

    if (previewEl) {
        previewEl.style.display = 'block';
        if (data.rendered_preview) {
            previewEl.innerHTML = `<div style="font-size:0.75rem;color:var(--text-secondary);margin-bottom:0.3rem">Rendered preview:</div><pre style="background:var(--bg-tertiary,#1a1a2e);padding:0.75rem;border-radius:6px;white-space:pre-wrap;font-size:0.8rem;max-height:400px;overflow:auto">${escapeHtml(data.rendered_preview)}</pre>`;
        } else if (data.error) {
            previewEl.innerHTML = `<div style="color:var(--error,#f44)">${escapeHtml(data.error)}</div>`;
        }
    }

    if (data.success) {
        loadTemplates();
    }
}

async function loadTools() {
    try {
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
                    <span class="name">${escapeHtml(t.name)}</span>
                    <span class="meta">${escapeHtml(t.description || '')}</span>
                </div>
                <div class="toggle ${t.is_enabled ? 'active' : ''}" onclick="toggleTool('${escapeHtml(t.name)}', ${!t.is_enabled})"></div>
            </div>
        `).join('');
    } catch (err) {
        console.error('Failed to load tools:', err);
    }
}

async function toggleTool(name, enabled) {
    await apiFetch(`/api/tools/${name}`, {
        method: 'PUT',
        body: JSON.stringify({ is_enabled: enabled })
    });
    loadTools();
}

async function loadSecrets() {
    try {
        const res = await apiFetch('/api/secrets');
        const data = await res.json();
        const container = document.getElementById('secrets-content');

        const knownKeys = ['discord_bot_token', 'openai_api_key', 'anthropic_api_key', 'minimax_api_key',
            'mimo_api_key', 'elevenlabs_api_key', 'gateway_api_key', 'dashboard_admin_password'];
        const customKeys = Object.keys(data).filter(k => !knownKeys.includes(k));

        const builtInHtml = knownKeys.map(key => `
            <div class="data-item">
                <span class="name">${escapeHtml(key)}</span>
                <span class="meta">${escapeHtml(data[key] || 'Not set')}</span>
            </div>
        `).join('');

        const customHtml = customKeys.length > 0
            ? `<h3 style="margin-top:1rem">Custom Secrets</h3>
               <div class="data-list">
                ${customKeys.map(key => `
                    <div class="data-item">
                        <span class="name">${escapeHtml(key)}</span>
                        <span class="meta">${escapeHtml(data[key] || 'Not set')}</span>
                        <button class="btn btn-sm btn-danger" style="margin-left:auto" onclick="deleteCustomSecret('${escapeHtml(key)}')">Delete</button>
                    </div>
                `).join('')}
               </div>`
            : '';

        container.innerHTML = `
            <div class="data-list">${builtInHtml}</div>
            ${customHtml}
            <div style="margin-top:1.5rem">
                <h3>Update Secret</h3>
                <div class="form-group">
                    <label for="secret-field">Field</label>
                    <select id="secret-field" style="width:100%;padding:0.75rem 1rem;background:var(--bg-secondary);border:1px solid var(--border);border-radius:8px;color:var(--text-primary);font-size:1rem">
                        ${knownKeys.map(k => `<option value="${k}">${escapeHtml(k)}</option>`).join('')}
                        ${customKeys.map(k => `<option value="${k}">${escapeHtml(k)} (custom)</option>`).join('')}
                    </select>
                </div>
                <div class="form-group">
                    <label for="secret-value">New Value</label>
                    <input type="password" id="secret-value" placeholder="Enter new value">
                </div>
                <div class="form-group">
                    <label for="secret-master">Master Password (required to save to disk)</label>
                    <input type="password" id="secret-master" placeholder="Enter MASTER_KEY">
                </div>
                <button class="btn btn-primary" onclick="saveSecret()">Save Secret</button>
                <p id="secret-msg" class="hidden" style="margin-top:0.5rem"></p>
            </div>
            <div style="margin-top:1.5rem">
                <h3>Add Custom Secret</h3>
                <div class="form-group">
                    <label for="custom-secret-key">Key Name</label>
                    <input type="text" id="custom-secret-key" placeholder="e.g. MY_API_KEY">
                </div>
                <div class="form-group">
                    <label for="custom-secret-value">Value</label>
                    <input type="password" id="custom-secret-value" placeholder="Enter value">
                </div>
                <div class="form-group">
                    <label for="custom-secret-master">Master Password</label>
                    <input type="password" id="custom-secret-master" placeholder="Enter MASTER_KEY">
                </div>
                <button class="btn btn-primary" onclick="addCustomSecret()">Add Custom Secret</button>
                <p id="custom-secret-msg" class="hidden" style="margin-top:0.5rem"></p>
            </div>
        `;
    } catch (err) {
        console.error('Failed to load secrets:', err);
    }
}

async function saveSecret() {
    const field = document.getElementById('secret-field').value;
    const value = document.getElementById('secret-value').value;
    const master = document.getElementById('secret-master').value;
    const msgEl = document.getElementById('secret-msg');

    if (!value) {
        msgEl.textContent = 'Value is required';
        msgEl.style.color = 'var(--error)';
        msgEl.classList.remove('hidden');
        return;
    }

    const body = { [field]: value };
    if (master) body.master_password = master;

    try {
        const res = await apiFetch('/api/secrets', {
            method: 'PUT',
            body: JSON.stringify(body)
        });
        const text = await res.text();
        msgEl.textContent = text;
        msgEl.style.color = 'var(--success)';
        msgEl.classList.remove('hidden');
        document.getElementById('secret-value').value = '';
        document.getElementById('secret-master').value = '';
        setTimeout(() => loadSecrets(), 1500);
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

    if (!key || !value) {
        msgEl.textContent = 'Key and value are required';
        msgEl.style.color = 'var(--error)';
        msgEl.classList.remove('hidden');
        return;
    }

    const body = { [key]: value };
    if (master) body.master_password = master;

    try {
        const res = await apiFetch('/api/secrets', {
            method: 'PUT',
            body: JSON.stringify(body)
        });
        const text = await res.text();
        msgEl.textContent = text;
        msgEl.style.color = 'var(--success)';
        msgEl.classList.remove('hidden');
        document.getElementById('custom-secret-key').value = '';
        document.getElementById('custom-secret-value').value = '';
        document.getElementById('custom-secret-master').value = '';
        setTimeout(() => loadSecrets(), 1500);
    } catch (err) {
        msgEl.textContent = 'Failed: ' + err.message;
        msgEl.style.color = 'var(--error)';
        msgEl.classList.remove('hidden');
    }
}

async function deleteCustomSecret(key) {
    if (!confirm(`Delete custom secret "${key}"?`)) return;
    try {
        await apiFetch('/api/secrets', {
            method: 'PUT',
            body: JSON.stringify({ [key]: '' })
        });
        loadSecrets();
    } catch (err) {
        alert('Failed to delete: ' + err.message);
    }
}

async function loadPairings() {
    try {
        const [pairingsRes, pendingRes] = await Promise.all([
            apiFetch('/api/pairings'),
            apiFetch('/api/pairings/pending')
        ]);
        const data = await pairingsRes.json();
        const pendingData = await pendingRes.json();
        const list = document.getElementById('pairings-list');
        const pendingList = document.getElementById('pending-pairings-list');

        if (!data.pairings || data.pairings.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No pairings found</span></div>';
        } else {
            list.innerHTML = data.pairings.map(p => `
                <div class="data-item">
                    <div>
                        <span class="name">${escapeHtml(p.user_id)}</span>
                        <span class="meta">Discord: ${escapeHtml(p.discord_user_id)} | Paired: ${escapeHtml(p.paired_at || '-')}</span>
                    </div>
                    <div class="actions">
                        <button class="btn btn-sm btn-danger" onclick="deletePairing('${escapeHtml(p.user_id)}')">Delete</button>
                    </div>
                </div>
            `).join('');
        }

        if (!pendingData.pending_pairings || pendingData.pending_pairings.length === 0) {
            pendingList.innerHTML = '<div class="data-item"><span class="name">No pending pairings</span></div>';
        } else {
            pendingList.innerHTML = pendingData.pending_pairings.map(p => `
                <div class="data-item">
                    <div>
                        <span class="name">${escapeHtml(p.code)}</span>
                        <span class="meta">Discord: ${escapeHtml(p.discord_user_id)} | Expires: ${escapeHtml(p.expires_at)}</span>
                    </div>
                    <div class="actions">
                        <button class="btn btn-sm btn-primary" onclick="approvePendingPairing('${escapeHtml(p.code)}')">Approve</button>
                        <button class="btn btn-sm btn-danger" onclick="deletePendingPairing('${escapeHtml(p.code)}')">Delete</button>
                    </div>
                </div>
            `).join('');
        }
    } catch (err) {
        console.error('Failed to load pairings:', err);
    }
}

async function deletePairing(userId) {
    if (!confirm('Delete this pairing?')) return;
    await apiFetch(`/api/pairings/${userId}`, { method: 'DELETE' });
    loadPairings();
}

async function approvePendingPairing(code) {
    if (!confirm('Approve this pending pairing?')) return;
    const res = await apiFetch(`/api/pairings/pending/${code}/approve`, { method: 'POST' });
    const text = await res.text();
    alert(text);
    loadPairings();
}

async function deletePendingPairing(code) {
    if (!confirm('Delete this pending pairing?')) return;
    await apiFetch(`/api/pairings/pending/${code}`, { method: 'DELETE' });
    loadPairings();
}

async function loadClFiles() {
    try {
        const res = await apiFetch('/api/cl-files');
        const data = await res.json();
        const list = document.getElementById('cl-files-list');

        if (!data.cl_files || data.cl_files.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No CL files found</span></div>';
            return;
        }

        list.innerHTML = data.cl_files.map(f => `
            <div class="data-item">
                <span class="name">${escapeHtml(f.name)}</span>
                <div class="actions">
                    <button class="btn btn-sm btn-primary" onclick="editClFile('${escapeHtml(f.name)}')">Edit</button>
                </div>
            </div>
        `).join('');
    } catch (err) {
        console.error('Failed to load CL files:', err);
    }
}

async function editClFile(name) {
    const res = await apiFetch(`/api/cl-files/${name}`);
    const content = await res.text();

    showModal('Edit CL File: ' + name, `
        <textarea id="cl-content" class="code-editor">${escapeHtml(content)}</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveClFile('${name}')">Save</button>
    `);
}

async function createClFile() {
    showModal('New CL File', `
        <div class="form-group">
            <label for="new-cl-name">File Name</label>
            <input type="text" id="new-cl-name" placeholder="e.g. my_workflow.cl">
        </div>
        <textarea id="cl-content" class="code-editor" placeholder="[state start]\nmode = chat\n\n[transitions]\n-> done</textarea>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="saveNewClFile()">Create</button>
    `);
}

async function saveNewClFile() {
    let name = document.getElementById('new-cl-name').value.trim();
    const content = document.getElementById('cl-content').value;

    if (!name) {
        alert('File name is required');
        return;
    }
    if (!name.endsWith('.cl')) {
        name += '.cl';
    }

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
    try {
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
                    <span class="name">${escapeHtml(j.name)}</span>
                    <span class="meta">${escapeHtml(j.schedule)} | Runs: ${j.run_count}</span>
                </div>
                <div class="toggle ${j.enabled ? 'active' : ''}"></div>
            </div>
        `).join('');
    } catch (err) {
        console.error('Failed to load cron jobs:', err);
    }
}

async function populateUserDropdowns() {
    try {
        const res = await apiFetch('/api/contexts');
        if (!res.ok) return;
        const data = await res.json();
        const users = (data.contexts || []).map(c => c.user_id).filter(Boolean);

        for (const selectId of ['message-user-id', 'memory-user-id']) {
            const select = document.getElementById(selectId);
            if (!select) continue;
            const current = select.value;
            select.innerHTML = '<option value="">Select user...</option>' +
                users.map(uid => `<option value="${escapeHtml(uid)}">${escapeHtml(uid)}</option>`).join('');
            if (current && users.includes(current)) {
                select.value = current;
            }
        }
    } catch (err) {
        console.error('Failed to populate user dropdowns:', err);
    }
}

async function loadMessages() {
    const userId = document.getElementById('message-user-id').value;
    if (!userId) {
        alert('Please select a User');
        return;
    }

    const list = document.getElementById('messages-list');
    list.innerHTML = '<div class="data-item"><span class="name">Loading...</span></div>';

    try {
        const res = await apiFetch(`/api/messages/${encodeURIComponent(userId)}`);

        if (!res.ok) {
            const text = await res.text();
            list.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error ${res.status}: ${escapeHtml(text || 'No response body')}</span></div>`;
            return;
        }

        const data = await res.json();

        if (!data.messages || data.messages.length === 0) {
            list.innerHTML = '<div class="data-item"><span class="name">No messages found</span></div>';
            return;
        }

        list.innerHTML = data.messages.map(m => {
            let toolCallsHtml = '';
            if (m.tool_calls && m.tool_calls.length > 0) {
                toolCallsHtml = '<div class="tool-calls">' + m.tool_calls.map(tc =>
                    `<div class="tool-call"><span class="tool-name">${escapeHtml(tc.name)}</span>: <span class="tool-args">${escapeHtml(tc.arguments)}</span></div>`
                ).join('') + '</div>';
            }
            return `
            <div class="message ${m.role}">
                <div class="role">${escapeHtml(m.role)}${m.tool_call_id ? ' (tool: ' + escapeHtml(m.tool_call_id) + ')' : ''}</div>
                ${toolCallsHtml}
                <div class="content">${escapeHtml(m.content || '')}</div>
            </div>`;
        }).join('');
        const info = document.createElement('div');
        info.className = 'data-item';
        info.innerHTML = `<span class="name">${data.message_count} messages (~${data.total_tokens} tokens)</span>`;
        list.prepend(info);
    } catch (err) {
        list.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`;
    }
}

async function loadMemory() {
    const userId = document.getElementById('memory-user-id').value;
    if (!userId) {
        alert('Please select a User');
        return;
    }

    const container = document.getElementById('memory-content');
    container.innerHTML = '<div class="data-item"><span class="name">Loading...</span></div>';

    try {
        const res = await apiFetch(`/api/memory/${encodeURIComponent(userId)}`);

        if (!res.ok) {
            const text = await res.text();
            container.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error ${res.status}: ${escapeHtml(text || 'No response body')}</span></div>`;
            return;
        }

        const data = await res.json();

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
    } catch (err) {
        container.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`;
    }
}

// ── Modal ─────────────────────────────────────────────────────────────────────

function showModal(title, content) {
    closeModal();
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

// ── VM ───────────────────────────────────────────────────────────────────────

let vncRfb = null;
let vncModule = null;

function vncLog(msg, level) {
    const log = document.getElementById('vm-vnc-log');
    if (!log) return;
    const time = new Date().toLocaleTimeString();
    const colors = { info: 'var(--text-primary)', warn: '#FFD600', error: '#FF5252', success: '#4CAF50' };
    const color = colors[level] || colors.info;
    const div = document.createElement('div');
    div.innerHTML = `<span style="color:var(--text-secondary)">${time}</span> <span style="color:${color}">${escapeHtml(msg)}</span>`;
    log.appendChild(div);
    log.scrollTop = log.scrollHeight;
    console.log(`[VNC] ${msg}`);
}

async function loadVM() {
    try {
        await loadVMStatus();
        await loadVMActivity();
    } catch (err) {
        console.error('Failed to load VM:', err);
    }
}

async function loadVMStatus() {
    const container = document.getElementById('vm-status-bar');
    try {
        const res = await apiFetch('/api/vm');
        if (!res.ok) {
            container.innerHTML = '<div class="data-item"><span class="name" style="color:var(--text-secondary)">VM feature not available. Add VM=true to .env</span></div>';
            return;
        }
        const data = await res.json();
        const vms = data.vms || [];
        const config = data.config || {};

        if (vms.length === 0) {
            container.innerHTML = '<div class="data-item"><span class="name">No VMs running</span></div>';
            disconnectVNC();
        } else {
            container.innerHTML = vms.map(vm => `
                <div class="data-item">
                    <div>
                        <span class="name">${escapeHtml(vm.name)}</span>
                        <span class="meta">Status: ${escapeHtml(vm.status)} | PID: ${vm.pid || '-'} | VNC: ${vm.vnc_port || '-'} | Layout: ${(vm.keyboard_layout || 'us').toUpperCase()}${vm.current_iso ? ' | CD: ' + escapeHtml(vm.current_iso.split('/').pop()) : ''}</span>
                    </div>
                    <div class="actions">
                        <button class="btn btn-sm btn-danger" onclick="vmStop('${escapeHtml(vm.name)}')">Stop</button>
                    </div>
                </div>
            `).join('');

            const runningVm = vms.find(vm => vm.status === 'running');
            if (runningVm) {
                vncLog(`Auto-connecting to running VM: ${runningVm.name}`, 'info');
                connectVNC(runningVm.name);
            }
        }

        // Config info
        const configEl = document.getElementById('vm-config');
        if (config && config.vm_enabled !== undefined) {
            configEl.innerHTML = `
                <div class="data-item"><span class="name">VM Enabled</span><span class="meta">${config.vm_enabled ? 'Yes' : 'No'}</span></div>
                <div class="data-item"><span class="name">CPU Cores</span><span class="meta">${config.vm_cpu_cores || '-'}</span></div>
                <div class="data-item"><span class="name">RAM (MB)</span><span class="meta">${config.vm_ram_mb || '-'}</span></div>
                <div class="data-item"><span class="name">Disk Size</span><span class="meta">${config.vm_disk_size || '-'}</span></div>
                <div class="data-item"><span class="name">Architecture</span><span class="meta">${config.vm_arch || '-'}</span></div>
            `;
        }
    } catch (err) {
        container.innerHTML = `<div class="data-item"><span class="name" style="color:var(--error)">Error: ${escapeHtml(err.message)}</span></div>`;
    }
}

async function connectVNC(vmName) {
    const placeholder = document.getElementById('vm-vnc-placeholder');
    const screen = document.getElementById('vm-vnc-screen');
    const container = document.getElementById('vm-vnc-container');
    const log = document.getElementById('vm-vnc-log');
    if (log) log.innerHTML = '';

    if (!screen) {
        console.error('VNC screen element not found - try hard refresh (Ctrl+Shift+R)');
        return;
    }

    try {
        vncLog(`Starting VNC connection to VM: ${vmName}`, 'info');

        vncLog('Loading noVNC module...', 'info');
        if (!vncModule) {
            vncModule = await import('/static/novnc/core/rfb.js');
            vncLog('noVNC module loaded successfully', 'success');
        } else {
            vncLog('noVNC module already loaded', 'info');
        }

        const RFB = vncModule.default || vncModule.RFB || vncModule;

        if (vncRfb) {
            vncLog('Disconnecting previous VNC session...', 'info');
            vncRfb.disconnect();
            vncRfb = null;
        }

        const wsProto = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${wsProto}//${location.host}/websockify?vm=${encodeURIComponent(vmName)}`;
        vncLog(`WebSocket URL: ${wsUrl}`, 'info');

        placeholder.textContent = 'Connecting to VNC...';
        placeholder.style.display = 'block';
        screen.innerHTML = '';
        vncLog('Creating RFB instance on screen div...', 'info');

        vncRfb = new RFB(screen, wsUrl, {
            credentials: { password: '' },
            shared: true,
            wsProtocols: ['binary'],
        });
        vncLog('RFB instance created, waiting for connection...', 'info');

        vncRfb.addEventListener('connect', () => {
            vncLog('VNC connected successfully!', 'success');
            placeholder.style.display = 'none';
        });

        vncRfb.addEventListener('disconnect', (e) => {
            const reason = e.detail?.reason || 'unknown';
            const clean = e.detail?.clean;
            vncLog(`VNC disconnected: ${reason} (clean: ${clean})`, clean ? 'warn' : 'error');
            placeholder.style.display = 'block';
            placeholder.textContent = 'VNC disconnected. Click Refresh to reconnect.';
        });

        vncRfb.addEventListener('credentialsrequired', () => {
            vncLog('VNC server requires credentials', 'warn');
        });

        vncRfb.scaleViewport = true;
        vncRfb.resizeSession = false;
        vncLog('VNC options set: scaleViewport=true, resizeSession=false', 'info');

    } catch (err) {
        vncLog(`VNC connection failed: ${err.message}`, 'error');
        console.error('Failed to connect VNC:', err);
        placeholder.textContent = 'VNC connection failed: ' + err.message;
    }
}

function disconnectVNC() {
    if (vncRfb) {
        vncLog('Disconnecting VNC...', 'info');
        vncRfb.disconnect();
        vncRfb = null;
        vncLog('VNC disconnected', 'info');
    }
    const placeholder = document.getElementById('vm-vnc-placeholder');
    const screen = document.getElementById('vm-vnc-screen');
    if (placeholder) {
        placeholder.style.display = 'block';
        placeholder.textContent = 'VM not running. Click "Start VM" to begin.';
    }
    if (screen) screen.innerHTML = '';
}

async function loadVMActivity() {
    const container = document.getElementById('vm-activity-log');
    try {
        const res = await apiFetch('/api/vm/activity');
        if (!res.ok) return;
        const data = await res.json();
        const activities = data.activities || [];

        if (activities.length === 0) {
            container.innerHTML = '<div style="color:var(--text-secondary)">No VM activity yet.</div>';
            return;
        }

        container.innerHTML = activities.map(a => {
            const time = a.created_at ? new Date(a.created_at).toLocaleTimeString() : '';
            let color = 'var(--text-primary)';
            let icon = '';
            const name = a.action || '';
            // VM-specific tools
            if (name === 'vm_shell') { color = '#00D9FF'; icon = '[SHELL]'; }
            else if (name === 'vm_keys') { color = '#FFD600'; icon = '[KEYS]'; }
            else if (name === 'vm_mouse') { color = '#FF9800'; icon = '[MOUSE]'; }
            else if (name === 'vm_screenshot') { color = '#6C63FF'; icon = '[SCREEN]'; }
            else if (name === 'vm_start') { color = '#00E676'; icon = '[START]'; }
            else if (name === 'vm_stop') { color = '#FF5252'; icon = '[STOP]'; }
            else if (name === 'vm_file_transfer') { color = '#E040FB'; icon = '[FILE]'; }
            else if (name === 'vm_snapshot') { color = '#7C4DFF'; icon = '[SNAP]'; }
            else if (name === 'vm_shared_folder') { color = '#00BCD4'; icon = '[SHARE]'; }
            // Host tools
            else if (name === 'execute_terminal') { color = '#4CAF50'; icon = '[TERM]'; }
            else if (name === 'write_file') { color = '#2196F3'; icon = '[WRITE]'; }
            else if (name === 'edit_file') { color = '#2196F3'; icon = '[EDIT]'; }
            else if (name === 'read_file') { color = '#90CAF9'; icon = '[READ]'; }
            else if (name === 'set_context' || name === 'get_context') { color = '#78909C'; icon = '[CTX]'; }
            else if (name.startsWith('agent_')) { color = '#FFA726'; icon = '[AGENT]'; }
            else if (name.startsWith('discord_')) { color = '#7289DA'; icon = '[DISCORD]'; }
            else if (name.startsWith('learn_')) { color = '#CE93D8'; icon = '[LEARN]'; }
            else { icon = '[TOOL]'; }

            // Parse input to show useful summary
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

            // Truncate output for display
            let outputPreview = (a.output || '').substring(0, 120).replace(/\n/g, ' ');

            return `<div style="margin-bottom:0.3rem;padding:0.2rem 0;border-bottom:1px solid rgba(255,255,255,0.05)">
                <span style="color:var(--text-secondary);font-size:0.75rem">${time}</span>
                <span style="color:${color};font-weight:600;font-size:0.8rem">${icon} ${escapeHtml(name)}</span>
                <span style="color:var(--text-primary);font-size:0.8rem"> ${escapeHtml(inputSummary)}</span>
                ${outputPreview ? `<div style="color:var(--text-secondary);font-size:0.75rem;padding-left:1rem;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${escapeHtml(outputPreview)}</div>` : ''}
            </div>`;
        }).join('');
    } catch (err) {
        console.error('Failed to load VM activity:', err);
    }
}

async function vmStart() {
    // Load keyboard layout from context settings
    let savedLayout = 'us';
    try {
        const ctxRes = await apiFetch('/api/contexts/default');
        if (ctxRes.ok) {
            const ctx = await ctxRes.json();
            savedLayout = ctx.settings?.vm_keyboard_layout || 'us';
        }
    } catch (e) { /* use default */ }

    const layouts = [
        { value: 'us', label: 'US (QWERTY)' },
        { value: 'de', label: 'DE (QWERTZ)' },
        { value: 'fr', label: 'FR (AZERTY)' },
        { value: 'es', label: 'ES (Spanish)' },
        { value: 'it', label: 'IT (Italian)' },
        { value: 'gb', label: 'GB (British)' },
    ];
    const layoutOptions = layouts.map(l => 
        `<option value="${l.value}" ${l.value === savedLayout ? 'selected' : ''}>${l.label}</option>`
    ).join('');

    showModal('Start VM', `
        <div class="form-group">
            <label>VM Name</label>
            <input type="text" id="vm-start-name" value="praxis-vm" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>CPU Cores</label>
            <input type="number" id="vm-start-cpu" value="2" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>RAM (MB)</label>
            <input type="number" id="vm-start-ram" value="4096" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>Disk Size</label>
            <input type="text" id="vm-start-disk" value="40G" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>ISO Path (optional, for OS installation)</label>
            <input type="text" id="vm-start-iso" placeholder="/path/to/linux.iso" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>Keyboard Layout</label>
            <select id="vm-start-layout" style="width:100%;padding:0.5rem">
                ${layoutOptions}
            </select>
        </div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoStart()">Start VM</button>
    `);
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
        const res = await apiFetch('/api/vm/start', {
            method: 'POST',
            body: JSON.stringify(body)
        });
        const data = await res.json();
        closeModal();
        alert(data.message || data.error || 'Done');
        loadVM();
    } catch (err) {
        alert('Failed: ' + err.message);
    }
}

async function vmStop(name) {
    name = name || 'praxis-vm';
    if (!confirm(`Stop VM "${name}"?`)) return;
    try {
        const res = await apiFetch('/api/vm/stop', {
            method: 'POST',
            body: JSON.stringify({ name })
        });
        const data = await res.json();
        alert(data.message || data.error || 'Stopped');
        loadVM();
    } catch (err) {
        alert('Failed: ' + err.message);
    }
}

async function vmRefresh() {
    await loadVM();
}

async function vmCreateSnapshot() {
    const name = prompt('Snapshot name:');
    if (!name) return;
    try {
        const res = await apiFetch('/api/vm/snapshot', {
            method: 'POST',
            body: JSON.stringify({ snapshot_name: name })
        });
        const data = await res.json();
        alert(data.message || data.error || 'Snapshot created');
    } catch (err) {
        alert('Failed: ' + err.message);
    }
}

async function vmAddSharedFolder() {
    showModal('Add Shared Folder', `
        <div class="form-group">
            <label>Host Path</label>
            <input type="text" id="vm-sf-host" placeholder="/home/user/projects" style="width:100%;padding:0.5rem">
        </div>
        <div class="form-group">
            <label>Mount Point (in VM)</label>
            <input type="text" id="vm-sf-mount" value="/mnt/projects" style="width:100%;padding:0.5rem">
        </div>
        <button class="btn btn-primary" style="margin-top:1rem" onclick="vmDoAddSharedFolder()">Add</button>
    `);
}

async function vmDoAddSharedFolder() {
    const host_path = document.getElementById('vm-sf-host').value;
    const mount_point = document.getElementById('vm-sf-mount').value;
    if (!host_path) { alert('Host path required'); return; }
    try {
        const res = await apiFetch('/api/vm/shared-folder', {
            method: 'POST',
            body: JSON.stringify({ host_path, mount_point })
        });
        const data = await res.json();
        closeModal();
        alert(data.message || data.error || 'Shared folder added');
    } catch (err) {
        alert('Failed: ' + err.message);
    }
}

function vmShowSetup() {
    alert('To configure VM settings, add these to your .env file:\n\nVM_ENABLED=true\nVM_CPU_CORES=2\nVM_RAM_MB=4096\nVM_DISK_SIZE=40G\nVM_ARCH=x86_64\nVM_MODE=shared\n\nThen restart Praxis.');
}

async function vmReboot() {
    if (!confirm('Reboot VM?')) return;
    try {
        const res = await apiFetch('/api/vm/reboot', { method: 'POST', body: JSON.stringify({ name: 'praxis-vm' }) });
        const data = await res.json();
        if (data.error) {
            alert('Reboot failed: ' + data.error);
        } else {
            alert(data.message || 'VM rebooting...');
        }
        loadVM();
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmInsertCD() {
    const iso = prompt('Path to ISO file:');
    if (!iso) return;
    try {
        const res = await apiFetch('/api/vm/cd', {
            method: 'POST',
            body: JSON.stringify({ name: 'praxis-vm', iso_path: iso })
        });
        const data = await res.json();
        alert(data.message || data.error || 'CD inserted');
    } catch (err) { alert('Failed: ' + err.message); }
}

async function vmEjectCD() {
    try {
        const res = await apiFetch('/api/vm/cd', {
            method: 'POST',
            body: JSON.stringify({ name: 'praxis-vm', iso_path: null })
        });
        const data = await res.json();
        alert(data.message || data.error || 'CD ejected');
    } catch (err) { alert('Failed: ' + err.message); }
}

// ── Event Listeners ───────────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
    // Check if already logged in
    if (authToken) {
        validateTokenAndLoad();
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
