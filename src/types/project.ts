export type ProjectStatus = 'running' | 'stopped'

export interface Project {
  id: string
  name: string
  path: string
  domain: string
  port: number
  ssl: boolean
  status: ProjectStatus
  command: string
  args: string[]
  pid?: number | null
}

