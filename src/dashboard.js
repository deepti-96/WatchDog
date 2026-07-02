    let incidents = [];
    let health = null;
    let activeIncidentId = null;
    let searchQuery = '';
    let severityFilter = 'all';
    let workflowFilter = 'all';
    let sortMode = 'newest';
    let lastSyncedAt = null;
    let refreshTimer = null;
    let isExplainingIncident = false;
    let detailRequestVersion = 0;
    let knownIncidentIds = new Set();
    let loadIncidentsInFlight = null;
    let refreshDelayMs = 5000;
    let toastSequence = 0;
    let highlightedIncidentIds = new Set();
    const THEME_KEY = 'watchdog-theme';
    const NOTIFICATION_PREF_KEY = 'watchdog-browser-alerts';
    const NOTIFIED_INCIDENTS_KEY = 'watchdog-notified-incidents';
    const NOTES_DRAFT_PREFIX = 'watchdog-notes-draft:';
    const BASE_REFRESH_INTERVAL_MS = 5000;
    const MAX_REFRESH_INTERVAL_MS = 30000;
    const MAX_STORED_NOTIFICATIONS = 80;
    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

    function readStorage(key) {
      try {
        return window.localStorage.getItem(key);
      } catch (_error) {
        return null;
      }
    }

    function writeStorage(key, value) {
      try {
        window.localStorage.setItem(key, value);
        return true;
      } catch (_error) {
        return false;
      }
    }

    function removeStorage(key) {
      try {
        window.localStorage.removeItem(key);
        return true;
      } catch (_error) {
        return false;
      }
    }

    document.addEventListener('DOMContentLoaded', () => {
      applySavedTheme();
      bindThemeToggle();
      bindNotificationToggle();
      bindNotificationReset();
      bindDemoScenarios();
      applyUrlState();
      bindFilters();
      bindKeyboardShortcuts();
      bindUrlNavigation();
      bindVisibilityRefresh();
      renderEmptyDetail();
      setupRevealObserver();
      loadHealth();
      loadIncidents();
      startAutoRefresh();
    });

    async function loadHealth() {
      try {
        const response = await fetch('/healthz');
        if (!response.ok) {
          throw new Error(await response.text());
        }
        health = await response.json();
      } catch (error) {
        health = { status: 'error', storage_backend: 'unknown', explainer: 'unknown', incident_count: 0, state_dir: error.message || String(error) };
      }
      renderSystemHealth();
    }

    async function loadIncidents(options = {}) {
      if (loadIncidentsInFlight) {
        return loadIncidentsInFlight;
      }

      const { silent = false } = options;
      setSyncBusy(true);
      if (!silent) {
        renderIncidentListLoading();
      }

      loadIncidentsInFlight = (async () => {
        try {
          const previousIncidentIds = new Set(knownIncidentIds);
          const response = await fetch('/api/incidents');
          if (!response.ok) {
            throw new Error(await response.text());
          }
          incidents = await response.json();
          knownIncidentIds = new Set(incidents.map((incident) => incident.id));
          highlightedIncidentIds = previousIncidentIds.size > 0
            ? new Set(incidents.filter((incident) => !previousIncidentIds.has(incident.id)).map((incident) => incident.id))
            : new Set();
          lastSyncedAt = new Date();
          refreshDelayMs = BASE_REFRESH_INTERVAL_MS;
          await loadHealth();
          renderSidebarStats();
          renderSystemHealth();
          renderIncidentList();
          renderSyncStatus();

          if (previousIncidentIds.size > 0) {
            notifyOnNewIncidents(previousIncidentIds, incidents);
            clearIncidentHighlightsAfterDelay();
          }

          if (incidents.length) {
            const nextId = activeIncidentId && incidents.some((incident) => incident.id === activeIncidentId)
              ? activeIncidentId
              : incidents[0].id;
            if (!isExplainingIncident) {
              await selectIncident(nextId, false, silent);
            }
          } else if (!silent) {
            renderEmptyDetail();
          }
        } catch (error) {
          refreshDelayMs = Math.min(Math.max(refreshDelayMs * 2, BASE_REFRESH_INTERVAL_MS), MAX_REFRESH_INTERVAL_MS);
          renderSyncStatus(error.message || String(error));
          if (!silent) {
            renderSidebarStats();
            renderSystemHealth();
            renderIncidentListError(error);
            renderErrorDetail(error);
          }
        } finally {
          setSyncBusy(false);
          loadIncidentsInFlight = null;
        }
      })();

      return loadIncidentsInFlight;
    }

    function setSyncBusy(isBusy) {
      document.getElementById('sync-state')?.classList.toggle('busy', isBusy);
    }

    function clearIncidentHighlightsAfterDelay() {
      if (!highlightedIncidentIds.size) {
        return;
      }

      window.setTimeout(() => {
        highlightedIncidentIds = new Set();
        renderIncidentList();
      }, prefersReducedMotion ? 0 : 3600);
    }

    function applySavedTheme() {
      const savedTheme = readStorage(THEME_KEY);
      const theme = savedTheme || 'light';
      document.documentElement.setAttribute('data-theme', theme);
    }

    function bindThemeToggle() {
      const toggle = document.getElementById('theme-toggle');
      updateThemeToggleLabel(toggle);
      toggle.addEventListener('click', () => {
        const current = document.documentElement.getAttribute('data-theme') || 'light';
        const next = current === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        writeStorage(THEME_KEY, next);
        updateThemeToggleLabel(toggle);
      });
    }

    function updateThemeToggleLabel(toggle) {
      const isDark = document.documentElement.getAttribute('data-theme') === 'dark';
      const label = isDark ? 'Switch to light mode' : 'Switch to dark mode';
      toggle.setAttribute('aria-label', label);
      toggle.setAttribute('title', label);
    }

    function applyUrlState() {
      const url = new URL(window.location.href);
      searchQuery = (url.searchParams.get('q') || '').trim().toLowerCase();
      severityFilter = url.searchParams.get('severity') || 'all';
      workflowFilter = url.searchParams.get('workflow') || 'all';
      sortMode = url.searchParams.get('sort') || 'newest';
      const hash = window.location.hash.replace(/^#/, '');
      activeIncidentId = hash.startsWith('incident=') ? decodeURIComponent(hash.slice('incident='.length)) : null;
      syncFilterControls();
    }

    function syncFilterControls() {
      const search = document.getElementById('incident-search');
      const severity = document.getElementById('severity-filter');
      const workflow = document.getElementById('workflow-filter');
      const sort = document.getElementById('sort-mode');
      if (search) search.value = searchQuery;
      if (severity) severity.value = severityFilter;
      if (workflow) workflow.value = workflowFilter;
      if (sort) sort.value = sortMode;
    }

    function updateUrlState() {
      const url = new URL(window.location.href);
      setOrClearSearchParam(url, 'q', searchQuery);
      setOrClearSearchParam(url, 'severity', severityFilter === 'all' ? '' : severityFilter);
      setOrClearSearchParam(url, 'workflow', workflowFilter === 'all' ? '' : workflowFilter);
      setOrClearSearchParam(url, 'sort', sortMode === 'newest' ? '' : sortMode);
      url.hash = activeIncidentId ? `incident=${encodeURIComponent(activeIncidentId)}` : '';
      window.history.replaceState({}, '', url);
    }

    function setOrClearSearchParam(url, key, value) {
      if (value) {
        url.searchParams.set(key, value);
      } else {
        url.searchParams.delete(key);
      }
    }

    function bindUrlNavigation() {
      window.addEventListener('popstate', () => {
        syncDashboardToUrlState();
      });
      window.addEventListener('hashchange', () => {
        syncDashboardToUrlState();
      });
    }

    async function syncDashboardToUrlState() {
      applyUrlState();
      renderIncidentList();

      if (!incidents.length) {
        renderEmptyDetail();
        return;
      }

      const selectedIncidentId = activeIncidentId && incidents.some((incident) => incident.id === activeIncidentId)
        ? activeIncidentId
        : visibleIncidents()[0]?.id || null;

      if (!selectedIncidentId) {
        renderFilteredEmptyDetail();
        return;
      }

      if (selectedIncidentId !== activeIncidentId) {
        activeIncidentId = selectedIncidentId;
        renderIncidentList();
      }

      await selectIncident(selectedIncidentId, false, true);
    }

    function bindNotificationToggle() {
      const toggle = document.getElementById('notify-toggle');
      updateNotificationToggle(toggle);
      toggle.addEventListener('click', async () => {
        if (!('Notification' in window)) {
          showToast('Browser notifications are not supported in this browser.', 'warning');
          return;
        }

        if (Notification.permission === 'granted') {
          const nextEnabled = !browserNotificationsEnabled();
          writeStorage(NOTIFICATION_PREF_KEY, nextEnabled ? 'on' : 'off');
          updateNotificationToggle(toggle);
          showToast(nextEnabled ? 'Browser notifications enabled for new incidents.' : 'Browser notifications turned off. In-app alerts stay on.');
          return;
        }

        const permission = await Notification.requestPermission();
        if (permission === 'granted') {
          writeStorage(NOTIFICATION_PREF_KEY, 'on');
          updateNotificationToggle(toggle);
          showToast('Browser notifications enabled for new incidents.');
        } else {
          writeStorage(NOTIFICATION_PREF_KEY, 'off');
          updateNotificationToggle(toggle);
          showToast('Browser notifications were not enabled. In-app alerts stay on.', 'warning');
        }
      });
    }

    function browserNotificationsEnabled() {
      return readStorage(NOTIFICATION_PREF_KEY) === 'on';
    }

    function updateNotificationToggle(toggle = document.getElementById('notify-toggle')) {
      if (!toggle) {
        return;
      }

      if (!('Notification' in window)) {
        toggle.textContent = 'Alerts unsupported';
        toggle.disabled = true;
        return;
      }

      if (Notification.permission === 'granted' && browserNotificationsEnabled()) {
        toggle.textContent = 'Browser alerts on';
      } else if (Notification.permission === 'denied') {
        toggle.textContent = 'Alerts blocked';
      } else {
        toggle.textContent = 'Browser alerts off';
      }
    }

    function bindNotificationReset() {
      const reset = document.getElementById('notification-reset');
      if (!reset) {
        return;
      }

      reset.addEventListener('click', () => {
        clearNotifiedIncidentIds();
        showToast('Remembered incident alerts cleared. New incidents can notify again.');
      });
    }

    function bindDemoScenarios() {
      const checkout = document.getElementById('demo-checkout');
      const payments = document.getElementById('demo-payments');
      checkout?.addEventListener('click', () => createDemoScenario('checkout-timeout'));
      payments?.addEventListener('click', () => createDemoScenario('payments-latency'));
    }

    async function createDemoScenario(scenario) {
      const buttons = [document.getElementById('demo-checkout'), document.getElementById('demo-payments')].filter(Boolean);
      buttons.forEach((button) => {
        button.disabled = true;
        button.dataset.previousLabel = button.textContent;
      });
      const activeButton = scenario === 'payments-latency' ? document.getElementById('demo-payments') : document.getElementById('demo-checkout');
      if (activeButton) {
        activeButton.textContent = 'Generating...';
      }

      try {
        const response = await fetch('/api/demo/scenarios', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ scenario }),
        });
        const body = await response.text();
        if (!response.ok) {
          throw new Error(body);
        }
        const result = JSON.parse(body);
        activeIncidentId = result.incident_id;
        knownIncidentIds.delete(result.incident_id);
        showToast('Backend scenario generated and persisted a new incident.', 'warning', result.incident);
        await loadIncidents({ silent: true });
        await selectIncident(result.incident_id, false, true);
      } catch (error) {
        showToast(`Could not generate scenario. ${error.message || String(error)}`, 'warning');
      } finally {
        buttons.forEach((button) => {
          button.disabled = false;
          button.textContent = button.dataset.previousLabel || button.textContent;
          delete button.dataset.previousLabel;
        });
      }
    }

    function notifyOnNewIncidents(previousIncidentIds, nextIncidents) {
      const notifiedIncidentIds = readNotifiedIncidentIds();
      const newIncidents = nextIncidents.filter((incident) => !previousIncidentIds.has(incident.id) && !notifiedIncidentIds.has(incident.id));
      if (!newIncidents.length) {
        return;
      }

      newIncidents.forEach((incident) => notifiedIncidentIds.add(incident.id));
      persistNotifiedIncidentIds(notifiedIncidentIds);

      const newestIncident = newIncidents[0];
      const extraCount = newIncidents.length - 1;
      const toastMessage = extraCount > 0
        ? `${newIncidents.length} new incidents detected. Most recent: ${newestIncident.deploy_id}.`
        : `New incident detected for ${newestIncident.deploy_id}.`;

      showToast(toastMessage, newestIncident.severity === 'high' ? 'warning' : 'info', newestIncident);

      if ('Notification' in window && Notification.permission === 'granted' && browserNotificationsEnabled()) {
        const body = `${newestIncident.summary}${extraCount > 0 ? ` (+${extraCount} more)` : ''}`;
        const notification = new Notification(`watchdog: ${newestIncident.deploy_id}`, {
          body,
          tag: newestIncident.id,
        });
        notification.onclick = () => {
          window.focus();
          selectIncident(newestIncident.id, true);
          notification.close();
        };
      }
    }

    function readNotifiedIncidentIds() {
      try {
        const raw = readStorage(NOTIFIED_INCIDENTS_KEY);
        const parsed = raw ? JSON.parse(raw) : [];
        return new Set(Array.isArray(parsed) ? parsed : []);
      } catch (_error) {
        return new Set();
      }
    }

    function persistNotifiedIncidentIds(notifiedIncidentIds) {
      const trimmed = Array.from(notifiedIncidentIds).slice(-MAX_STORED_NOTIFICATIONS);
      writeStorage(NOTIFIED_INCIDENTS_KEY, JSON.stringify(trimmed));
    }

    function clearNotifiedIncidentIds() {
      removeStorage(NOTIFIED_INCIDENTS_KEY);
    }

    function showToast(message, tone = 'info', incident = null) {
      const stack = document.getElementById('toast-stack');
      if (!stack) {
        return;
      }

      const meta = incident
        ? `<div class="toast-meta"><span>${escapeHtml(incident.severity)} severity</span><span>${escapeHtml(incident.environment)}</span><span>${escapeHtml(incident.deploy_id)}</span></div>`
        : '';
      const toastId = `toast-${Date.now()}-${++toastSequence}`;

      stack.insertAdjacentHTML('beforeend', `
        <div class="toast ${tone}" data-toast-id="${toastId}" role="status">
          <strong>${incident ? 'New deploy regression captured' : 'Dashboard update'}</strong>
          <p>${escapeHtml(message)}</p>
          ${meta}
          <button class="toast-dismiss" type="button" onclick="dismissToast('${toastId}')">Dismiss</button>
        </div>
      `);

      const toast = stack.querySelector(`[data-toast-id="${toastId}"]`);
      requestAnimationFrame(() => toast?.classList.add('visible'));
      window.setTimeout(() => dismissToast(toastId), 5200);
    }

    function dismissToast(toastId) {
      const stack = document.getElementById('toast-stack');
      const toast = toastId
        ? stack?.querySelector(`[data-toast-id="${toastId}"]`)
        : stack?.firstElementChild;
      if (!toast) {
        return;
      }

      toast.classList.remove('visible');
      window.setTimeout(() => {
        toast.remove();
      }, prefersReducedMotion ? 0 : 220);
    }

    function bindVisibilityRefresh() {
      document.addEventListener('visibilitychange', () => {
        if (!document.hidden) {
          loadIncidents({ silent: true });
        }
      });
    }

    function startAutoRefresh() {
      scheduleNextRefresh();
    }

    function scheduleNextRefresh() {
      if (refreshTimer) {
        clearTimeout(refreshTimer);
      }

      refreshTimer = window.setTimeout(async () => {
        if (!document.hidden) {
          await loadIncidents({ silent: true });
        }
        scheduleNextRefresh();
      }, refreshDelayMs);
    }

    function renderSyncStatus(errorMessage = null) {
      const state = document.getElementById('sync-state');
      const time = document.getElementById('sync-time');
      if (!state || !time) {
        return;
      }

      if (errorMessage) {
        state.textContent = 'Auto-refresh retrying';
        state.classList.add('paused');
        time.textContent = `${errorMessage} · retry in ${Math.round(refreshDelayMs / 1000)}s`;
        return;
      }

      state.textContent = 'Auto-refresh on';
      state.classList.remove('paused');
      time.textContent = lastSyncedAt
        ? `Last synced ${lastSyncedAt.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', second: '2-digit' })} · next sync in ${Math.round(refreshDelayMs / 1000)}s`
        : 'Waiting for first sync';
    }

    function setupRevealObserver() {
      if (prefersReducedMotion || !('IntersectionObserver' in window)) {
        document.querySelectorAll('.reveal').forEach((element) => element.classList.add('in-view'));
        return;
      }

      const observer = new IntersectionObserver((entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add('in-view');
            observer.unobserve(entry.target);
          }
        });
      }, { threshold: 0.12, rootMargin: '0px 0px -6% 0px' });

      document.querySelectorAll('.reveal').forEach((element) => observer.observe(element));
    }

    function activateReveals(scope = document) {
      const elements = scope.querySelectorAll('.reveal');
      if (prefersReducedMotion) {
        elements.forEach((element) => element.classList.add('in-view'));
        return;
      }

      elements.forEach((element, index) => {
        requestAnimationFrame(() => {
          setTimeout(() => element.classList.add('in-view'), Math.min(index * 40, 180));
        });
      });
    }

    function bindFilters() {
      document.getElementById('incident-search').addEventListener('input', (event) => {
        searchQuery = event.target.value.trim().toLowerCase();
        updateUrlState();
        renderIncidentList();
      });

      document.getElementById('severity-filter').addEventListener('change', (event) => {
        severityFilter = event.target.value;
        updateUrlState();
        renderIncidentList();
      });

      document.getElementById('workflow-filter').addEventListener('change', (event) => {
        workflowFilter = event.target.value;
        updateUrlState();
        renderIncidentList();
      });

      document.getElementById('sort-mode').addEventListener('change', (event) => {
        sortMode = event.target.value;
        updateUrlState();
        renderIncidentList();
      });
    }

    function visibleIncidents() {
      return incidents
        .filter((incident) => {
          const haystack = [incident.deploy_id, incident.summary, incident.environment].join(' ').toLowerCase();
          const matchesSearch = !searchQuery || haystack.includes(searchQuery);
          const matchesSeverity = severityFilter === 'all'
            || (severityFilter === 'cached' ? incident.has_cached_explanation : incident.severity === severityFilter);
          const matchesWorkflow = workflowFilter === 'all' || incident.status === workflowFilter;
          return matchesSearch && matchesSeverity && matchesWorkflow;
        })
        .sort(compareIncidents);
    }

    function compareIncidents(left, right) {
      const createdDelta = new Date(right.created_at).getTime() - new Date(left.created_at).getTime();
      const severityDelta = severityRank(right.severity) - severityRank(left.severity);
      const workflowDelta = workflowRank(left.status) - workflowRank(right.status);

      switch (sortMode) {
        case 'oldest':
          return -createdDelta;
        case 'severity':
          return severityDelta || createdDelta;
        case 'open-first':
          return workflowDelta || createdDelta;
        case 'newest':
        default:
          return createdDelta || severityDelta;
      }
    }

    function severityRank(severity) {
      return severity === 'high' ? 2 : 1;
    }

    function workflowRank(status) {
      return status === 'open' ? 0 : 1;
    }

    function bindKeyboardShortcuts() {
      window.addEventListener('keydown', (event) => {
        const activeTag = document.activeElement?.tagName?.toLowerCase();
        const isEditable = document.activeElement?.isContentEditable || activeTag === 'textarea' || activeTag === 'select' || activeTag === 'input';

        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's') {
          const notes = document.getElementById('incident-notes');
          if (notes && document.activeElement === notes && activeIncidentId) {
            event.preventDefault();
            saveIncidentNotes(activeIncidentId);
          }
          return;
        }

        if (event.key === '/' && !isEditable) {
          event.preventDefault();
          document.getElementById('incident-search')?.focus();
          return;
        }

        if (isEditable) {
          return;
        }

        if (event.key === 'j') {
          event.preventDefault();
          focusAdjacentIncident(1);
          return;
        }

        if (event.key === 'k') {
          event.preventDefault();
          focusAdjacentIncident(-1);
          return;
        }

        if (event.key === 'e' && activeIncidentId) {
          event.preventDefault();
          explainIncident(activeIncidentId);
          return;
        }

        if (event.key === 'R' && activeIncidentId) {
          event.preventDefault();
          regenerateExplanation(activeIncidentId);
        }
      });
    }

    function focusAdjacentIncident(direction) {
      const visible = visibleIncidents();
      if (!visible.length) {
        return;
      }

      const currentIndex = visible.findIndex((incident) => incident.id === activeIncidentId);
      const safeIndex = currentIndex === -1 ? 0 : currentIndex;
      const nextIndex = Math.min(visible.length - 1, Math.max(0, safeIndex + direction));
      const nextIncident = visible[nextIndex];
      if (!nextIncident || nextIncident.id === activeIncidentId) {
        return;
      }

      selectIncident(nextIncident.id);
    }

    function renderSidebarStats() {
      updateMetricText('incident-count', incidents.length);
      updateMetricText('open-count', incidents.filter((incident) => incident.status === 'open').length);
      updateMetricText('high-count', incidents.filter((incident) => incident.severity === 'high').length);
      updateMetricText('cached-count', incidents.filter((incident) => incident.has_cached_explanation).length);
    }

    function updateMetricText(id, value) {
      const element = document.getElementById(id);
      if (!element) {
        return;
      }

      const nextValue = String(value);
      if (element.textContent === nextValue) {
        return;
      }

      element.textContent = nextValue;
      if (prefersReducedMotion) {
        return;
      }

      element.classList.remove('metric-bump');
      void element.offsetWidth;
      element.classList.add('metric-bump');
    }

    function renderSystemHealth() {
      const container = document.getElementById('system-health-grid');
      if (!container) {
        return;
      }

      const current = health || {
        status: 'checking',
        storage_backend: 'unknown',
        explainer: 'unknown',
        incident_count: incidents.length,
      };

      container.innerHTML = `
        ${renderHealthChip('Status', current.status || 'unknown')}
        ${renderHealthChip('Storage', current.storage_backend || 'unknown')}
        ${renderHealthChip('Explainer', current.explainer || 'unknown')}
        ${renderHealthChip('Records', String(current.incident_count ?? incidents.length))}
      `;
    }

    function renderHealthChip(label, value) {
      return `<div class="health-chip"><div class="label">${escapeHtml(label)}</div><strong>${escapeHtml(value)}</strong></div>`;
    }

    function renderIncidentListLoading() {
      const container = document.getElementById('incident-list');
      container.innerHTML = Array.from({ length: 3 }).map((_, index) => `<div class="skeleton row reveal reveal-delay-${Math.min(index + 1, 3)}" aria-hidden="true"></div>`).join('');
      activateReveals(container);
    }

    function renderIncidentListError(error) {
      const container = document.getElementById('incident-list');
      container.innerHTML = `<div class="empty reveal in-view">Unable to load incidents. ${escapeHtml(error.message || String(error))}</div>`;
    }

    function renderIncidentList() {
      const container = document.getElementById('incident-list');
      const visible = visibleIncidents();
      if (!visible.length) {
        container.innerHTML = '<div class="empty reveal in-view">No incidents match the current triage controls. Try clearing the search or relaxing the severity, workflow, or sort filters.</div>';
        if (!incidents.length) {
          return;
        }
        if (!activeIncidentId || !incidents.some((incident) => incident.id === activeIncidentId)) {
          renderFilteredEmptyDetail();
        }
        return;
      }

      container.innerHTML = visible.map((incident, index) => renderIncidentCard(incident, index)).join('');
      activateReveals(container);
    }

    function renderIncidentCard(incident, index) {
      const isActive = incident.id === activeIncidentId;
      const delayClass = `reveal-delay-${Math.min((index % 3) + 1, 3)}`;
      const isNew = highlightedIncidentIds.has(incident.id);
      return `
        <article
          class="incident-card reveal ${delayClass} ${isActive ? 'active in-view' : ''} ${isNew ? 'new' : ''}"
          onclick="selectIncident('${incident.id}')"
          onkeydown="handleIncidentKey(event, '${incident.id}')"
          tabindex="0"
          role="button"
          aria-pressed="${isActive}"
          aria-label="Open incident ${escapeHtml(incident.deploy_id)}"
        >
          <div class="incident-card-head">
            <div class="badge-row">
              ${renderBadge(incident.severity, incident.severity)}
              ${renderBadge(incident.status, incident.status)}
              ${renderBadge('environment', incident.environment)}
              ${incident.has_cached_explanation ? renderBadge('subtle', 'explained') : ''}
              ${incident.has_notes ? renderBadge('subtle', 'Notes') : ''}
            </div>
            ${renderBadge('subtle', new Date(incident.created_at).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }))}
          </div>
          <h3 style="margin-top: 10px;">${escapeHtml(incident.deploy_id)}</h3>
          <p class="meta">${new Date(incident.created_at).toLocaleString()}</p>
          <p class="meta">${escapeHtml(incident.summary)}</p>
          <div class="incident-metrics" aria-label="Incident detection metrics">
            ${renderIncidentMetric(`${incident.seconds_after_deploy}s`, 'delay')}
            ${renderIncidentMetric(`+${Number(incident.error_rate_delta).toFixed(3)}`, 'error')}
            ${renderIncidentMetric(`+${Number(incident.latency_delta_ms).toFixed(0)}ms`, 'latency')}
          </div>
        </article>
      `;
    }

    function renderIncidentMetric(value, label) {
      return `<div class="incident-metric"><strong>${escapeHtml(value)}</strong>${escapeHtml(label)}</div>`;
    }

    function handleIncidentKey(event, id) {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        selectIncident(id);
      }
    }

    function beginDetailRequest(id) {
      activeIncidentId = id;
      detailRequestVersion += 1;
      updateUrlState();
      renderIncidentList();
      return detailRequestVersion;
    }

    function isLatestDetailRequest(id, requestVersion) {
      return activeIncidentId === id && detailRequestVersion === requestVersion;
    }

    async function selectIncident(id, showLoading = true, preserveCurrentDetail = false) {
      const requestVersion = beginDetailRequest(id);
      if (showLoading) {
        renderDetailLoading();
      } else if (!preserveCurrentDetail && !isExplainingIncident) {
        renderDetailLoading();
      }

      try {
        const response = await fetch(`/api/incidents/${encodeURIComponent(id)}`);
        if (!response.ok) {
          throw new Error(await response.text());
        }
        const incident = await response.json();
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(incident, null, false, null);
      } catch (error) {
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderErrorDetail(error);
      }
    }

    function renderEmptyDetail() {
      setDetailPanelHtml(`
        <div class="empty reveal in-view">
          No incidents yet. Run a hosted scenario from the sidebar or connect the daemon to metrics and deploy events to create the first saved regression record.
        </div>
      `);
    }

    function renderFilteredEmptyDetail() {
      setDetailPanelHtml(`
        <div class="empty reveal in-view">
          No incident matches the current URL state. Adjust the triage controls or clear the filters to choose another incident.
        </div>
      `);
    }

    function renderDetailLoading() {
      setDetailPanelHtml(`
        <div class="loading-state" aria-label="Loading incident details">
          <div class="skeleton hero reveal in-view"></div>
          <div class="overview-grid">
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
            <div class="skeleton row reveal in-view"></div>
          </div>
          <div class="detail-grid">
            <div class="skeleton panel-block reveal in-view"></div>
            <div class="skeleton panel-block reveal in-view"></div>
          </div>
        </div>
      `);
    }

    function renderErrorDetail(error) {
      setDetailPanelHtml(`
        <div class="empty reveal in-view">
          Could not load this incident. ${escapeHtml(error.message || String(error))}
        </div>
      `);
    }

    function setDetailPanelHtml(html) {
      const panel = document.getElementById('detail-panel');
      if (!panel) {
        return;
      }

      if (!prefersReducedMotion) {
        panel.classList.add('detail-updating');
      }
      panel.innerHTML = html;
      requestAnimationFrame(() => panel.classList.remove('detail-updating'));
    }

    function renderDetail(incident, explanation, loading, explanationError) {
      const verdict = incident.verdict;
      const comparison = verdict.comparison;
      const signature = verdict.top_error_signature || 'No dominant error signature captured';
      const resolvedExplanation = explanation || incident.cached_explanation;
      const explanationBody = resolvedExplanation
        ? escapeHtml(resolvedExplanation)
        : explanationError
          ? `Explanation failed: ${escapeHtml(explanationError)}`
          : 'No explanation generated yet. Click "Explain Incident" to get an evidence-grounded summary and debugging steps.';
      const explanationMeta = incident.cached_explanation_updated_at
        ? `Cached ${new Date(incident.cached_explanation_updated_at).toLocaleString()}`
        : 'Generated on demand';

      setDetailPanelHtml(`
        <section class="hero reveal in-view">
          <div class="hero-copy">
            <div class="badge-row">
              ${renderBadge(incident.severity, incident.severity)}
              ${renderBadge(incident.status, incident.status)}
              ${renderBadge('environment', verdict.environment)}
              ${renderBadge('subtle', `deploy ${escapeHtml(verdict.deploy_id)}`)}
            </div>
            <h2>${escapeHtml(incident.summary)}</h2>
            <p class="subhead">WatchDog flagged this release ${verdict.seconds_after_deploy}s after deploy. The backend detector compared post-deploy behavior against the saved baseline and persisted the strongest evidence for triage.</p>
          </div>
          <div class="hero-actions">
            <button class="button button-primary" ${loading ? 'disabled' : ''} onclick="explainIncident('${incident.id}', this)">${loading ? 'Explaining…' : 'Explain Incident'}</button>
            <button class="button button-secondary" ${loading ? 'disabled' : ''} onclick="regenerateExplanation('${incident.id}', this)">${loading ? 'Refreshing…' : 'Regenerate Explanation'}</button>
            <button class="button button-secondary" onclick="loadIncidents()">Refresh Incidents</button>
            <button class="button button-secondary" onclick="copyIncidentLink('${incident.id}', this)">Copy Link</button>
            <button class="button button-secondary" onclick="copyIncidentSummary('${incident.id}', this)">Copy Summary</button>
            <a class="refresh-link" href="/api/incidents/${encodeURIComponent(incident.id)}/export/markdown">Download Markdown</a>
            <a class="refresh-link" href="/api/incidents/${encodeURIComponent(incident.id)}/export/json">Download JSON</a>
          </div>
        </section>

        <section class="signal-grid" aria-label="Primary signals">
          ${renderSignalCard('Detection Delay', `${verdict.seconds_after_deploy}s`, 'Time between deploy and detected regression', 'warning', 'reveal reveal-delay-1')}
          ${renderSignalCard('Top Error Signature', signature, verdict.top_error_count ? `Seen ${verdict.top_error_count} times after deploy` : 'No repeated new error count captured', 'danger', 'reveal reveal-delay-2')}
          ${renderSignalCard('Requests at Detection', `${comparison.request_rate_at_detection.toFixed(1)} req/s`, 'Traffic volume when the verdict was raised', '', 'reveal reveal-delay-3')}
        </section>

        <section class="overview-grid" aria-label="Incident overview">
          ${renderStatCard('Error Rate Delta', verdict.error_rate_delta.toFixed(3), 'reveal reveal-delay-1')}
          ${renderStatCard('Latency Delta', `${verdict.latency_delta_ms.toFixed(1)} ms`, 'reveal reveal-delay-2')}
          ${renderStatCard('Deploy Time', formatDateTime(verdict.deploy_timestamp), 'reveal reveal-delay-3')}
          ${renderStatCard('Detected At', formatDateTime(verdict.detected_at), 'reveal reveal-delay-3')}
        </section>

        <section class="detail-grid">
          <div>
            <article class="section-card reveal reveal-delay-1">
              <div class="section-heading">
                <h3>Operator Checklist</h3>
                <span class="muted">First-response steps</span>
              </div>
              <div class="operator-grid">
                ${renderOperatorItem(1, 'Confirm the deploy window', `Deploy ${verdict.deploy_id} in ${verdict.environment} was detected ${verdict.seconds_after_deploy}s before the alert.`)}
                ${renderOperatorItem(2, 'Compare customer-facing signals', `Error rate moved by ${verdict.error_rate_delta.toFixed(3)} and P95 latency moved by ${verdict.latency_delta_ms.toFixed(1)} ms.`)}
                ${renderOperatorItem(3, 'Inspect the dominant error', signature)}
                ${renderOperatorItem(4, 'Record the decision', 'Use notes and status to capture whether this stays open, needs rollback, or is resolved.')}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-1">
              <div class="section-heading">
                <h3>Before vs After</h3>
                <span class="muted">Baseline against detected state</span>
              </div>
              <div class="compare-grid">
                ${renderCompareCard('Error Rate', comparison.baseline_error_rate, comparison.detected_error_rate, { decimals: 3, tone: 'accent' })}
                ${renderCompareCard('P95 Latency', comparison.baseline_latency_ms, comparison.detected_latency_ms, { decimals: 1, suffix: ' ms', tone: 'warning' })}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-2">
              <div class="section-heading">
                <h3>Incident Timeline</h3>
                <span class="muted">What happened, in order</span>
              </div>
              <div class="timeline">${verdict.timeline.map((event) => renderTimelineItem(event)).join('')}</div>
            </article>
          </div>

          <div>
            <article class="section-card reveal reveal-delay-1">
              <div class="section-heading">
                <h3>Why Watchdog Flagged It</h3>
                <span class="muted">Detector verdict</span>
              </div>
              <div class="callout">${escapeHtml(incident.alert_text)}</div>
            </article>

            <article class="section-card reveal reveal-delay-2">
              <div class="section-heading">
                <h3>Detection Details</h3>
                <span class="muted">Values persisted with the incident</span>
              </div>
              <div class="evidence-table">
                ${renderEvidenceRow('Deploy id', verdict.deploy_id)}
                ${renderEvidenceRow('Environment', verdict.environment)}
                ${renderEvidenceRow('Reason', verdict.reason)}
                ${renderEvidenceRow('Request rate', `${comparison.request_rate_at_detection.toFixed(1)} req/s`)}
                ${renderEvidenceRow('New error signature', verdict.top_error_is_new ? 'yes' : 'no')}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-2">
              <div class="section-heading">
                <h3>Dominant Error Signature</h3>
                <span class="muted">Most repeated new error</span>
              </div>
              <div class="signature-chip">
                <strong>${escapeHtml(signature)}</strong>
                ${verdict.top_error_count ? `<span>Seen ${verdict.top_error_count} times after deploy</span>` : '<span>No repeated count available</span>'}
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>Incident Workflow</h3>
                <span class="muted">Track the investigation state</span>
              </div>
              <div class="badge-row">
                ${renderBadge(incident.status, incident.status)}
                ${incident.notes.trim() ? renderBadge('subtle', 'Notes saved') : renderBadge('subtle', 'No notes yet')}
              </div>
              <div class="hero-actions" style="margin-top: 14px; justify-content: flex-start;">
                <button class="button button-secondary" ${incident.status === 'open' ? 'disabled' : ''} onclick="setIncidentStatus('${incident.id}', 'open', this)">Mark Open</button>
                <button class="button button-secondary" ${incident.status === 'resolved' ? 'disabled' : ''} onclick="setIncidentStatus('${incident.id}', 'resolved', this)">Mark Resolved</button>
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>Investigation Notes</h3>
                <span class="muted">Persisted with the incident</span>
              </div>
              <textarea id="incident-notes" class="notes-box" placeholder="Capture what you found, what changed, and what to check next...">${escapeHtml(resolveIncidentNotesValue(incident))}</textarea>
              <div class="notes-actions">
                <span id="notes-draft-status" class="muted">${escapeHtml(renderNotesDraftStatus(incident))}</span>
                <button class="button button-secondary" onclick="saveIncidentNotes('${incident.id}', this)">Save Notes</button>
              </div>
            </article>

            <article class="section-card reveal reveal-delay-3">
              <div class="section-heading">
                <h3>AI Explanation</h3>
                <span class="muted">${escapeHtml(explanationMeta)}</span>
              </div>
              <pre>${explanationBody}</pre>
            </article>
          </div>
        </section>
      `);

      const panel = document.getElementById('detail-panel');
      activateReveals(panel);
      bindNotesDraft(incident);
    }

    function notesDraftStorageKey(id) {
      return `${NOTES_DRAFT_PREFIX}${id}`;
    }

    function readNotesDraft(id) {
      return readStorage(notesDraftStorageKey(id)) || '';
    }

    function persistNotesDraft(id, value) {
      if (value.trim()) {
        writeStorage(notesDraftStorageKey(id), value);
      } else {
        clearNotesDraft(id);
      }
    }

    function clearNotesDraft(id) {
      removeStorage(notesDraftStorageKey(id));
    }

    function resolveIncidentNotesValue(incident) {
      const draft = readNotesDraft(incident.id);
      return draft || incident.notes;
    }

    function renderNotesDraftStatus(incident) {
      const draft = readNotesDraft(incident.id);
      if (!draft) {
        return 'Notes are saved back into the incident file';
      }

      if (draft === incident.notes) {
        return 'Draft matches the saved notes';
      }

      return 'Draft saved locally until you press Save Notes';
    }

    function bindNotesDraft(incident) {
      const textarea = document.getElementById('incident-notes');
      const status = document.getElementById('notes-draft-status');
      if (!textarea || !status) {
        return;
      }

      updateNotesDraftState(textarea, status, incident);
      textarea.addEventListener('input', () => {
        persistNotesDraft(incident.id, textarea.value);
        updateNotesDraftState(textarea, status, incident);
      });
    }

    function updateNotesDraftState(textarea, status, incident) {
      const hasDraft = Boolean(readNotesDraft(incident.id)) && textarea.value !== incident.notes;
      textarea.classList.toggle('has-draft', hasDraft);
      status.classList.toggle('has-draft', hasDraft);
      status.textContent = renderNotesDraftStatus({ ...incident, notes: incident.notes });
    }

    async function copyIncidentLink(id, button = null) {
      setActionPending(button, true);
      try {
        const url = new URL(window.location.href);
        url.hash = `incident=${encodeURIComponent(id)}`;
        const link = url.toString();
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(link);
          showToast('Incident link copied to clipboard.');
          return;
        }

        showToast(link);
      } catch (error) {
        showToast(`Could not copy incident link. ${error.message || String(error)}`, 'warning');
      } finally {
        setActionPending(button, false);
      }
    }

    async function copyIncidentSummary(id, button = null) {
      setActionPending(button, true);
      try {
        const response = await fetch(`/api/incidents/${encodeURIComponent(id)}/summary`);
        if (!response.ok) {
          throw new Error(await response.text());
        }

        const summary = await response.text();
        if (navigator.clipboard?.writeText) {
          await navigator.clipboard.writeText(summary);
          showToast('Incident summary copied to clipboard.');
          return;
        }

        showToast(summary);
      } catch (error) {
        showToast(`Could not copy incident summary. ${error.message || String(error)}`, 'warning');
      } finally {
        setActionPending(button, false);
      }
    }

    async function setIncidentStatus(id, status, button = null) {
      const requestVersion = detailRequestVersion;
      setActionPending(button, true);
      try {
        const response = await fetch(`/api/incidents/${encodeURIComponent(id)}/status`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ status }),
        });

        if (!response.ok) {
          throw new Error(await response.text());
        }

        const incident = await response.json();
        clearNotesDraft(id);
        await loadIncidents({ silent: true });
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(incident, incident.cached_explanation, false, null);
        showToast(`Incident marked ${status}.`);
      } catch (error) {
        showToast(`Could not update incident status. ${error.message || String(error)}`, 'warning');
      } finally {
        setActionPending(button, false);
      }
    }

    async function saveIncidentNotes(id, button = null) {
      const requestVersion = detailRequestVersion;
      const notes = document.getElementById('incident-notes')?.value || '';
      setActionPending(button, true);
      try {
        const response = await fetch(`/api/incidents/${encodeURIComponent(id)}/notes`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ notes }),
        });

        if (!response.ok) {
          throw new Error(await response.text());
        }

        const incident = await response.json();
        clearNotesDraft(id);
        await loadIncidents({ silent: true });
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(incident, incident.cached_explanation, false, null);
        showToast('Investigation notes saved.');
      } catch (error) {
        showToast(`Could not save notes. ${error.message || String(error)}`, 'warning');
      } finally {
        setActionPending(button, false);
      }
    }

    async function regenerateExplanation(id, button = null) {
      let incident;
      const requestVersion = detailRequestVersion;
      isExplainingIncident = true;
      setActionPending(button, true);
      try {
        const incidentResponse = await fetch(`/api/incidents/${encodeURIComponent(id)}`);
        if (!incidentResponse.ok) {
          throw new Error(await incidentResponse.text());
        }
        incident = await incidentResponse.json();
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(incident, null, true, null);

        const explainResponse = await fetch(`/api/incidents/${encodeURIComponent(id)}/explain/refresh`, { method: 'POST' });
        const body = await explainResponse.text();
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(
          incident,
          explainResponse.ok ? JSON.parse(body).explanation : null,
          false,
          explainResponse.ok ? null : body,
        );
        await loadIncidents({ silent: true });
      } catch (error) {
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        if (incident) {
          renderDetail(incident, null, false, error.message || String(error));
        } else {
          renderErrorDetail(error);
        }
      } finally {
        isExplainingIncident = false;
        setActionPending(button, false);
      }
    }

    async function explainIncident(id, button = null) {
      let incident;
      const requestVersion = detailRequestVersion;
      isExplainingIncident = true;
      setActionPending(button, true);
      try {
        const incidentResponse = await fetch(`/api/incidents/${encodeURIComponent(id)}`);
        if (!incidentResponse.ok) {
          throw new Error(await incidentResponse.text());
        }
        incident = await incidentResponse.json();
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(incident, null, true, null);

        const explainResponse = await fetch(`/api/incidents/${encodeURIComponent(id)}/explain`, { method: 'POST' });
        const body = await explainResponse.text();
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        renderDetail(
          incident,
          explainResponse.ok ? JSON.parse(body).explanation : null,
          false,
          explainResponse.ok ? null : body,
        );
      } catch (error) {
        if (!isLatestDetailRequest(id, requestVersion)) {
          return;
        }
        if (incident) {
          renderDetail(incident, null, false, error.message || String(error));
        } else {
          renderErrorDetail(error);
        }
      } finally {
        isExplainingIncident = false;
        setActionPending(button, false);
      }
    }

    function setActionPending(button, isPending) {
      if (!button) {
        return;
      }

      button.classList.toggle('is-pending', isPending);
      button.disabled = isPending;
      button.setAttribute('aria-busy', String(isPending));
    }

    function renderBadge(kind, label) {
      return `<span class="badge ${kind}">${escapeHtml(label)}</span>`;
    }

    function renderStatCard(label, value, extraClass = '') {
      return `
        <article class="stat-card ${extraClass}">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
        </article>
      `;
    }

    function renderSignalCard(label, value, detail, tone, extraClass = '') {
      return `
        <article class="signal-card ${tone} ${extraClass}">
          <div class="label">${escapeHtml(label)}</div>
          <strong>${escapeHtml(value)}</strong>
          <p class="meta" style="margin-top: 8px;">${escapeHtml(detail)}</p>
        </article>
      `;
    }

    function renderOperatorItem(index, title, detail) {
      return `
        <div class="operator-item">
          <span class="operator-index">${index}</span>
          <div>
            <strong>${escapeHtml(title)}</strong>
            <p class="meta" style="margin-top: 4px;">${escapeHtml(detail)}</p>
          </div>
        </div>
      `;
    }

    function renderEvidenceRow(label, value) {
      return `
        <div class="evidence-row">
          <strong>${escapeHtml(label)}</strong>
          <span class="meta">${escapeHtml(value)}</span>
        </div>
      `;
    }

    function renderCompareCard(title, baselineValue, detectedValue, options = {}) {
      const { decimals = 2, suffix = '', tone = 'accent' } = options;
      const baseline = formatMetricValue(baselineValue, decimals, suffix);
      const detected = formatMetricValue(detectedValue, decimals, suffix);
      return `
        <div class="compare-card ${tone === 'warning' ? 'warning' : ''} reveal reveal-delay-1">
          <h4>${escapeHtml(title)}</h4>
          ${renderCompareChart(title, baselineValue, detectedValue, tone)}
          <div class="compare-row">
            <span class="row-label muted">Baseline</span>
            <strong>${escapeHtml(baseline)}</strong>
          </div>
          <div class="compare-row">
            <span class="row-label muted">Detected</span>
            <strong>${escapeHtml(detected)}</strong>
          </div>
        </div>
      `;
    }

    function renderCompareChart(title, baselineValue, detectedValue, tone = 'accent') {
      const points = buildTrendPoints(Number(baselineValue), Number(detectedValue));
      const width = 320;
      const height = 82;
      const left = 10;
      const right = width - 10;
      const top = 10;
      const bottom = height - 16;
      const minValue = Math.min(...points);
      const maxValue = Math.max(...points);
      const span = Math.max(maxValue - minValue, Math.max(Math.abs(maxValue) * 0.08, 0.001));
      const normalizedMin = minValue - span * 0.16;
      const normalizedMax = maxValue + span * 0.16;
      const xFor = (index) => left + ((right - left) * index / (points.length - 1));
      const yFor = (value) => {
        const ratio = (value - normalizedMin) / (normalizedMax - normalizedMin || 1);
        return bottom - ratio * (bottom - top);
      };
      const linePoints = points.map((value, index) => `${xFor(index).toFixed(1)},${yFor(value).toFixed(1)}`);
      const areaPoints = [`${left},${bottom}`, ...linePoints, `${right},${bottom}`].join(' ');
      const startX = xFor(0).toFixed(1);
      const startY = yFor(points[0]).toFixed(1);
      const endX = xFor(points.length - 1).toFixed(1);
      const endY = yFor(points[points.length - 1]).toFixed(1);
      return `
        <svg class="compare-chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(title)} trend from baseline to detected value">
          <line class="compare-chart-grid" x1="${left}" y1="${bottom}" x2="${right}" y2="${bottom}"></line>
          <line class="compare-chart-grid" x1="${left}" y1="${top}" x2="${right}" y2="${top}"></line>
          <polygon class="compare-chart-area" points="${areaPoints}"></polygon>
          <polyline class="compare-chart-line" pathLength="1" points="${linePoints.join(' ')}"></polyline>
          <circle class="compare-chart-dot" cx="${startX}" cy="${startY}" r="4.5"></circle>
          <circle class="compare-chart-dot" cx="${endX}" cy="${endY}" r="4.5"></circle>
          <text class="compare-chart-label" x="${left}" y="${height - 2}">baseline</text>
          <text class="compare-chart-label" x="${right}" y="${height - 2}" text-anchor="end">detected</text>
        </svg>
      `;
    }

    function buildTrendPoints(baselineValue, detectedValue) {
      const start = Number.isFinite(baselineValue) ? baselineValue : 0;
      const end = Number.isFinite(detectedValue) ? detectedValue : start;
      const delta = end - start;
      const shape = [0, 0.03, 0.08, 0.16, 0.34, 0.56, 0.8, 1];
      return shape.map((ratio, index) => {
        const easing = ratio * ratio * (3 - 2 * ratio);
        const sway = delta === 0 ? 0 : Math.sin(index * 1.2) * delta * 0.04;
        return start + delta * easing + sway;
      });
    }

    function formatMetricValue(value, decimals = 2, suffix = '') {
      return `${Number(value).toFixed(decimals)}${suffix}`;
    }

    function renderTimelineItem(event) {
      return `
        <article class="timeline-item reveal reveal-delay-1">
          <div class="timeline-time">${new Date(event.timestamp).toLocaleTimeString()}</div>
          <div class="timeline-content">
            <strong>${escapeHtml(event.label)}</strong>
            <div class="meta">${escapeHtml(event.detail)}</div>
          </div>
        </article>
      `;
    }

    function formatDateTime(value) {
      return new Date(value).toLocaleString([], {
        month: 'short',
        day: 'numeric',
        hour: 'numeric',
        minute: '2-digit',
      });
    }

    function escapeHtml(value) {
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }
