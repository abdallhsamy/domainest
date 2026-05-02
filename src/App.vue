<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { openUrl } from '@tauri-apps/plugin-opener'
import { listen } from '@tauri-apps/api/event'

import type { Project } from './types/project'
import { projectsService, settingsService } from './services/projectsService'

type ViewKey = 'projects' | 'settings' | 'about'

const AUTHOR_NAME = 'Abdallah Samy'
const AUTHOR_LINKEDIN = 'https://www.linkedin.com/in/abdallah-samy'
const AUTHOR_GITHUB_REPO = 'https://github.com/abdallhsamy/domainest'

const state = reactive({
  loading: false,
  error: '' as string,
})

const projects = ref<Project[]>([])
const activeView = ref<ViewKey>('projects')
const logViewer = reactive({
  open: false,
  projectId: '' as string,
  projectName: '' as string,
  content: '' as string,
  loading: false,
})
let logTimer: number | null = null
const domainSuffix = ref('test')
const suffixState = reactive({
  saving: false,
  error: '' as string,
  warning: '' as string,
})

async function refresh() {
  state.loading = true
  state.error = ''
  try {
    projects.value = await projectsService.list()
  } catch (e) {
    state.error = String(e)
  } finally {
    state.loading = false
  }
}

async function addProject() {
  state.error = ''
  const selected = await open({ directory: true, multiple: false })
  const path = typeof selected === 'string' ? selected : null
  if (!path) return

  try {
    const p = await projectsService.add(path)
    projects.value = [...projects.value, p]
  } catch (e) {
    state.error = String(e)
  }
}

async function startStop(p: Project) {
  state.error = ''
  try {
    const updated = p.status === 'running' ? await projectsService.stop(p.id) : await projectsService.start(p.id)
    projects.value = projects.value.map((x) => (x.id === updated.id ? updated : x))
  } catch (e) {
    state.error = String(e)
  }
}

async function save(p: Project) {
  state.error = ''
  try {
    const updated = await projectsService.update(p)
    projects.value = projects.value.map((x) => (x.id === updated.id ? updated : x))
  } catch (e) {
    state.error = String(e)
  }
}

async function remove(p: Project) {
  state.error = ''
  try {
    await projectsService.remove(p.id)
    projects.value = projects.value.filter((x) => x.id !== p.id)
  } catch (e) {
    state.error = String(e)
  }
}

async function openInBrowser(p: Project) {
  state.error = ''
  try {
    await projectsService.open(p.id)
  } catch (e) {
    state.error = String(e)
  }
}

async function openLogs(p: Project) {
  logViewer.open = true
  logViewer.projectId = p.id
  logViewer.projectName = p.name
  await refreshLogs()
  if (logTimer) window.clearInterval(logTimer)
  logTimer = window.setInterval(refreshLogs, 1200)
}

async function closeLogs() {
  logViewer.open = false
  if (logTimer) window.clearInterval(logTimer)
  logTimer = null
}

async function refreshLogs() {
  if (!logViewer.open) return
  logViewer.loading = true
  try {
    logViewer.content = await projectsService.readLog(logViewer.projectId, 120_000)
  } finally {
    logViewer.loading = false
  }
}

async function updateArgsFromEvent(p: Project, e: Event) {
  const value = (e.target as HTMLInputElement | null)?.value ?? ''
  p.args = value.split(' ').filter(Boolean)
  await save(p)
}

async function openExternal(url: string) {
  try {
    await openUrl(url)
  } catch {
    window.open(url, '_blank', 'noopener,noreferrer')
  }
}

onMounted(async () => {
  await refresh()
  try {
    domainSuffix.value = await settingsService.getDomainSuffix()
  } catch {
    domainSuffix.value = 'test'
  }
  await listen<string>('ui:navigate', (e) => {
    const next = (e.payload ?? '').toLowerCase()
    if (next === 'settings') activeView.value = 'settings'
    else if (next === 'about') activeView.value = 'about'
    else activeView.value = 'projects'
  })
  await listen('ui:add_project', () => {
    activeView.value = 'projects'
    return addProject()
  })
})

async function saveSuffix() {
  suffixState.error = ''
  suffixState.warning = ''
  suffixState.saving = true
  try {
    const next = await settingsService.setDomainSuffix(domainSuffix.value)
    domainSuffix.value = next
    if (next === 'dev' || next === 'app' || next === 'local') {
      suffixState.warning = `.${next} can be problematic (HSTS or mDNS). Prefer .test unless you know the trade-offs.`
    }
  } catch (e) {
    suffixState.error = String(e)
  } finally {
    suffixState.saving = false
  }
}
</script>

