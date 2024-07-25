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
}

const usePersistentStore = create<PersistentStore>()(
  persist(
    (set, get) => ({
      get,
      set,
      widgetSettings: {},
      setWidgetSettings: (widgetSettings: PersistentStore['widgetSettings']) => set({ widgetSettings }),
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
    }),
    {
      name: 'homepage_persistent_store', // unique name for the store
      storage: createJSONStorage(() => localStorage),
    }
  )
);

export default usePersistentStore;