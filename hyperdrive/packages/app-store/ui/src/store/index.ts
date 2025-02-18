import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { PackageState, AppListing, MirrorCheckFile, DownloadItem, HomepageApp, ManifestResponse, Notification, UpdateInfo } from '../types/Apps'
import { HTTP_STATUS } from '../constants/http'
import KinodeClientApi from "@kinode/client-api"
import { WEBSOCKET_URL } from '../utils/ws'

const BASE_URL = '/main:app-store:sys'

interface AppsStore {
  listings: Record<string, AppListing>
  installed: Record<string, PackageState>
  downloads: Record<string, DownloadItem[]>
  ourApps: AppListing[]
  ws: KinodeClientApi
  notifications: Notification[]
  homepageApps: HomepageApp[]
  activeDownloads: Record<string, { downloaded: number, total: number }>
  updates: Record<string, UpdateInfo>

  fetchData: (id: string) => Promise<void>
  fetchListings: () => Promise<void>
  fetchListing: (id: string) => Promise<AppListing | null>
  fetchInstalled: () => Promise<void>
  fetchInstalledApp: (id: string) => Promise<PackageState | null>
  fetchDownloads: () => Promise<DownloadItem[]>
  fetchOurApps: () => Promise<void>
  fetchDownloadsForApp: (id: string) => Promise<DownloadItem[]>
  checkMirror: (id: string, node: string) => Promise<MirrorCheckFile | null>
  resetStore: () => Promise<void>

  fetchHomepageApps: () => Promise<void>
  getLaunchUrl: (id: string) => string | null

  addNotification: (notification: Notification) => void;
  removeNotification: (id: string) => void;
  clearNotifications: () => void;

  installApp: (id: string, version_hash: string) => Promise<void>
  uninstallApp: (id: string) => Promise<void>
  downloadApp: (id: string, version_hash: string, downloadFrom: string) => Promise<void>
  removeDownload: (packageId: string, versionHash: string) => Promise<void>
  getManifest: (id: string, version_hash: string) => Promise<ManifestResponse | null>
  approveCaps: (id: string) => Promise<void>
  startMirroring: (id: string) => Promise<void>
  stopMirroring: (id: string) => Promise<void>
  setAutoUpdate: (id: string, version_hash: string, autoUpdate: boolean) => Promise<void>

  setActiveDownload: (appId: string, downloaded: number, total: number) => void
  clearActiveDownload: (appId: string) => void
  clearAllActiveDownloads: () => void;

  fetchUpdates: () => Promise<void>
  clearUpdates: (packageId: string) => Promise<void>
  checkMirrors: (packageId: string, onMirrorSelect: (mirror: string, status: boolean | null | 'http') => void) => Promise<{ mirror: string, status: boolean | null | 'http', mirrors: string[] } | { error: string, mirrors: string[] }>
}

