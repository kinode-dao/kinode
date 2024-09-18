import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { PackageState, AppListing, MirrorCheckFile, PackageManifest, DownloadItem, HomepageApp } from '../types/Apps'
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
  homepageApps: HomepageApp[]
  activeDownloads: Record<string, { downloaded: number, total: number }>

  fetchData: (id: string) => Promise<void>
  fetchListings: () => Promise<void>
  fetchListing: (id: string) => Promise<AppListing | null>
  fetchInstalled: () => Promise<void>
  fetchInstalledApp: (id: string) => Promise<PackageState | null>
  fetchDownloads: () => Promise<DownloadItem[]>
  fetchOurApps: () => Promise<void>
  fetchDownloadsForApp: (id: string) => Promise<DownloadItem[]>
  checkMirror: (node: string) => Promise<MirrorCheckFile | null>

  fetchHomepageApps: () => Promise<void>
  getLaunchUrl: (id: string) => string | null

  installApp: (id: string, version_hash: string) => Promise<void>
  uninstallApp: (id: string) => Promise<void>
  downloadApp: (id: string, version_hash: string, downloadFrom: string) => Promise<void>
  removeDownload: (packageId: string, versionHash: string) => Promise<void>
  getCaps: (id: string) => Promise<PackageManifest | null>
  approveCaps: (id: string) => Promise<void>
  startMirroring: (id: string) => Promise<void>
  stopMirroring: (id: string) => Promise<void>
  setAutoUpdate: (id: string, version_hash: string, autoUpdate: boolean) => Promise<void>

  setActiveDownload: (appId: string, downloaded: number, total: number) => void
  clearActiveDownload: (appId: string) => void
  clearAllActiveDownloads: () => void;

}

