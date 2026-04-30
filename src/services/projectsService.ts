import type { Project } from '../types/project'
import { tauriInvoke } from './tauri'

export const projectsService = {
  async list(): Promise<Project[]> {
    return tauriInvoke<Project[]>('list_projects')
  },

  async add(path: string): Promise<Project> {
    return tauriInvoke<Project>('add_project', { path })
  },

  async update(project: Project): Promise<Project> {
    return tauriInvoke<Project>('update_project', { project })
  },

  async remove(id: string): Promise<void> {
    return tauriInvoke<void>('remove_project', { id })
  },

  async start(id: string): Promise<Project> {
    return tauriInvoke<Project>('start_project', { id })
  },

  async stop(id: string): Promise<Project> {
    return tauriInvoke<Project>('stop_project', { id })
  },

  async open(id: string): Promise<void> {
    return tauriInvoke<void>('open_project', { id })
  },

  async readLog(id: string, maxBytes = 80_000): Promise<string> {
    return tauriInvoke<string>('read_project_log', { id, maxBytes })
  },
}

export const settingsService = {
  async getDomainSuffix(): Promise<string> {
    return tauriInvoke<string>('get_domain_suffix')
  },

  async setDomainSuffix(suffix: string): Promise<string> {
    return tauriInvoke<string>('set_domain_suffix', { suffix })
  },
}

