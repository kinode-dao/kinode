import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { PackageState, AppListing, MirrorCheckFile, PackageManifest, DownloadItem, PackageId } from '../types/Apps'
import { HTTP_STATUS } from '../constants/http'
import KinodeClientApi from "@kinode/client-api"
import { WEBSOCKET_URL } from '../utils/ws'

const BASE_URL = '/main:app_store:sys'

interface AppsStore {
  listings: Record<string, AppListing>
  installed: Record<string, PackageState>
  downloads: Record<string, DownloadItem[]>
  ourApps: AppListing[]
  ws: KinodeClientApi
  activeDownloads: Record<string, [number, number]>

  fetchListings: () => Promise<void>
  fetchListing: (id: string) => Promise<AppListing>
  fetchInstalled: () => Promise<void>
  fetchInstalledApp: (id: string) => Promise<PackageState | null>
  fetchDownloads: () => Promise<DownloadItem[]>
  fetchOurApps: () => Promise<void>
  fetchDownloadsForApp: (id: string) => Promise<DownloadItem[]>
  checkMirror: (node: string) => Promise<MirrorCheckFile>

  installApp: (id: string, version_hash: string) => Promise<void>
  uninstallApp: (id: string) => Promise<void>
  downloadApp: (id: string, version_hash: string, downloadFrom: string) => Promise<void>
  getCaps: (id: string) => Promise<PackageManifest>
  approveCaps: (id: string) => Promise<void>
  startMirroring: (id: string) => Promise<void>
  stopMirroring: (id: string) => Promise<void>
  setAutoUpdate: (id: string, version_hash: string, autoUpdate: boolean) => Promise<void>
}

const appId = (id: string): PackageId => {
  const [package_name, publisher_node] = id.split(":");
  return { package_name, publisher_node };
}


const useAppsStore = create<AppsStore>()(
  persist(
    (set, get): AppsStore => ({
      listings: {},
      installed: {},
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
          const data: AppListing[] = await res.json()
          const listingsMap = data.reduce((acc, listing) => {
            acc[`${listing.package_id.package_name}:${listing.package_id.publisher_node}`] = listing
            return acc
          }, {} as Record<string, AppListing>)
          set({ listings: listingsMap })
        }
      },

      fetchListing: async (id: string) => {
        const res = await fetch(`${BASE_URL}/apps/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const listing: AppListing = await res.json()
          set((state) => ({
            listings: { ...state.listings, [id]: listing }
          }))
          return listing
        }
        throw new Error(`Failed to fetch listing for app: ${id}`)
      },

      fetchInstalled: async () => {
        const res = await fetch(`${BASE_URL}/installed`)
        if (res.status === HTTP_STATUS.OK) {
          const data: PackageState[] = await res.json()
          const installedMap = data.reduce((acc, pkg) => {
            acc[`${pkg.package_id.package_name}:${pkg.package_id.publisher_node}`] = pkg
            return acc
          }, {} as Record<string, PackageState>)
          set({ installed: installedMap })
        }
      },

      fetchInstalledApp: async (id: string) => {
        const res = await fetch(`${BASE_URL}/installed/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const installedApp: PackageState = await res.json()
          set((state) => ({
            installed: { ...state.installed, [id]: installedApp }
          }))
          return installedApp
        }
        return null
      },

      fetchDownloads: async () => {
        const res = await fetch(`${BASE_URL}/downloads`)
        if (res.status === HTTP_STATUS.OK) {
          const downloads: DownloadItem[] = await res.json()
          set({ downloads: { root: downloads } })
          return downloads
        }
        return []
      },

      fetchOurApps: async () => {
        const res = await fetch(`${BASE_URL}/ourapps`)
        if (res.status === HTTP_STATUS.OK) {
          const data: AppListing[] = await res.json()
          set({ ourApps: data })
        }
      },

      fetchDownloadsForApp: async (id: string) => {
        const res = await fetch(`${BASE_URL}/downloads/${id}`)
        if (res.status === HTTP_STATUS.OK) {
          const downloads: DownloadItem[] = await res.json()
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
          return await res.json() as MirrorCheckFile
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
          return await res.json() as PackageManifest
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

      startMirroring: async (id: string) => {
        const res = await fetch(`${BASE_URL}/downloads/${id}/mirror`, {
          method: 'PUT'
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to start mirroring: ${id}`)
        }
        await get().fetchDownloadsForApp(id.split(':').slice(0, -1).join(':'))
      },

      stopMirroring: async (id: string) => {
        const res = await fetch(`${BASE_URL}/downloads/${id}/mirror`, {
          method: 'DELETE'
        })
        if (res.status !== HTTP_STATUS.OK) {
          throw new Error(`Failed to stop mirroring: ${id}`)
        }
        await get().fetchDownloadsForApp(id.split(':').slice(0, -1).join(':'))
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