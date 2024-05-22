import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';

export interface PersistentStore {
  get: () => PersistentStore
  set: (state: PersistentStore | Partial<PersistentStore>) => void
  widgetSettings: {
    [key: string]: {
      hide?: boolean,
      size?: 'small' | 'large',
    }
  }
  setWidgetSettings: (widgetSettings: PersistentStore['widgetSettings']) => void
  toggleWidgetVisibility: (package_name: string) => void
  setWidgetSize: (package_name: string, size: 'small' | 'large') => void,
  favoriteApps: {
    [key: string]: {
      favorite: boolean
      order?: number
    }
  }
  setFavoriteApps: (favoriteApps: PersistentStore['favoriteApps']) => void
  favoriteApp: (package_name: string) => void
}

const usePersistentStore = create<PersistentStore>()(
  persist(
    (set, get) => ({
      get,
      set,
      widgetSettings: {},
      favoriteApps: {},
      setWidgetSettings: (widgetSettings: PersistentStore['widgetSettings']) => set({ widgetSettings }),
      setFavoriteApps: (favoriteApps: PersistentStore['favoriteApps']) => set({ favoriteApps }),
      toggleWidgetVisibility: (package_name: string) => {
        const { widgetSettings } = get()
        set({
          widgetSettings: {
            ...widgetSettings,
            [package_name]: {
              ...widgetSettings[package_name],
              hide: !widgetSettings[package_name]?.hide
            }
          }
        })
      },
      setWidgetSize: (package_name: string, size: 'small' | 'large') => {
        const { widgetSettings } = get()
        set({
          widgetSettings: {
            ...widgetSettings,
            [package_name]: {
              ...widgetSettings[package_name],
              size
            }
          }
        })
      },
      favoriteApp: async (package_name: string) => {
        const { favoriteApps } = get()
        set({
          favoriteApps: {
            ...favoriteApps,
            [package_name]: {
              ...favoriteApps[package_name],
              favorite: !favoriteApps[package_name]?.favorite
            }
          }
        })
      },
    }),
    {
      name: 'homepage_persistent_store', // unique name for the store
      storage: createJSONStorage(() => localStorage),
    }
  )
);

export default usePersistentStore;