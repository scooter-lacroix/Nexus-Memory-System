/**
 * Nexus Memory System - Dashboard JavaScript
 *
 * Provides:
 * - Memory CRUD operations
 * - Search functionality
 * - Real-time WebSocket updates
 * - Statistics display
 * - Hooks management
 */

// Configuration
const API_BASE = '/api/v1';
const WS_URL = `ws://${window.location.host}/ws/events`;

// State
let state = {
    memories: [],
    stats: null,
    hooks: [],
    ws: null,
    currentTab: 'memories',
    filters: {
        agent: 'general',
        category: '',
        search: ''
    }
};

// =============================================================================
// WebSocket Connection
// =============================================================================

function connectWebSocket() {
    try {
        state.ws = new WebSocket(WS_URL);

        state.ws.onopen = () => {
            console.log('WebSocket connected');
            updateConnectionStatus(true);
        };

        state.ws.onmessage = (event) => {
            const data = JSON.parse(event.data);
            handleWebSocketEvent(data);
        };

        state.ws.onclose = () => {
            console.log('WebSocket disconnected');
            updateConnectionStatus(false);
            // Reconnect after 3 seconds
            setTimeout(connectWebSocket, 3000);
        };

        state.ws.onerror = (error) => {
            console.error('WebSocket error:', error);
        };
    } catch (error) {
        console.error('Failed to connect WebSocket:', error);
        updateConnectionStatus(false);
    }
}

function handleWebSocketEvent(data) {
    console.log('WebSocket event:', data.type, data);

    switch (data.type) {
        case 'connected':
            showToast('Connected to Nexus Memory System', 'success');
            break;
        case 'memory_created':
            showToast('New memory created', 'success');
            if (state.currentTab === 'memories') {
                loadMemories();
            }
            if (state.currentTab === 'stats') {
                loadStats();
            }
            break;
        case 'memory_updated':
            showToast('Memory updated', 'info');
            if (state.currentTab === 'memories') {
                loadMemories();
            }
            break;
        case 'memory_deleted':
            showToast('Memory deleted', 'warning');
            if (state.currentTab === 'memories') {
                loadMemories();
            }
            if (state.currentTab === 'stats') {
                loadStats();
            }
            break;
        case 'session_started':
        case 'session_ended':
            if (state.currentTab === 'stats') {
                loadStats();
            }
            break;
        case 'extraction_completed':
            showToast(`Extracted ${data.data.memory_count} memories from ${data.data.agent_type}`, 'success');
            if (state.currentTab === 'hooks') {
                loadHooksStatus();
            }
            break;
    }
}

function updateConnectionStatus(connected) {
    const indicator = document.getElementById('connection-status');
    const dot = indicator.querySelector('.status-dot');
    const text = indicator.querySelector('.status-text');

    if (connected) {
        indicator.className = 'status-indicator connected';
        text.textContent = 'Connected';
    } else {
        indicator.className = 'status-indicator disconnected';
        text.textContent = 'Disconnected';
    }
}

// =============================================================================
// API Functions
// =============================================================================

async function apiRequest(endpoint, options = {}) {
    const url = `${API_BASE}${endpoint}`;
    const defaults = {
        headers: {
            'Content-Type': 'application/json'
        }
    };

    try {
        const response = await fetch(url, { ...defaults, ...options });
        const data = await response.json();

        if (!response.ok) {
            throw new Error(data.error || data.detail || 'API request failed');
        }

        return data;
    } catch (error) {
        console.error('API request error:', error);
        throw error;
    }
}

async function loadMemories() {
    const container = document.getElementById('memories-list');
    container.innerHTML = '<div class="loading">Loading memories...</div>';

    try {
        const params = new URLSearchParams({
            agent_type: state.filters.agent,
            limit: '50'
        });

        if (state.filters.category) params.append('category', state.filters.category);
        if (state.filters.search) params.append('query', state.filters.search);

        const data = await apiRequest(`/memories?${params}`);

        if (data.total === 0) {
            container.innerHTML = '<div class="empty-state">No memories found</div>';
            return;
        }

        container.innerHTML = data.results.map(memory => `
            <div class="memory-card">
                <div class="memory-header">
                    <span class="memory-id">#${memory.id}</span>
                    <div class="memory-badges">
                        <span class="badge badge-${memory.category}">${memory.category}</span>
                        ${memory.labels.map(l => `<span class="badge badge-general">${escapeHtml(l)}</span>`).join('')}
                    </div>
                </div>
                <div class="memory-content">${escapeHtml(memory.content)}</div>
                <div class="memory-meta">
                    <span>Created: ${formatDate(memory.created_at)}</span>
                    <span>Accessed: ${memory.access_count}x</span>
                </div>
                <div class="memory-actions">
                    <button class="btn btn-sm btn-secondary" onclick="viewMemory(${memory.id})">View</button>
                    <button class="btn btn-sm btn-danger" onclick="deleteMemory(${memory.id})">Delete</button>
                </div>
            </div>
        `).join('');

        state.memories = data.results;
    } catch (error) {
        container.innerHTML = `<div class="empty-state">Error loading memories: ${escapeHtml(error.message)}</div>`;
        showToast(error.message, 'error');
    }
}