const useAppsStore = create<AppsStore>()((set, get) => ({
  listings: {},
  installed: {},
  downloads: {},
  ourApps: [],
  activeDownloads: {},
  homepageApps: [],
  notifications: [],
  updates: {},

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
      const res = await fetch(`${BASE_URL}/homepageapps`);
      if (res.status === HTTP_STATUS.OK) {
        const data = await res.json();
        const apps = data.GetApps || [];
        set({ homepageApps: apps });
      }
    } catch (error) {
      console.error("Error fetching homepage apps:", error);
      set({ homepageApps: [] });
    }
  },

  getLaunchUrl: (id: string) => {
    const app = get().homepageApps?.find(app => `${app.package_name}:${app.publisher}` === id);
    return app?.path || null;
  },

  checkMirror: async (id: string, node: string) => {
    try {
      const res = await fetch(`${BASE_URL}/mirrorcheck/${id}/${node}`);
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
        await get().fetchHomepageApps();
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

  getManifest: async (id: string, version_hash: string) => {
    try {
      const res = await fetch(`${BASE_URL}/manifest?id=${id}&version_hash=${version_hash}`);
      if (res.status === HTTP_STATUS.OK) {
        return await res.json() as ManifestResponse;
      }
    } catch (error) {
      console.error("Error getting manifest:", error);
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


  addNotification: (notification) => set(state => ({
    notifications: [...state.notifications, notification]
  })),

  removeNotification: (id) => set(state => ({
    notifications: state.notifications.filter(n => n.id !== id)
  })),

  clearNotifications: () => set({ notifications: [] }),


  clearActiveDownload: (appId) => {
    set((state) => {
      const { [appId]: _, ...rest } = state.activeDownloads;
      return { activeDownloads: rest };
    });
  },

  fetchUpdates: async () => {
    try {
      const res = await fetch(`${BASE_URL}/updates`);
      if (res.status === HTTP_STATUS.OK) {
        const updates = await res.json();
        set({ updates });
      }
    } catch (error) {
      console.error("Error fetching updates:", error);
    }
  },

  clearUpdates: async (packageId: string) => {
    try {
      await fetch(`${BASE_URL}/updates/${packageId}/clear`, {
        method: 'POST',
      });
      set((state) => {
        const newUpdates = { ...state.updates };
        delete newUpdates[packageId];
        return { updates: newUpdates };
      });
    } catch (error) {
      console.error("Error clearing updates:", error);
    }
  },

  checkMirrors: async (packageId: string, onMirrorSelect: (mirror: string, status: boolean | null | 'http') => void) => {
    try {
      onMirrorSelect("", null); // Signal checking started

      const appData = await get().fetchListing(packageId);
      if (!appData) {
        onMirrorSelect("", false);
        return { error: "Failed to fetch app data", mirrors: [] };
      }

      const mirrors = Array.from(new Set([
        appData.package_id.publisher_node,
        ...(appData.metadata?.properties?.mirrors || [])
      ]));

      // check for http mirrors
      const httpMirrors = mirrors.filter(m => m.startsWith('http'));
      for (const mirror of httpMirrors) {
        onMirrorSelect(mirror, 'http');
        return { mirror, status: 'http' as const, mirrors };
      }

      // check node mirrors
      const nodeMirrors = mirrors.filter(m => !m.startsWith('http'));
      for (const mirror of nodeMirrors) {
        const status = await get().checkMirror(packageId, mirror);
        if (status?.is_online) {
          onMirrorSelect(mirror, true);
          return { mirror, status: true as const, mirrors };
        }
      }

      // upon reaching this, no online mirrors found.
      onMirrorSelect("", false);
      return { error: "No available mirrors found", mirrors };
    } catch (error) {
      console.error("Mirror check error:", error);
      onMirrorSelect("", false);
      return { error: "Failed to check mirrors", mirrors: [] };
    }
  },

  resetStore: async () => {
    try {
      const response = await fetch(`${BASE_URL}/reset`, {
        method: 'POST',
      });

      if (!response.ok) {
        throw new Error('Reset failed');
      }

      // Refresh the store data
      await Promise.all([
        get().fetchInstalled(),
        get().fetchListings(),
        get().fetchUpdates(),
      ]);
    } catch (error) {
      console.error('Reset failed:', error);
      throw error;
    }
  },

  ws: new KinodeClientApi({
    uri: WEBSOCKET_URL,
    nodeId: (window as any).our?.node,
    processId: "main:app-store:sys",
    onMessage: (message) => {
      console.log('WebSocket message received', message);
      try {
        const data = JSON.parse(message);
        if (data.kind === 'progress') {
          const { package_id, version_hash, downloaded, total } = data.data;
          const appId = `${package_id.package_name}:${package_id.publisher_node}:${version_hash}`;
          get().setActiveDownload(appId, downloaded, total);

          const existingNotification = get().notifications.find(
            n => n.id === `download-${appId}`
          );

          if (existingNotification) {
            get().removeNotification(`download-${appId}`);
          }

          get().addNotification({
            id: `download-${appId}`,
            type: 'download',
            message: `Downloading ${package_id.package_name}`,
            timestamp: Date.now(),
            metadata: {
              packageId: `${package_id.package_name}:${package_id.publisher_node}`,
              versionHash: version_hash,
              progress: Math.round((downloaded / total) * 100)
            }
          });
        } else if (data.kind === 'complete') {
          const { package_id, version_hash, error } = data.data;
          const appId = `${package_id.package_name}:${package_id.publisher_node}:${version_hash}`;
          get().clearActiveDownload(appId);
          get().removeNotification(`download-${appId}`);

          if (error) {
            const formatDownloadError = (error: any): string => {
              if (typeof error === 'object' && error !== null) {
                if ('HashMismatch' in error) {
                  const { actual, desired } = error.HashMismatch;
                  return `Hash mismatch: expected ${desired.slice(0, 8)}..., got ${actual.slice(0, 8)}...`;
                }
                // Try to serialize the error object if it's not a HashMismatch
                try {
                  return JSON.stringify(error);
                } catch {
                  return String(error);
                }
              }
              return String(error);
            };

            get().addNotification({
              id: `error-${appId}`,
              type: 'error',
              message: `Download failed for ${package_id.package_name}: ${formatDownloadError(error)}`,
              timestamp: Date.now(),
            });
          } else {
            get().addNotification({
              id: `complete-${appId}`,
              type: 'success',
              message: `Download complete: ${package_id.package_name}`,
              timestamp: Date.now(),
            });
          }

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
}))

export default useAppsStore
