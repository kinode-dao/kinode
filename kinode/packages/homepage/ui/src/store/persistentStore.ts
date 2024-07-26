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
  toggleWidgetVisibility: (package_id: string) => void
  setWidgetSize: (package_id: string, size: 'small' | 'large') => void,
}

const usePersistentStore = create<PersistentStore>()(
  persist(
    (set, get) => ({
      get,
      set,
      widgetSettings: {},
      setWidgetSettings: (widgetSettings: PersistentStore['widgetSettings']) => set({ widgetSettings }),
      toggleWidgetVisibility: (package_id: string) => {
        const { widgetSettings } = get()
        set({
          widgetSettings: {
            ...widgetSettings,
            [package_id]: {
              ...widgetSettings[package_id],
              hide: !widgetSettings[package_id]?.hide
            }
          }
        })
      },
      setWidgetSize: (package_id: string, size: 'small' | 'large') => {
        const { widgetSettings } = get()
        set({
          widgetSettings: {
            ...widgetSettings,
            [package_id]: {
              ...widgetSettings[package_id],
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