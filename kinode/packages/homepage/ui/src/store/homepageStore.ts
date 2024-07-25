import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'

export interface HomepageApp {
  package_name: string,
  path: string
  label: string,
  base64_icon?: string,
  state?: {
    our_version: string
  }
  widget?: string
  order?: number
}

export interface HomepageStore {
  get: () => HomepageStore
  set: (partial: HomepageStore | Partial<HomepageStore>) => void

  apps: HomepageApp[]
  setApps: (apps: HomepageApp[]) => void
  showWidgetsSettings: boolean
  setShowWidgetsSettings: (showWidgetsSettings: boolean) => void
}

const useHomepageStore = create<HomepageStore>()(
  persist(
    (set, get) => ({
      get,
      set,

      apps: [],
      setApps: (apps: HomepageApp[]) => set({ apps }),
      showWidgetsSettings: false,
      setShowWidgetsSettings: (showWidgetsSettings: boolean) => set({ showWidgetsSettings }),
    }),
    {
      name: 'homepage_store', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useHomepageStore