async function createMemory(data) {
    try {
        const result = await apiRequest('/memories', {
            method: 'POST',
            body: JSON.stringify(data)
        });

        showToast('Memory created successfully', 'success');
        closeModal();
        loadMemories();
        loadStats();
        return result;
    } catch (error) {
        showToast(error.message, 'error');
        throw error;
    }
}

async function deleteMemory(id) {
    if (!confirm('Are you sure you want to delete this memory?')) return;

    try {
        await apiRequest(`/memories/${id}`, { method: 'DELETE' });
        showToast('Memory deleted', 'success');
        loadMemories();
        loadStats();
    } catch (error) {
        showToast(error.message, 'error');
    }
}

async function semanticSearch(query, options = {}) {
    const container = document.getElementById('search-results');
    container.innerHTML = '<div class="loading">Searching...</div>';

    try {
        const data = await apiRequest('/search/semantic', {
            method: 'POST',
            body: JSON.stringify({
                query,
                agent_type: options.agent || 'general',
                k: options.limit || 10,
                threshold: options.threshold,
                category: options.category,
                memory_lane_type: options.memory_lane_type
            })
        });

        if (!data.success || data.total === 0) {
            container.innerHTML = '<div class="empty-state">No results found</div>';
            return;
        }

        container.innerHTML = data.results.map(result => `
            <div class="memory-card">
                <div class="memory-header">
                    <span class="memory-id">#${result.id}</span>
                    <span class="badge badge-general">Similarity: ${(result.similarity * 100).toFixed(1)}%</span>
                </div>
                <div class="memory-content">${escapeHtml(result.content)}</div>
                <div class="memory-meta">
                    <span>Created: ${formatDate(result.created_at)}</span>
                </div>
            </div>
        `).join('');
    } catch (error) {
        container.innerHTML = `<div class="empty-state">Error: ${escapeHtml(error.message)}</div>`;
        showToast(error.message, 'error');
    }
}

async function loadStats() {
    try {
        const [summary, orchestrator] = await Promise.all([
            apiRequest('/stats/summary'),
            apiRequest('/stats/orchestrator')
        ]);

        document.getElementById('stat-total').textContent = summary.total_memories.toLocaleString();
        document.getElementById('stat-categories').textContent = Object.keys(summary.categories || {}).length;
        document.getElementById('stat-sessions').textContent = summary.active_sessions || '-';
        document.getElementById('stat-websockets').textContent = summary.hooks_monitoring ? '1' : '0';

        // Category chart
        const chartContainer = document.getElementById('category-chart');
        const categories = summary.categories || {};
        const maxCount = Math.max(...Object.values(categories), 1);

        chartContainer.innerHTML = Object.entries(categories)
            .sort((a, b) => b[1] - a[1])
            .map(([cat, count]) => `
                <div class="category-item">
                    <span>${escapeHtml(cat)}</span>
                    <div class="category-bar">
                        <div class="category-fill" style="width: ${(count / maxCount) * 100}%"></div>
                    </div>
                    <span>${count}</span>
                </div>
            `).join('');

        // Orchestrator status
        const orchContainer = document.getElementById('orchestrator-status');
        if (orchestrator.success) {
            orchContainer.innerHTML = `
                <p>Active Sessions: ${orchestrator.active_sessions}</p>
                <p>Events Processed: ${orchestrator.total_events_processed}</p>
            `;
        } else {
            orchContainer.innerHTML = '<p>Orchestrator not available</p>';
        }

        state.stats = summary;
    } catch (error) {
        console.error('Error loading stats:', error);
    }
}

async function loadHooksStatus() {
    const container = document.getElementById('hooks-list');
    container.innerHTML = '<div class="loading">Loading hooks status...</div>';

    try {
        const data = await apiRequest('/hooks/status?verbose=true');

        if (data.total_installed === 0) {
            container.innerHTML = '<div class="empty-state">No hooks installed</div>';
            return;
        }

        container.innerHTML = data.hooks.map(hook => `
            <div class="hook-card">
                <div class="hook-header">
                    <span class="hook-name">${escapeHtml(hook.agent_type)}</span>
                    <span class="status-badge ${hook.installed ? 'installed' : 'not-installed'}">
                        ${hook.installed ? 'Installed' : 'Not Installed'}
                    </span>
                </div>
                <div class="hook-info">
                    <span>Hook Type: ${escapeHtml(hook.hook_type)}</span>
                    <span>Extractions: ${hook.extraction_count}</span>
                    <span>Last: ${hook.last_extraction ? formatDate(hook.last_extraction) : 'Never'}</span>
                </div>
                ${hook.error_count > 0 ? `<p style="color: var(--danger); margin-top: 0.5rem;">Errors: ${hook.error_count}</p>` : ''}
            </div>
        `).join('');

        state.hooks = data.hooks;
    } catch (error) {
        container.innerHTML = `<div class="empty-state">Error loading hooks: ${escapeHtml(error.message)}</div>`;
    }
}

