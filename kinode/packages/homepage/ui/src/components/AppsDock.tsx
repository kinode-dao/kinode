import { useEffect, useState } from "react"
import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const [favoriteApps, setFavoriteApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    setFavoriteApps(apps.filter(a => a.is_favorite))
  }, [apps])

  return <div className='flex-center flex-wrap obox border border-orange gap-8'>
    {favoriteApps.length === 0 ? <div>Favorite an app to pin it to your dock.</div> : favoriteApps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AppsDock