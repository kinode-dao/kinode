import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"
import usePersistentStore from "../store/persistentStore"
import { useEffect, useState } from "react"

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const { favoriteApps } = usePersistentStore()
  const [dockedApps, setDockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    setDockedApps(apps.filter(a => favoriteApps[a.package_name]))
  }, [apps, favoriteApps])

  return <div className='flex-center flex-wrap obox border border-orange gap-8 !rounded-xl'>
    {dockedApps.length === 0 ? <div>Favorite an app to pin it to your dock.</div> : dockedApps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AppsDock