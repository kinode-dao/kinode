import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"
import usePersistentStore from "../store/persistentStore"
import { useEffect, useState } from "react"
import { isMobileCheck } from "../utilities/dimensions"
import classNames from "classnames"

const AppsDock: React.FC = () => {
  const { apps } = useHomepageStore()
  const { favoriteApps } = usePersistentStore()
  const [dockedApps, setDockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    setDockedApps(apps.filter(a => favoriteApps[a.package_name]))
  }, [apps, favoriteApps])

  const isMobile = isMobileCheck()

  return <div className={classNames('flex-center flex-wrap border border-orange bg-orange/25 p-2 rounded !rounded-xl', {
    'gap-8 mb-4': !isMobile,
    'gap-4 mb-2': isMobile
  })}>
    {dockedApps.length === 0
      ? <div>Favorite an app to pin it to your dock.</div>
      : dockedApps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AppsDock