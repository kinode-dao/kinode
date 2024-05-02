import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'

export interface HomepageApp {
  package_name: string,
  path: string
  label: string,
  base64_icon?: string,
  is_favorite: boolean,
  state?: {
    our_version: string
  }
}

export interface HomepageStore {
  get: () => HomepageStore
  set: (partial: HomepageStore | Partial<HomepageStore>) => void

  isHosted: boolean
  setIsHosted: (isHosted: boolean) => void
  fetchHostedStatus: (our: string) => Promise<void>
  favoriteApp: (appId: string) => Promise<Response>

  apps: HomepageApp[]
  setApps: (apps: HomepageApp[]) => void
}

const useHomepageStore = create<HomepageStore>()(
  persist(
    (set, get) => ({
      get,
      set,

      apps: [],
      setApps: (apps: HomepageApp[]) => set({ apps }),
      isHosted: false,
      setIsHosted: (isHosted: boolean) => set({ isHosted }),
      fetchHostedStatus: async (our: string) => {
        let hosted = false
        try {
          const res = await fetch(`https://${our.replace('.os', '')}.hosting.kinode.net/`)
          hosted = res.status === 200
        } catch (error) {
          // do nothing
        } finally {
          set({ isHosted: hosted })
        }
      },
      favoriteApp: async (appId: string) => await fetch(`/apps/${appId.replace('/', '')}/favorite`, { method: 'POST' }),
    }),
    {
      name: 'homepage_store', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useHomepageStore
