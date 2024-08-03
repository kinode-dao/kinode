import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { AppInfo, MirrorCheckFile, PackageManifest } from '../types/Apps'
import { HTTP_STATUS } from '../constants/http'
import { appId } from '../utils/app'
import KinodeClientApi from "@kinode/client-api";
import { WEBSOCKET_URL } from '../utils/ws'

const BASE_URL = '/main:app_store:sys'

interface AppsStore {
  apps: AppInfo[]
  ws: KinodeClientApi
  downloads: Record<string, [number, number]>
  getApps: () => Promise<void>
  getApp: (id: string) => Promise<AppInfo>
  checkMirror: (node: string) => Promise<MirrorCheckFile>
  installApp: (app: AppInfo) => Promise<void>
  updateApp: (app: AppInfo) => Promise<void>
  uninstallApp: (app: AppInfo) => Promise<void>
  downloadApp: (app: AppInfo, downloadFrom: string) => Promise<void>
  getCaps: (app: AppInfo) => Promise<PackageManifest>
  approveCaps: (app: AppInfo) => Promise<void>
  setMirroring: (app: AppInfo, mirroring: boolean) => Promise<void>
  setAutoUpdate: (app: AppInfo, autoUpdate: boolean) => Promise<void>
  rebuildIndex: () => Promise<void>
}

const useAppsStore = create<AppsStore>()(
  persist(
    (set, get) => ({
      apps: [],

      downloads: {},

      ws: new KinodeClientApi({
        uri: WEBSOCKET_URL,
        nodeId: window.our?.node,
        processId: "main:app_store:sys",
        onMessage: (message) => {
          const data = JSON.parse(message);
          if (data.kind === 'progress') {
            const appId = data.data.file_name.slice(1).replace('.zip', '');
            console.log('got app id with progress: ', appId, data.data.chunks_received, data.data.total_chunks)
            set((state) => ({
              downloads: {
                ...state.downloads,
                [appId]: [data.data.chunks_received, data.data.total_chunks]
              }
            }));

            if (data.data.chunks_received === data.data.total_chunks) {
              get().getApp(appId);
            }
          }
        },
        onOpen: (_e) => {
          console.log('open')
        },
      }),

      getApps: async () => {
        const res = await fetch(`${BASE_URL}/apps`)
        if (res.status === HTTP_STATUS.OK) {
          const apps = await res.json()
          set({ apps })
        }
      },

      getApp: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const app = await res.json()
          set(state => ({ apps: state.apps.map(a => a.package === app.package ? app : a) }))
          return app
        }
        throw new Error(`Failed to get app: ${id}`)
      },

      checkMirror: async (node: string) => {
        const res = await fetch(`${BASE_URL}/mirrorcheck/${node}`)
        if (res.status === HTTP_STATUS.OK) {
          return await res.json()
        }
        throw new Error(`Failed to check mirror status for node: ${node}`)
      },

      installApp: async (app: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/install`, { method: 'POST' })
        if (res.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to install app: ${appId(app)}`)
        }
        await get().getApp(appId(app))
      },

      updateApp: async (app: AppInfo) => {
        // Note: The backend doesn't have a specific update endpoint, so we might need to implement this differently
        throw new Error('Update functionality not implemented')
      },

      uninstallApp: async (app: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}`, { method: 'DELETE' })
        if (res.status !== HTTP_STATUS.NO_CONTENT) {
          throw new Error(`Failed to uninstall app: ${appId(app)}`)
        }
        set(state => ({ apps: state.apps.filter(a => a.package !== app.package) }))
      },

      downloadApp: async (app: AppInfo, downloadFrom: string) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/download`, {
          method: 'PUT',
          body: JSON.stringify({ download_from: downloadFrom }),
        })
        if (res.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to download app: ${appId(app)}`)
        }
      },

      getCaps: async (app: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/caps`)
        if (res.status === HTTP_STATUS.OK) {
          return await res.json()
        }
        throw new Error(`Failed to get caps for app: ${appId(app)}`)
      },

      approveCaps: async (app: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/caps`, { method: 'POST' })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to approve caps for app: ${appId(app)}`)
        }
        await get().getApp(appId(app))
      },

      setMirroring: async (app: AppInfo, mirroring: boolean) => {
        const method = mirroring ? 'PUT' : 'DELETE'
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/mirror`, { method })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to ${mirroring ? 'start' : 'stop'} mirroring: ${appId(app)}`)
        }
        await get().getApp(appId(app))
      },

      setAutoUpdate: async (app: AppInfo, autoUpdate: boolean) => {
        const method = autoUpdate ? 'PUT' : 'DELETE'
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}/auto-update`, { method })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to ${autoUpdate ? 'enable' : 'disable'} auto-update: ${appId(app)}`)
        }
        await get().getApp(appId(app))
      },

      rebuildIndex: async () => {
        const res = await fetch(`${BASE_URL}/apps/rebuild-index`, { method: 'POST' })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error('Failed to rebuild index')
        }
      },
    }),
    {
      name: 'app_store',
    }
  )
)

export default useAppsStore