const useAppsStore = create<AppsStore>()(
  persist(
    (set, get): AppsStore => ({
      listings: {},
      installed: {},
      downloads: {},
      ourApps: [],
      activeDownloads: {},
      homepageApps: [],


      fetchData: async (id: string) => {
        if (!id) return;
        try {
          const [listing, downloads, installedApp] = await Promise.all([
            get().fetchListing(id),
            get().fetchDownloadsForApp(id),
            get().fetchInstalledApp(id)
          ]);
          set((state) => ({
            listings: listing ? { ...state.listings, [id]: listing } : state.listings,
            downloads: { ...state.downloads, [id]: downloads },
            installed: installedApp ? { ...state.installed, [id]: installedApp } : state.installed
          }));
        } catch (error) {
          console.error("Error fetching app data:", error);
        }
      },

      fetchListings: async () => {
        try {
          const res = await fetch(`${BASE_URL}/apps`);
          if (res.status === HTTP_STATUS.OK) {
            const data: AppListing[] = await res.json();
            const listingsMap = data.reduce((acc, listing) => {
              acc[`${listing.package_id.package_name}:${listing.package_id.publisher_node}`] = listing;
              return acc;
            }, {} as Record<string, AppListing>);
            set({ listings: listingsMap });
          }
        } catch (error) {
          console.error("Error fetching listings:", error);
        }
      },

      fetchListing: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}`);
          if (res.status === HTTP_STATUS.OK) {
            const listing: AppListing = await res.json();
            set((state) => ({
              listings: { ...state.listings, [id]: listing }
            }));
            return listing;
          }
        } catch (error) {
          console.error("Error fetching listing:", error);
        }
        return null;
      },

      fetchInstalled: async () => {
        try {
          const res = await fetch(`${BASE_URL}/installed`);
          if (res.status === HTTP_STATUS.OK) {
            const data: PackageState[] = await res.json();
            const installedMap = data.reduce((acc, pkg) => {
              acc[`${pkg.package_id.package_name}:${pkg.package_id.publisher_node}`] = pkg;
              return acc;
            }, {} as Record<string, PackageState>);
            set({ installed: installedMap });
          }
        } catch (error) {
          console.error("Error fetching installed apps:", error);
        }
      },

      fetchInstalledApp: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/installed/${id}`);
          if (res.status === HTTP_STATUS.OK) {
            const installedApp: PackageState = await res.json();
            set((state) => ({
              installed: { ...state.installed, [id]: installedApp }
            }));
            return installedApp;
          }
        } catch (error) {
          console.error("Error fetching installed app:", error);
        }
        return null;
      },

      fetchDownloads: async () => {
        try {
          const res = await fetch(`${BASE_URL}/downloads`);
          if (res.status === HTTP_STATUS.OK) {
            const downloads: DownloadItem[] = await res.json();
            set({ downloads: { root: downloads } });
            return downloads;
          }
        } catch (error) {
          console.error("Error fetching downloads:", error);
        }
        return [];
      },

      fetchOurApps: async () => {
        try {
          const res = await fetch(`${BASE_URL}/ourapps`);
          if (res.status === HTTP_STATUS.OK) {
            const data: AppListing[] = await res.json();
            set({ ourApps: data });
          }
        } catch (error) {
          console.error("Error fetching our apps:", error);
        }
      },

      fetchDownloadsForApp: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/downloads/${id}`);
          if (res.status === HTTP_STATUS.OK) {
            const downloads: DownloadItem[] = await res.json();
            set((state) => ({
              downloads: { ...state.downloads, [id]: downloads }
            }));
            return downloads;
          }
        } catch (error) {
          console.error("Error fetching downloads for app:", error);
        }
        return [];
      },

      fetchHomepageApps: async () => {
        try {
          const res = await fetch('/apps');
          if (res.status === HTTP_STATUS.OK) {
            const data: HomepageApp[] = await res.json();
            set({ homepageApps: data });
          }
        } catch (error) {
          console.error("Error fetching homepage apps:", error);
        }
      },

      getLaunchUrl: (id: string) => {
        const app = get().homepageApps.find(app => `${app.package}:${app.publisher}` === id);
        if (app && app.path) {
          return app.path;
        }
        return null;
      },

      checkMirror: async (node: string) => {
        try {
          const res = await fetch(`${BASE_URL}/mirrorcheck/${node}`);
          if (res.status === HTTP_STATUS.OK) {
            return await res.json() as MirrorCheckFile;
          }
        } catch (error) {
          console.error("Error checking mirror:", error);
        }
        return null;
      },

      installApp: async (id: string, version_hash: string) => {
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}/install`, {
            method: 'POST',
            body: JSON.stringify({ version_hash })
          });
          if (res.status === HTTP_STATUS.CREATED) {
            await get().fetchInstalled();
          }
        } catch (error) {
          console.error("Error installing app:", error);
        }
      },

      uninstallApp: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}`, { method: 'DELETE' });
          if (res.status === HTTP_STATUS.NO_CONTENT) {
            await get().fetchInstalled();
          }
        } catch (error) {
          console.error("Error uninstalling app:", error);
        }
      },

      downloadApp: async (id: string, version_hash: string, downloadFrom: string) => {
        const [package_name, publisher_node] = id.split(':');
        const appId = `${id}:${version_hash}`;
        set((state) => ({
          activeDownloads: {
            ...state.activeDownloads,
            [appId]: { downloaded: 0, total: 100 }
          }
        }));
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}/download`, {
            method: 'POST',
            body: JSON.stringify({
              package_id: { package_name, publisher_node },
              version_hash,
              download_from: downloadFrom,
            }),
          });
          if (res.status !== HTTP_STATUS.OK) {
            get().clearActiveDownload(appId);
          }
        } catch (error) {
          console.error("Error downloading app:", error);
          get().clearActiveDownload(appId);
        }
      },

      clearAllActiveDownloads: () => set({ activeDownloads: {} }),

      removeDownload: async (packageId: string, versionHash: string) => {
        try {
          const response = await fetch(`${BASE_URL}/downloads/${packageId}/remove`, {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({ version_hash: versionHash }),
          });
          if (response.ok) {
            await get().fetchDownloadsForApp(packageId);
          }
        } catch (error) {
          console.error('Failed to remove download:', error);
        }
      },

      getCaps: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}/caps`);
          if (res.status === HTTP_STATUS.OK) {
            return await res.json() as PackageManifest;
          }
        } catch (error) {
          console.error("Error getting caps:", error);
        }
        return null;
      },

      approveCaps: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/apps/${id}/caps`, { method: 'POST' });
          if (res.status === HTTP_STATUS.OK) {
            await get().fetchListing(id);
          }
        } catch (error) {
          console.error("Error approving caps:", error);
        }
      },

      startMirroring: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/downloads/${id}/mirror`, {
            method: 'PUT'
          });
          if (res.status === HTTP_STATUS.OK) {
            await get().fetchDownloadsForApp(id.split(':').slice(0, -1).join(':'));
          }
        } catch (error) {
          console.error("Error starting mirroring:", error);
        }
      },

      stopMirroring: async (id: string) => {
        try {
          const res = await fetch(`${BASE_URL}/downloads/${id}/mirror`, {
            method: 'DELETE'
          });
          if (res.status === HTTP_STATUS.OK) {
            await get().fetchDownloadsForApp(id.split(':').slice(0, -1).join(':'));
          }
        } catch (error) {
          console.error("Error stopping mirroring:", error);
        }
      },

      setAutoUpdate: async (id: string, version_hash: string, autoUpdate: boolean) => {
        try {
          const method = autoUpdate ? 'PUT' : 'DELETE';
          const res = await fetch(`${BASE_URL}/apps/${id}/auto-update`, {
            method,
            body: JSON.stringify({ version_hash })
          });
          if (res.status === HTTP_STATUS.OK) {
            await get().fetchListing(id);
          }
        } catch (error) {
          console.error("Error setting auto-update:", error);
        }
      },

      setActiveDownload: (appId, downloaded, total) => {
        set((state) => ({
          activeDownloads: {
            ...state.activeDownloads,
            [appId]: { downloaded, total }
          }
        }));
      },

      clearActiveDownload: (appId) => {
        set((state) => {
          const { [appId]: _, ...rest } = state.activeDownloads;
          return { activeDownloads: rest };
        });
      },

      ws: new KinodeClientApi({
        uri: WEBSOCKET_URL,
        nodeId: (window as any).our?.node,
        processId: "main:app_store:sys",
        onMessage: (message) => {
          console.log('WebSocket message received', message);
          try {
            const data = JSON.parse(message);
            if (data.kind === 'progress') {
              const { package_id, version_hash, downloaded, total } = data.data;
              const appId = `${package_id.package_name}:${package_id.publisher_node}:${version_hash}`;
              get().setActiveDownload(appId, downloaded, total);
            } else if (data.kind === 'complete') {
              const { package_id, version_hash } = data.data;
              const appId = `${package_id.package_name}:${package_id.publisher_node}:${version_hash}`;
              get().clearActiveDownload(appId);
              get().fetchData(`${package_id.package_name}:${package_id.publisher_node}`);
            }
          } catch (error) {
            console.error('Error parsing WebSocket message:', error);
          }
        },
        onOpen: (_e) => {
          console.log('WebSocket connection opened');
        },
        onClose: (_e) => {
          console.log('WebSocket connection closed');
        },
        onError: (error) => {
          console.error('WebSocket error:', error);
        },
      }),
    }),
    {
      name: 'app_store',
    }
  )
)

export default useAppsStore