import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'
import { MyApps, AppInfo, PackageManifest } from '../types/Apps'
import { HTTP_STATUS } from '../constants/http';
import { appId, getAppType } from '../utils/app';

const BASE_URL = (import.meta as any).env.BASE_URL; // eslint-disable-line

const isApp = (a1: AppInfo, a2: AppInfo) => a1.package === a2.package && a1.publisher === a2.publisher

export interface AppsStore {
  myApps: MyApps
  listedApps: AppInfo[]
  searchResults: AppInfo[]
  query: string

  getMyApps: () => Promise<MyApps>
  getListedApps: () => Promise<AppInfo[]>
  getMyApp: (app: AppInfo) => Promise<AppInfo>
  installApp: (app: AppInfo) => Promise<void>
  updateApp: (app: AppInfo) => Promise<void>
  uninstallApp: (app: AppInfo) => Promise<void>
  getListedApp: (packageName: string) => Promise<AppInfo>
  downloadApp: (app: AppInfo, download_from: string) => Promise<void>
  getCaps: (app: AppInfo) => Promise<PackageManifest>
  approveCaps: (app: AppInfo) => Promise<void>
  setMirroring: (info: AppInfo, mirroring: boolean) => Promise<void>
  setAutoUpdate: (app: AppInfo, autoUpdate: boolean) => Promise<void>
  rebuildIndex: () => Promise<void>

  // searchApps: (query: string, onlyMyApps?: boolean) => Promise<AppInfo[]>

  get: () => AppsStore
  set: (partial: AppsStore | Partial<AppsStore>) => void
}

const useAppsStore = create<AppsStore>()(
  persist(
    (set, get) => ({
      myApps: {
        downloaded: [] as AppInfo[],
        installed: [] as AppInfo[],
        local: [] as AppInfo[],
        system: [] as AppInfo[],
      },
      listedApps: [] as AppInfo[],
      searchResults: [] as AppInfo[],
      query: '',
      getMyApps: async () => {
        const listedApps = await get().getListedApps()
        const res = await fetch(`${BASE_URL}/apps`)
        const apps = await res.json() as AppInfo[]

        const myApps = apps.reduce((acc, app) => {
          const appType = getAppType(app)

          if (listedApps.find(lapp => lapp.metadata_hash === app.metadata_hash)) {
            console.log({ listedappmatch: app })
          }
          acc[appType].push(app)
          return acc
        }, {
          downloaded: [],
          installed: [],
          local: [],
          system: [],
        } as MyApps)

        set(() => ({ myApps }))
        return myApps
      },
      getListedApps: async () => {
        const res = await fetch(`${BASE_URL}/apps/listed`)
        const apps = await res.json() as AppInfo[]
        set({ listedApps: apps })
        return apps
      },
      getMyApp: async (info: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(info)}`)
        const app = await res.json() as AppInfo
        const appType = getAppType(app)
        const myApps = get().myApps
        myApps[appType] = myApps[appType].map((a) => isApp(a, app) ? app : a)
        const listedApps = [...get().listedApps].map((a) => isApp(a, app) ? app : a)
        set({ myApps, listedApps })
        return app
      },
      installApp: async (info: AppInfo) => {
        const approveRes = await fetch(`${BASE_URL}/apps/${appId(info)}/caps`, {
          method: 'POST'
        })
        if (approveRes.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to approve caps for app: ${appId(info)}`)
        }

        const installRes = await fetch(`${BASE_URL}/apps/${appId(info)}`, {
          method: 'POST'
        })
        if (installRes.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to install app: ${appId(info)}`)
        }
      },
      updateApp: async (app: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(app)}`, {
          method: 'PUT'
        })
        if (res.status !== HTTP_STATUS.NO_CONTENT) {
          throw new Error(`Failed to update app: ${appId(app)}`)
        }

        // TODO: get the app from the server instead of updating locally
      },
      uninstallApp: async (app: AppInfo) => {
        if (!confirm(`Are you sure you want to remove ${appId(app)}?`)) return

        const res = await fetch(`${BASE_URL}/apps/${appId(app)}`, {
          method: 'DELETE'
        })
        if (res.status !== HTTP_STATUS.NO_CONTENT) {
          throw new Error(`Failed to remove app: ${appId(app)}`)
        }

        const myApps = { ...get().myApps }
        const appType = getAppType(app)
        myApps[appType] = myApps[appType].filter((a) => !isApp(a, app))
        const listedApps = get().listedApps.map((a) => isApp(a, app) ? { ...a, state: undefined, installed: false } : a)
        set({ myApps, listedApps })
      },
      getListedApp: async (packageName: string) => {
        const res = await fetch(`${BASE_URL}/apps/listed/${packageName}`)
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to get app: ${packageName}`)
        }
        const app = await res.json() as AppInfo
        return app
      },
      downloadApp: async (info: AppInfo, download_from: string) => {
        const res = await fetch(`${BASE_URL}/apps/listed/${appId(info)}`, {
          method: 'POST',
          body: JSON.stringify({ download_from }),
        })
        if (res.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to get app: ${appId(info)}`)
        }
      },
      getCaps: async (info: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(info)}/caps`)
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to get app: ${appId(info)}`)
        }

        const caps = await res.json() as PackageManifest[]
        return caps[0]
      },
      approveCaps: async (info: AppInfo) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(info)}/caps`, {
          method: 'POST'
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to get app: ${appId(info)}`)
        }
      },
      rebuildIndex: async () => {
        const res = await fetch(`${BASE_URL}/apps/rebuild-index`, {
          method: 'POST'
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error('Failed to rebuild index')
        }
      },
      setMirroring: async (info: AppInfo, mirroring: boolean) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(info)}/mirror`, {
          method: mirroring ? 'PUT' : 'DELETE',
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to start mirror: ${appId(info)}`)
        }
        get().getMyApp(info)
      },
      setAutoUpdate: async (info: AppInfo, autoUpdate: boolean) => {
        const res = await fetch(`${BASE_URL}/apps/${appId(info)}/auto-update`, {
          method: autoUpdate ? 'PUT' : 'DELETE',
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to change auto update: ${appId(info)}`)
        }
        get().getMyApp(info)
      },

      // searchApps: async (query: string, onlyMyApps = true) => {
      //   if (onlyMyApps) {
      //     const searchResults = get().myApps.filter((app) =>
      //       app.name.toLowerCase().includes(query.toLowerCase())
      //       || app.publisher.toLowerCase().includes(query.toLowerCase())
      //       || app.metadata?.name?.toLowerCase()?.includes(query.toLowerCase())
      //     )
      //     set(() => ({ searchResults }))
      //     return searchResults
      //   } else {
      //     const res = await fetch(`${BASE_URL}/apps/search/${encodeURIComponent(query)}`)
      //     const searchResults = await res.json() as AppInfo[]
      //     set(() => ({ searchResults }))
      //     return searchResults
      //   }
      // },

      get,
      set,
    }),
    {
      name: 'app_store', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useAppsStore