async function installHooks(agentType = 'all') {
    try {
        const data = await apiRequest(`/hooks/install?agent_type=${agentType}&enable_monitoring=true`, {
            method: 'POST'
        });

        if (data.success) {
            showToast('Hooks installed successfully', 'success');
            loadHooksStatus();
        } else {
            showToast('Failed to install hooks', 'error');
        }
    } catch (error) {
        showToast(error.message, 'error');
    }
}

// =============================================================================
// UI Functions
// =============================================================================

function initTabs() {
    const tabs = document.querySelectorAll('.tab');
    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            const tabName = tab.dataset.tab;
            switchTab(tabName);
        });
    });
}

function switchTab(tabName) {
    // Update active tab
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    document.querySelector(`[data-tab="${tabName}"]`).classList.add('active');

    // Update active content
    document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
    document.getElementById(`tab-${tabName}`).classList.add('active');

    state.currentTab = tabName;

    // Load data for the tab
    switch (tabName) {
        case 'memories':
            loadMemories();
            break;
        case 'stats':
            loadStats();
            break;
        case 'hooks':
            loadHooksStatus();
            break;
    }
}

function initModal() {
    const modal = document.getElementById('add-memory-modal');
    const form = document.getElementById('add-memory-form');

    // Open modal
    document.getElementById('add-memory-btn').addEventListener('click', () => {
        modal.classList.add('active');
    });

    // Close modal
    modal.querySelectorAll('.modal-close').forEach(btn => {
        btn.addEventListener('click', closeModal);
    });

    // Close on backdrop click
    modal.addEventListener('click', (e) => {
        if (e.target === modal) closeModal();
    });

    // Form submit
    form.addEventListener('submit', async (e) => {
        e.preventDefault();

        const content = document.getElementById('memory-content').value;
        const agentType = document.getElementById('memory-agent').value;
        const category = document.getElementById('memory-category').value;
        const labelsInput = document.getElementById('memory-labels').value;
        const labels = labelsInput ? labelsInput.split(',').map(l => l.trim()).filter(l => l) : [];

        await createMemory({ content, agent_type: agentType, category, labels });
    });
}

function closeModal() {
    document.getElementById('add-memory-modal').classList.remove('active');
    document.getElementById('add-memory-form').reset();
}

function initFilters() {
    document.getElementById('filter-agent').addEventListener('change', (e) => {
        state.filters.agent = e.target.value;
        loadMemories();
    });

    document.getElementById('filter-category').addEventListener('change', (e) => {
        state.filters.category = e.target.value;
        loadMemories();
    });

    document.getElementById('filter-search').addEventListener('input', debounce((e) => {
        state.filters.search = e.target.value;
        loadMemories();
    }, 300));

    document.getElementById('refresh-btn').addEventListener('click', () => {
        loadMemories();
        loadStats();
        showToast('Data refreshed', 'info');
    });
}

function initSearch() {
    const input = document.getElementById('semantic-search-input');
    const btn = document.getElementById('semantic-search-btn');

    btn.addEventListener('click', () => {
        const query = input.value.trim();
        if (!query) return;

        const agent = document.getElementById('search-agent').value;
        const limit = parseInt(document.getElementById('search-limit').value);
        const useSemantic = document.getElementById('search-semantic').checked;

        if (useSemantic) {
            semanticSearch(query, { agent, limit });
        } else {
            state.filters.search = query;
            state.filters.agent = agent;
            switchTab('memories');
        }
    });

    input.addEventListener('keypress', (e) => {
        if (e.key === 'Enter') btn.click();
    });
}

function initHooksActions() {
    document.getElementById('install-hooks-btn').addEventListener('click', () => {
        if (confirm('Install hooks for all supported agents?')) {
            installHooks('all');
        }
    });
}

function showToast(message, type = 'info') {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = `toast ${type}`;
    toast.textContent = message;
    container.appendChild(toast);

    setTimeout(() => {
        toast.remove();
    }, 3000);
}

function viewMemory(id) {
    // For now, just show the memory details
    const memory = state.memories.find(m => m.id === id);
    if (memory) {
        alert(`Memory #${id}\n\n${memory.content}`);
    }
}

// =============================================================================
// Utility Functions
// =============================================================================

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function formatDate(isoString) {
    const date = new Date(isoString);
    return date.toLocaleString();
}

function debounce(func, wait) {
    let timeout;
    return function executedFunction(...args) {
        const later = () => {
            clearTimeout(timeout);
            func(...args);
        };
        clearTimeout(timeout);
        timeout = setTimeout(later, wait);
    };
}

// =============================================================================
// Initialization
// =============================================================================

document.addEventListener('DOMContentLoaded', () => {
    initTabs();
    initModal();
    initFilters();
    initSearch();
    initHooksActions();

    // Initial data load
    loadMemories();
    loadStats();

    // Connect WebSocket
    connectWebSocket();
});

// Export for external use
window.nexusDashboard = {
    loadMemories,
    loadStats,
    loadHooksStatus,
    semanticSearch,
    createMemory,
    deleteMemory,
    installHooks
};