<template>
  <main class="appShell">
    <header class="topbar">
      <div class="brand">
        <div class="brandTitle">Domainest</div>
        <div class="brandSub">Local domains for your dev servers.</div>
      </div>

      <div class="topbarRight">
        <nav class="segmented" aria-label="Primary navigation">
          <button class="segmentedBtn" :class="{ active: activeView === 'projects' }" @click="activeView = 'projects'">
            Projects
          </button>
          <button class="segmentedBtn" :class="{ active: activeView === 'settings' }" @click="activeView = 'settings'">
            Settings
          </button>
          <button class="segmentedBtn" :class="{ active: activeView === 'about' }" @click="activeView = 'about'">
            About
          </button>
        </nav>

        <div class="toolbar">
          <button class="btn" @click="refresh" :disabled="state.loading">Refresh</button>
          <button class="btn primary" @click="addProject">Add project</button>
        </div>
      </div>
    </header>

    <section class="content">
      <div v-if="state.error" class="alert danger">
        <div class="alertTitle">Something went wrong</div>
        <div class="alertBody">{{ state.error }}</div>
      </div>

      <div v-if="state.loading" class="muted">Loading…</div>

      <section v-if="!state.loading && activeView === 'projects'" class="projects">
        <div class="sectionHeader">
          <div class="sectionTitle">Projects</div>
          <div class="sectionHint">Start/stop dev servers and map them to friendly domains.</div>
        </div>

        <div v-if="projects.length === 0" class="empty">
          <div class="emptyTitle">No projects yet</div>
          <div class="emptyHint">Add a project folder to get a `.test` domain and optional HTTPS.</div>
          <button class="btn primary" @click="addProject">Add your first project</button>
        </div>

        <div v-else class="cardList">
          <article v-for="p in projects" :key="p.id" class="card">
            <div class="cardTop">
              <div class="cardIdentity">
                <div class="nameRow">
                  <div class="name">{{ p.name }}</div>
                  <span class="pill" :data-status="p.status">
                    {{ p.status === 'running' ? 'Running' : 'Stopped' }}
                  </span>
                </div>
                <div class="path">{{ p.path }}</div>
              </div>

              <div class="cardActions">
                <button class="btn" @click="startStop(p)">{{ p.status === 'running' ? 'Stop' : 'Start' }}</button>
                <button class="btn" @click="openInBrowser(p)">Open</button>
                <button class="btn" @click="openLogs(p)" :disabled="p.status !== 'running'">Logs</button>
                <button class="btn danger" @click="remove(p)">Remove</button>
              </div>
            </div>

            <div class="formGrid">
              <label class="field">
                <span>Domain</span>
                <input v-model="p.domain" @change="save(p)" placeholder="myapp.test" />
              </label>

              <label class="field">
                <span>Port</span>
                <input type="number" min="1" max="65535" v-model.number="p.port" @change="save(p)" />
              </label>

              <label class="field">
                <span>Command</span>
                <input v-model="p.command" @change="save(p)" placeholder="pnpm" />
              </label>

              <label class="field">
                <span>Args</span>
                <input :value="p.args.join(' ')" @change="(e) => updateArgsFromEvent(p, e)" placeholder="dev" />
              </label>

              <label class="toggle">
                <input type="checkbox" v-model="p.ssl" @change="save(p)" />
                <span class="toggleMeta">
                  <span class="toggleTitle">HTTPS (mkcert)</span>
                  <span class="toggleHint">Generate & trust a local cert for this domain.</span>
                </span>
              </label>
            </div>
          </article>
        </div>
      </section>

      <section v-if="!state.loading && activeView === 'about'" class="about">
        <div class="sectionHeader">
          <div class="sectionTitle">About</div>
          <div class="sectionHint">Domainest — local HTTPS domains and dev server orchestration.</div>
        </div>

        <div class="card aboutCard">
          <div class="aboutAvatar" aria-hidden="true">AS</div>
          <div class="aboutBody">
            <div class="aboutName">{{ AUTHOR_NAME }}</div>
            <div class="aboutRole">Author</div>
            <p class="aboutBio">
              Built to make local development feel like production: friendly domains, trusted TLS, and a tray-first workflow.
            </p>
            <div class="aboutLinks">
              <button type="button" class="btn primary" @click="openExternal(AUTHOR_LINKEDIN)">LinkedIn</button>
              <button type="button" class="btn" @click="openExternal(AUTHOR_GITHUB_REPO)">GitHub repository</button>
            </div>
            <div class="aboutUrls muted">
              <div>{{ AUTHOR_LINKEDIN }}</div>
              <div>{{ AUTHOR_GITHUB_REPO }}</div>
            </div>
          </div>
        </div>
      </section>

      <section v-if="!state.loading && activeView === 'settings'" class="settings">
        <div class="sectionHeader">
          <div class="sectionTitle">Settings</div>
          <div class="sectionHint">App-level behavior and diagnostics.</div>
        </div>

        <div class="card">
          <div class="settingsGrid">
            <div class="setting">
              <div class="settingTitle">Domain suffix (TLD)</div>
              <div class="settingHint">
                New projects default to `name.&lt;suffix&gt;`. DNS routing is configured for the selected suffix on this machine.
              </div>
              <div class="settingRow">
                <input v-model="domainSuffix" placeholder="test" spellcheck="false" />
                <button class="btn primary" @click="saveSuffix" :disabled="suffixState.saving">Save</button>
              </div>
              <div v-if="suffixState.error" class="settingError">{{ suffixState.error }}</div>
              <div v-if="suffixState.warning" class="settingWarn">{{ suffixState.warning }}</div>
            </div>
            <div class="setting">
              <div class="settingTitle">Tray / Menu bar</div>
              <div class="settingHint">Use the menu-bar icon to manage projects quickly.</div>
            </div>
            <div class="setting">
              <div class="settingTitle">HTTPS domains</div>
              <div class="settingHint">Certificates are generated per-domain when HTTPS is enabled.</div>
            </div>
            <div class="setting">
              <div class="settingTitle">DNS for `.test`</div>
              <div class="settingHint">Embedded DNS answers `*.&lt;suffix&gt;` to `127.0.0.1` (macOS uses port 53535).</div>
            </div>
          </div>
        </div>
      </section>
    </section>

    <div v-if="logViewer.open" class="modalBackdrop" @click.self="closeLogs">
      <div class="modal">
        <div class="modalTop">
          <div>
            <div class="modalTitle">Logs · {{ logViewer.projectName }}</div>
            <div class="modalHint">Live tail from `~/.dev-domains/logs/{{ logViewer.projectId }}.log`</div>
          </div>
          <div class="modalActions">
            <button class="btn" @click="refreshLogs" :disabled="logViewer.loading">Refresh</button>
            <button class="btn danger" @click="closeLogs">Close</button>
          </div>
        </div>
        <pre class="logBox">{{ logViewer.content || '(no output yet)' }}</pre>
      </div>
    </div>
  </main>
