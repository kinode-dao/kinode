import { HomepageApp } from "../store/homepageStore"
import AppDisplay from "./AppDisplay"

interface AllAppsProps {
  apps: HomepageApp[]
}

const AllApps: React.FC<AllAppsProps> = ({ apps }) => {
  return <div className='flex-center flex-wrap gap-8 overflow-y-auto flex-grow self-stretch px-16'>
    {apps.length === 0 ? <div>Loading apps...</div> : apps.map(app => <AppDisplay app={app} />)}
  </div>
}

export default AllApps