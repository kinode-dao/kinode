import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'


export interface HomepageStore {
  get: () => HomepageStore
  set: (partial: HomepageStore | Partial<HomepageStore>) => void

  isHosted: boolean
  setIsHosted: (isHosted: boolean) => void
  fetchHostedStatus: (our: string) => Promise<void>
}

const useHomepageStore = create<HomepageStore>()(
  persist(
    (set, get) => ({
      get,
      set,

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
    }),
    {
      name: 'homepage_store', // unique name
      storage: createJSONStorage(() => sessionStorage), // (optional) by default, 'localStorage' is used
    }
  )
)

export default useHomepageStore
