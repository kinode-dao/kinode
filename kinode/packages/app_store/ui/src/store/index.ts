import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { PackageState, AppListing, MirrorCheckFile, PackageManifest, DownloadItem, PackageId } from '../types/Apps'
import { HTTP_STATUS } from '../constants/http'
import KinodeClientApi from "@kinode/client-api"
import { WEBSOCKET_URL } from '../utils/ws'

const BASE_URL = '/main:app_store:sys'

interface AppsStore {
  listings: AppListing[]
  installed: PackageState[]
  downloads: Record<string, DownloadItem[]>
  ourApps: AppListing[]
  ws: KinodeClientApi
  activeDownloads: Record<string, [number, number]>

  fetchListings: () => Promise<void>
  fetchListing: (id: string) => Promise<AppListing>
  fetchInstalled: () => Promise<void>
  fetchDownloads: () => Promise<DownloadItem[]>;
  fetchOurApps: () => Promise<void>;
  fetchDownloadsForApp: (id: string) => Promise<DownloadItem[]>;
  checkMirror: (node: string) => Promise<MirrorCheckFile>

  installApp: (id: string, version_hash: string) => Promise<void>
  uninstallApp: (id: string) => Promise<void>
  downloadApp: (id: string, version_hash: string, downloadFrom: string) => Promise<void>
  getCaps: (id: string) => Promise<PackageManifest>
  approveCaps: (id: string) => Promise<void>
  setMirroring: (id: string, version_hash: string, mirroring: boolean) => Promise<void>
  setAutoUpdate: (id: string, version_hash: string, autoUpdate: boolean) => Promise<void>
}

const appId = (id: string): PackageId => {
  const [package_name, publisher_node] = id.split(":");
  return { package_name, publisher_node };
}


const useAppsStore = create<AppsStore>()(
  persist(
    (set, get): AppsStore => ({
      listings: [],
      installed: [],
      downloads: {},
      ourApps: [],
      activeDownloads: {},

      ws: new KinodeClientApi({
        uri: WEBSOCKET_URL,
        nodeId: (window as any).our?.node,
        processId: "main:app_store:sys",
        onMessage: (message) => {
          const data = JSON.parse(message);
          if (data.kind === 'progress') {
            const appId = data.data.file_name.slice(1).replace('.zip', '');
            set((state) => ({
              activeDownloads: {
                ...state.activeDownloads,
                [appId]: [data.data.chunks_received, data.data.total_chunks]
              }
            }));

            if (data.data.chunks_received === data.data.total_chunks) {
              get().fetchDownloads();
            }
          }
        },
        onOpen: (_e) => {
          console.log('WebSocket connection opened')
        },
      }),


      fetchListings: async () => {
        const res = await fetch(`${BASE_URL}/apps`)
        if (res.status === HTTP_STATUS.OK) {
          const data = await res.json()
          console.log('listings', data)
          set({ listings: data.apps || [] })
        }
      },

      fetchListing: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const listing = await res.json()
          set((state) => ({
            listings: state.listings.map(l => l.package_id === appId(id) ? listing : l)
          }))
          return listing
        }
        throw new Error(`Failed to get listing for app: ${id}`)
      },

      fetchInstalled: async () => {
        const res = await fetch(`${BASE_URL}/installed`)
        if (res.status === HTTP_STATUS.OK) {
          const installed = await res.json()
          set({ installed })
        }
      },

      fetchDownloads: async () => {
        const res = await fetch(`${BASE_URL}/downloads`)
        if (res.status === HTTP_STATUS.OK) {
          const downloads = await res.json()
          set({ downloads: { root: downloads } })
          return downloads
        }
        return []
      },

      fetchOurApps: async () => {
        const res = await fetch(`${BASE_URL}/ourapps`)
        if (res.status === HTTP_STATUS.OK) {
          const ourApps = await res.json()
          set({ ourApps })
        }
      },

      fetchDownloadsForApp: async (id: string) => {
        const res = await fetch(`${BASE_URL}/downloads/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const downloads = await res.json()
          set((state) => ({
            downloads: { ...state.downloads, [id]: downloads }
          }))
          return downloads
        }
        throw new Error(`Failed to get downloads for app: ${id}`)
      },

      checkMirror: async (node: string) => {
        const res = await fetch(`${BASE_URL}/mirrorcheck/${node}`)
        if (res.status === HTTP_STATUS.OK) {
          return await res.json()
        }
        throw new Error(`Failed to check mirror status for node: ${node}`)
      },

      installApp: async (id: string, version_hash: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}/install`, {
          method: 'POST',
          body: JSON.stringify({ version_hash })
        })
        if (res.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to install app: ${id}`)
        }
        await get().fetchInstalled()
      },

      uninstallApp: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}`, { method: 'DELETE' })
        if (res.status !== HTTP_STATUS.NO_CONTENT) {
          throw new Error(`Failed to uninstall app: ${id}`)
        }
        await get().fetchInstalled()
      },

      downloadApp: async (id: string, version_hash: string, downloadFrom: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}/download`, {
          method: 'POST',
          body: JSON.stringify({ download_from: downloadFrom, version_hash }),
        })
        if (res.status !== HTTP_STATUS.CREATED) {
          throw new Error(`Failed to download app: ${id}`)
        }
      },

      getCaps: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}/caps`)
        if (res.status === HTTP_STATUS.OK) {
          return await res.json()
        }
        throw new Error(`Failed to get caps for app: ${id}`)
      },

      approveCaps: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}/caps`, { method: 'POST' })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to approve caps for app: ${id}`)
        }
        await get().fetchListing(id)
      },

      setMirroring: async (id: string, version_hash: string, mirroring: boolean) => {
        const method = mirroring ? 'PUT' : 'DELETE'
        const res = await fetch(`${BASE_URL}/apps/${id}/mirror`, {
          method,
          body: JSON.stringify({ version_hash })
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to ${mirroring ? 'start' : 'stop'} mirroring: ${id}`)
        }
        await get().fetchListing(id)
      },

      setAutoUpdate: async (id: string, version_hash: string, autoUpdate: boolean) => {
        const method = autoUpdate ? 'PUT' : 'DELETE'
        const res = await fetch(`${BASE_URL}/apps/${id}/auto-update`, {
          method,
          body: JSON.stringify({ version_hash })
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to ${autoUpdate ? 'enable' : 'disable'} auto-update: ${id}`)
        }
        await get().fetchListing(id)
      },
    }),
    {
      name: 'app_store',
    }
  )
)

export default useAppsStore