</template>

<style scoped>
.appShell {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.topbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 14px 14px;
  border: 1px solid var(--ui-border);
  border-radius: 16px;
  background: linear-gradient(180deg, rgba(255, 255, 255, 0.06), rgba(255, 255, 255, 0.02));
  backdrop-filter: blur(14px);
}

.brandTitle {
  font-size: 18px;
  font-weight: 750;
  letter-spacing: -0.02em;
}
.brandSub {
  font-size: 12px;
  color: var(--ui-muted);
  margin-top: 3px;
}

.topbarRight {
  display: flex;
  align-items: center;
  gap: 12px;
}

.segmented {
  display: inline-flex;
  padding: 3px;
  border-radius: 999px;
  border: 1px solid var(--ui-border);
  background: rgba(0, 0, 0, 0.18);
}
.segmentedBtn {
  border: 0;
  background: transparent;
  color: var(--ui-muted);
  padding: 7px 10px;
  border-radius: 999px;
  cursor: pointer;
  font-size: 13px;
  font-weight: 650;
}
.segmentedBtn.active {
  background: rgba(255, 255, 255, 0.08);
  color: var(--ui-fg);
}

.toolbar {
  display: inline-flex;
  gap: 8px;
}

.content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.muted {
  color: var(--ui-muted);
  font-size: 13px;
}

.sectionHeader {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 2px 2px 8px;
}
.sectionTitle {
  font-size: 13px;
  font-weight: 750;
  letter-spacing: -0.01em;
}
.sectionHint {
  font-size: 12px;
  color: var(--ui-muted);
}

.alert {
  border: 1px solid var(--ui-border);
  border-radius: 14px;
  padding: 12px;
  background: rgba(255, 255, 255, 0.04);
}
.alert.danger {
  border-color: rgba(255, 90, 90, 0.35);
  background: rgba(255, 90, 90, 0.08);
}
.alertTitle {
  font-weight: 750;
  font-size: 13px;
}
.alertBody {
  margin-top: 4px;
  color: var(--ui-muted);
  font-size: 12px;
  white-space: pre-wrap;
}

