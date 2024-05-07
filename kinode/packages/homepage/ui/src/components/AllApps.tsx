import classNames from "classnames"
import useHomepageStore, { HomepageApp } from "../store/homepageStore"
import { isMobileCheck } from "../utilities/dimensions"
import AppDisplay from "./AppDisplay"
import usePersistentStore from "../store/persistentStore"
import { useEffect, useState } from "react"

const AllApps: React.FC = () => {
  const { apps } = useHomepageStore()
  const { favoriteApps } = usePersistentStore()
  const isMobile = isMobileCheck()
  const [undockedApps, setUndockedApps] = useState<HomepageApp[]>([])

  useEffect(() => {
    setUndockedApps(apps.filter(a => !favoriteApps[a.package_name]))
  }, [apps, favoriteApps])

  return <div className={classNames('flex-center flex-wrap overflow-y-auto flex-grow self-stretch', {
    'px-8 gap-4': isMobile,
    'px-16 gap-8': !isMobile
  })}>
    {undockedApps.length === 0
      ? (apps && apps.length === 0)
        ? <div>Loading apps...</div>
        : <></>
      : undockedApps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AllApps