.empty {
  border: 1px dashed var(--ui-border);
  border-radius: 18px;
  padding: 22px;
  background: rgba(255, 255, 255, 0.02);
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: flex-start;
}
.emptyTitle {
  font-weight: 800;
  letter-spacing: -0.02em;
}
.emptyHint {
  color: var(--ui-muted);
  font-size: 12px;
  max-width: 70ch;
}

.cardList {
  display: grid;
  grid-template-columns: 1fr;
  gap: 12px;
}

.card {
  border: 1px solid var(--ui-border);
  background: rgba(255, 255, 255, 0.03);
  border-radius: 18px;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.cardTop {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.cardIdentity {
  min-width: 0;
  flex: 1;
}
.nameRow {
  display: flex;
  align-items: center;
  gap: 10px;
}
.name {
  font-weight: 800;
  letter-spacing: -0.02em;
}
.path {
  margin-top: 4px;
  font-size: 12px;
  color: var(--ui-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.pill {
  font-size: 12px;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid var(--ui-border);
  background: rgba(0, 0, 0, 0.2);
  color: var(--ui-muted);
}
.pill[data-status='running'] {
  border-color: rgba(34, 197, 94, 0.35);
  color: rgba(34, 197, 94, 0.95);
}

.cardActions {
  display: inline-flex;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.formGrid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.field span {
  font-size: 12px;
  color: var(--ui-muted);
}
.toggle {
  grid-column: 1 / -1;
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px 12px;
  border: 1px solid var(--ui-border);
  border-radius: 14px;
  background: rgba(0, 0, 0, 0.14);
}
.toggleMeta {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.toggleTitle {
  font-size: 13px;
  font-weight: 750;
}
.toggleHint {
  font-size: 12px;
  color: var(--ui-muted);
}

.settingsGrid {
  display: grid;
  grid-template-columns: 1fr;
  gap: 14px;
}
.settingTitle {
  font-weight: 800;
  letter-spacing: -0.02em;
}
.settingHint {
  margin-top: 4px;
  font-size: 12px;
  color: var(--ui-muted);
}
.settingRow {
  display: flex;
  gap: 10px;
  margin-top: 10px;
  align-items: center;
}
.settingRow input {
  flex: 1;
}
.settingError {
  margin-top: 8px;
  color: rgba(255, 77, 109, 0.95);
  font-size: 12px;
}
.settingWarn {
  margin-top: 8px;
  color: rgba(245, 158, 11, 0.95);
  font-size: 12px;
}

.aboutCard {
  flex-direction: row;
  align-items: flex-start;
  gap: 18px;
  padding: 20px;
}
.aboutAvatar {
  flex-shrink: 0;
  width: 56px;
  height: 56px;
  border-radius: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 850;
  font-size: 18px;
  letter-spacing: -0.03em;
  background: linear-gradient(135deg, rgba(124, 58, 237, 0.45), rgba(6, 182, 212, 0.35));
  border: 1px solid var(--ui-border);
}
.aboutBody {
  min-width: 0;
  flex: 1;
}
.aboutName {
  font-size: 20px;
  font-weight: 850;
  letter-spacing: -0.03em;
}
.aboutRole {
  margin-top: 2px;
  font-size: 12px;
  color: var(--ui-muted);
  font-weight: 650;
}
.aboutBio {
  margin: 12px 0 0;
  font-size: 13px;
  line-height: 1.55;
  color: var(--ui-muted);
  max-width: 62ch;
}
.aboutLinks {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin-top: 16px;
}
.aboutUrls {
  margin-top: 14px;
  font-size: 11px;
  line-height: 1.5;
  word-break: break-all;
}

.modalBackdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.55);
  backdrop-filter: blur(8px);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 18px;
}
.modal {
  width: min(980px, 100%);
  max-height: min(760px, 92vh);
  overflow: hidden;
  border-radius: 18px;
  border: 1px solid var(--ui-border);
  background: rgba(15, 17, 24, 0.92);
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px;
}
.modalTop {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}
.modalTitle {
  font-weight: 850;
  letter-spacing: -0.02em;
}
.modalHint {
  margin-top: 3px;
  color: var(--ui-muted);
  font-size: 12px;
}
.modalActions {
  display: inline-flex;
  gap: 8px;
}
.logBox {
  margin: 0;
  border-radius: 14px;
  border: 1px solid var(--ui-border);
  background: rgba(0, 0, 0, 0.35);
  padding: 12px;
  overflow: auto;
  font-family: var(--ui-mono);
  font-size: 12px;
  line-height: 1.45;
  white-space: pre-wrap;
}
</style>
